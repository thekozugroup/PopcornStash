package com.thekozugroup.popcornstash.feature.downloads

import okhttp3.HttpUrl.Companion.toHttpUrlOrNull
import java.net.URLDecoder

// What someone pasted or shared: a magnet link, a link to a .torrent file, or an ordinary
// web link (web/src/components/downloads/linkDetect.ts, the same rules and sentences).
// Used by the Add a link sheet (as the person types, and the clipboard suggestion) and by
// the share sheet (the first link in shared text).

enum class LinkKind { Magnet, TorrentUrl, Http }

data class DetectedLink(
    val kind: LinkKind,
    /** The exact link to send to `POST /api/downloads`. */
    val url: String,
    /** Web links: the host ("download.blender.org"). */
    val host: String?,
    /** A readable name: the magnet's `dn`, or the last part of the web address. */
    val name: String?,
)

sealed interface LinkCheck {
    data object Empty : LinkCheck

    /** [extracted]: the link was found inside longer text. */
    data class Ok(val link: DetectedLink, val extracted: Boolean) : LinkCheck

    data class Invalid(val message: String) : LinkCheck
}

const val ONLY_LINKS_MESSAGE = "Only web links (http or https) and magnet links can be downloaded."
private const val INCOMPLETE = "This web address is incomplete. Copy the whole link and paste it again."

private val SCHEME = Regex("^([a-z][a-z0-9+.-]*):", RegexOption.IGNORE_CASE)
private val MAGNET_HASH = Regex("(?:^|[?&])xt=urn:(?:btih|btmh):[^&]+", RegexOption.IGNORE_CASE)
private val FIND_LINK = Regex("\\b(?:magnet:\\?[^\\s\"'<>]+|https?://[^\\s\"'<>]+)", RegexOption.IGNORE_CASE)
private val BARE_DOMAIN = Regex("^(?:www\\.)?[a-z0-9-]+(?:\\.[a-z0-9-]+)+(?::\\d+)?(?:[/?#]\\S*)?$", RegexOption.IGNORE_CASE)
private val TRAILING_PUNCTUATION = Regex("[).,;:!?'\"\\]}>]+$")
private val DOMAIN_LIKE = Regex("^[a-z0-9-]+\\.[a-z]", RegexOption.IGNORE_CASE)

private fun decodeSafely(value: String): String =
    runCatching { URLDecoder.decode(value.replace("+", " "), "UTF-8") }.getOrDefault(value)

private fun magnetName(url: String): String? {
    val query = url.substringAfter('?', "")
    for (part in query.split('&')) {
        val key = part.substringBefore('=')
        val value = part.substringAfter('=', "")
        if (key.equals("dn", ignoreCase = true) && value.isNotEmpty()) {
            val name = decodeSafely(value).trim()
            if (name.isNotEmpty()) return name
        }
    }
    return null
}

private sealed interface Parsed {
    data class Link(val link: DetectedLink) : Parsed

    data class Error(val message: String) : Parsed
}

private fun parseMagnet(url: String): Parsed {
    val query = url.substring(url.indexOf('?').coerceAtLeast(0))
    if (!MAGNET_HASH.containsMatchIn(query)) {
        return Parsed.Error("This magnet link is missing its torrent hash. Copy the whole link and paste it again.")
    }
    return Parsed.Link(DetectedLink(LinkKind.Magnet, url, null, magnetName(url)))
}

private fun parseWeb(url: String): Parsed {
    val parsed = url.toHttpUrlOrNull() ?: return Parsed.Error(INCOMPLETE)
    if (parsed.host.isEmpty()) return Parsed.Error(INCOMPLETE)
    val last = parsed.pathSegments.lastOrNull { it.isNotEmpty() }?.let(::decodeSafely)
    val isTorrent = parsed.encodedPath.lowercase().endsWith(".torrent")
    return Parsed.Link(
        DetectedLink(
            kind = if (isTorrent) LinkKind.TorrentUrl else LinkKind.Http,
            url = parsed.toString(),
            host = parsed.host.removePrefix("www."),
            name = last?.takeIf { it.isNotEmpty() },
        ),
    )
}

private fun parseOne(candidate: String): Parsed? {
    val scheme = SCHEME.find(candidate)?.groupValues?.get(1)?.lowercase() ?: return null
    return when (scheme) {
        "magnet" -> parseMagnet(candidate)
        "http", "https" -> parseWeb(candidate)
        else -> Parsed.Error(ONLY_LINKS_MESSAGE)
    }
}

/** The first magnet or http(s) link inside any text, or null. */
fun findLink(text: CharSequence?): DetectedLink? {
    if (text.isNullOrEmpty()) return null
    val match = FIND_LINK.find(text) ?: return null
    val candidate = match.value.replace(TRAILING_PUNCTUATION, "")
    return (parseOne(candidate) as? Parsed.Link)?.link
}

/** Checks the field's value as the person types. */
fun checkLink(raw: String): LinkCheck {
    val value = raw.trim()
    if (value.isEmpty()) return LinkCheck.Empty

    // A single token with a scheme: judge it as written.
    if (value.none { it.isWhitespace() } && SCHEME.containsMatchIn(value) && !DOMAIN_LIKE.containsMatchIn(value)) {
        when (val parsed = parseOne(value)) {
            is Parsed.Error -> return LinkCheck.Invalid(parsed.message)
            is Parsed.Link -> return LinkCheck.Ok(parsed.link, extracted = false)
            null -> Unit
        }
    }

    // Longer text (a pasted message): use the first link inside it.
    findLink(value)?.let { return LinkCheck.Ok(it, extracted = true) }

    if (BARE_DOMAIN.matches(value)) return LinkCheck.Invalid("Add https:// to the start of the web address.")
    if (SCHEME.containsMatchIn(value)) return LinkCheck.Invalid(ONLY_LINKS_MESSAGE)
    return LinkCheck.Invalid("This does not look like a link. Paste a web link or a magnet link.")
}

/**
 * One sentence about a recognised link. Torrents run only through the VPN, unless the
 * server runs them without one ([viaVpn] false).
 */
fun describeLink(link: DetectedLink, viaVpn: Boolean = true): String {
    val route = if (viaVpn) ", only through the VPN" else ""
    return when (link.kind) {
        LinkKind.Magnet -> "Magnet link. Downloads as a torrent$route."
        LinkKind.TorrentUrl -> "Link to a .torrent file on ${link.host ?: "the web"}. Downloads as a torrent$route."
        LinkKind.Http -> "Web link from ${link.host ?: "the web"}. Downloads straight to PopcornStash."
    }
}

fun DetectedLink.isTorrent(): Boolean = kind == LinkKind.Magnet || kind == LinkKind.TorrentUrl

/** What a link is called on screen: the magnet's name or the file at the end of the address. */
fun DetectedLink.displayName(): String = name ?: host ?: url

/** A link that may wrap after its separators, so two lines fill instead of breaking once after "magnet:?". */
fun breakableUrl(url: String): String = url.replace(Regex("([/?&=:._-])"), "$1​")
