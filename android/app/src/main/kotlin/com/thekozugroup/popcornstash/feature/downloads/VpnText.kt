package com.thekozugroup.popcornstash.feature.downloads

import com.thekozugroup.popcornstash.data.api.VpnState
import com.thekozugroup.popcornstash.data.api.VpnStatus

// The VPN line in the web app's words (web/src/lib/vpn.ts and components/downloads/VpnLine.tsx):
// every state says what happens to torrents.

enum class VpnTone { Connected, Warning, Error, Neutral }

/** The line under Downloads' top bar: a status label with its dot, and one sentence. */
data class VpnLine(
    val tone: VpnTone,
    /** "Protected · NordVPN · Netherlands", "Torrents paused: VPN connecting". */
    val label: String,
    /** What it means for torrents; empty while the status is being checked (the label says it). */
    val detail: String,
    /** For the owner: the line opens the web VPN settings. */
    val opensSettings: Boolean,
)

private const val TORRENTS_OFF_FALLBACK = "Torrents are turned off on this server."
private const val STILL_WORK = "Web links and uploads still work."

private fun providerName(provider: String?): String? = when (provider) {
    null -> null
    "nordvpn" -> "NordVPN"
    "wireguard" -> "WireGuard"
    "openvpn" -> "OpenVPN"
    else -> provider
}

/** "NordVPN · Netherlands"; an uploaded file keeps the server's label ("WireGuard · home-router"). */
fun vpnName(status: VpnStatus): String? {
    val base = if (status.provider == "nordvpn") "NordVPN" else status.label ?: providerName(status.provider)
    if (base == null) return status.country
    val country = status.country
    if (country == null || base.split(" · ").contains(country)) return base
    return "$base · $country"
}

/** Torrents run without a VPN on this server (`STASH_ALLOW_NO_VPN`): the permanent warning. */
fun isNoVpnMode(status: VpnStatus?): Boolean = status?.allowNoVpn == true && !status.torrentsOff

/** True when a new torrent waits for the VPN instead of starting. */
fun torrentsWait(status: VpnStatus?): Boolean {
    if (status == null) return true
    if (status.torrentsOff || isNoVpnMode(status)) return false
    return !(status.state == VpnState.Connected && status.protected != false)
}

private fun offReason(status: VpnStatus): String? =
    status.error?.trim()?.takeIf { it.isNotEmpty() && it != TORRENTS_OFF_FALLBACK }

private fun offDetail(status: VpnStatus): String = listOfNotNull(offReason(status), STILL_WORK).joinToString(" ")

/**
 * The Downloads page's VPN line, or null to leave it out. When the server does not answer
 * ([serverQuiet]: the page failed to load, or the live events are cut off) and nothing is known
 * about the VPN, the page already says so once; a line would only repeat it, or claim that links
 * and uploads still work when nothing does. A status read before stays. [asked]: the page has
 * asked for the status at least once, so a missing one could not be read.
 */
fun pageVpnLine(status: VpnStatus?, asked: Boolean, serverQuiet: Boolean, isOwner: Boolean): VpnLine? =
    if (status == null && serverQuiet) null else vpnLine(status, unavailable = asked && status == null, isOwner = isOwner)

/**
 * The VPN line for this person. [unavailable] is true when the server answered but its VPN
 * status could not be read (when the server itself does not answer, see [pageVpnLine]). Owners get the owner's sentences and a way into the VPN settings;
 * members get what still works.
 */
fun vpnLine(status: VpnStatus?, unavailable: Boolean, isOwner: Boolean): VpnLine {
    if (status == null) {
        // Each state is named once: the label says what is wrong, the detail what still works.
        return if (unavailable) {
            VpnLine(VpnTone.Neutral, "VPN status unavailable", STILL_WORK, isOwner)
        } else {
            VpnLine(VpnTone.Neutral, "Checking VPN", "", false)
        }
    }
    if (isNoVpnMode(status)) {
        return VpnLine(
            VpnTone.Warning,
            "No VPN: torrents use this network",
            if (isOwner) "STASH_ALLOW_NO_VPN is on. Your router must protect the traffic." else "The server is set up this way.",
            isOwner,
        )
    }
    return when (status.state) {
        VpnState.Connected -> if (status.protected == false) {
            VpnLine(
                VpnTone.Error,
                "Torrents blocked: VPN not protecting",
                "The tunnel is up, but the public address is not a VPN address. Torrents stay stopped.",
                isOwner,
            )
        } else {
            val parts = listOf("Protected") + (vpnName(status)?.split(" · ") ?: emptyList())
            VpnLine(
                VpnTone.Connected,
                parts.filter { it.isNotBlank() }.joinToString(" · "),
                status.publicIp?.let { "Torrents run only through the VPN. Public address $it." }
                    ?: "Torrents run only through the VPN.",
                isOwner,
            )
        }
        VpnState.Connecting -> VpnLine(
            VpnTone.Warning,
            "Torrents paused: VPN connecting",
            "Torrents wait until the VPN connects. $STILL_WORK",
            // While the VPN connects there is nothing to do but wait.
            false,
        )
        VpnState.Disconnected -> VpnLine(
            VpnTone.Warning,
            "Torrents paused: VPN disconnected",
            "Torrents wait until the VPN connects. $STILL_WORK",
            isOwner,
        )
        VpnState.Error -> VpnLine(
            VpnTone.Error,
            "Torrents blocked: VPN error",
            if (isOwner) {
                status.error ?: "The VPN could not connect. Torrents stay stopped until it does."
            } else {
                "Torrents are stopped until the owner fixes the VPN. $STILL_WORK"
            },
            isOwner,
        )
        VpnState.Unreachable -> if (status.torrentsOff) {
            VpnLine(VpnTone.Error, "Torrents off", offDetail(status), isOwner)
        } else {
            VpnLine(
                VpnTone.Error,
                "Torrents unavailable",
                if (isOwner) {
                    status.error ?: "The torrent engine is not answering. Torrents stay stopped until it is back."
                } else {
                    "Torrents are stopped until the owner fixes the VPN. $STILL_WORK"
                },
                isOwner,
            )
        }
        VpnState.NotConfigured, VpnState.Unknown -> VpnLine(
            VpnTone.Error,
            "Torrents blocked: no VPN",
            if (isOwner) {
                "Set up a VPN to start torrents. $STILL_WORK"
            } else {
                "Torrents need a VPN, and the owner has not set one up yet. $STILL_WORK"
            },
            isOwner,
        )
    }
}
