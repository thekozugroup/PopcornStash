package com.thekozugroup.popcornstash.feature.downloads

import com.thekozugroup.popcornstash.data.api.Download
import com.thekozugroup.popcornstash.data.api.DownloadKind
import com.thekozugroup.popcornstash.data.api.DownloadState
import com.thekozugroup.popcornstash.data.api.Library
import com.thekozugroup.popcornstash.data.api.Place
import com.thekozugroup.popcornstash.feature.library.MediaFormat
import java.text.NumberFormat
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale
import kotlin.math.floor
import kotlin.math.roundToLong

// Downloads in plain words, in the web app's wording (web/src/components/downloads/downloadText.ts
// and web/src/lib/format.ts), so a download reads the same on the phone and in a browser.

/** Queued, downloading, paused or seeding: shown under In progress. */
fun Download.isActive(): Boolean = when (state) {
    DownloadState.Queued, DownloadState.Downloading, DownloadState.Paused, DownloadState.Seeding -> true
    else -> false
}

fun Download.canPause(): Boolean =
    state == DownloadState.Downloading || state == DownloadState.Queued || state == DownloadState.Seeding

fun Download.canResume(): Boolean = state == DownloadState.Paused

/** A failed link or magnet can be sent again; a failed upload needs the file shared again. */
fun Download.canRetry(): Boolean =
    state == DownloadState.Failed && kind != DownloadKind.Upload &&
        (source.startsWith("http://", true) || source.startsWith("https://", true) || source.startsWith("magnet:", true)) &&
        !source.endsWith("#redacted")

/** Until it is sorted, what arrived is still in Downloads and goes when the download is removed. */
fun Download.filesStillInDownloads(): Boolean = state != DownloadState.Sorted && state != DownloadState.Cancelled

/** 0..1 when the size is known (or something arrived), else null. */
fun Download.knownProgress(): Double? {
    val known = (totalBytes ?: 0) > 0
    return if (known || progress > 0) progress.coerceIn(0.0, 1.0) else null
}

fun Download.fetchingTorrentDetails(): Boolean = kind == DownloadKind.Torrent && state == DownloadState.Downloading &&
    (totalBytes ?: 0) <= 0 && doneBytes == 0L

/** One changing status; the icon already communicates the kind of transfer. */
fun Download.activeStatus(): String = when (state) {
    DownloadState.Queued -> error?.takeIf { it.isNotBlank() } ?: "Waiting to start"
    DownloadState.Paused -> "Paused"
    DownloadState.Seeding -> "Sharing with peers"
    else -> when {
        fetchingTorrentDetails() -> "Fetching torrent details"
        kind == DownloadKind.Torrent && (peers ?: 0) == 0L && downBps == 0L -> "Looking for peers"
        else -> "Downloading"
    }
}

/**
 * The facts under an in-progress row, spaced apart rather than joined
 * (android/DESIGN.md, Downloads): "64%  2.1 MB/s  12 min left", or the reason it waits.
 */
fun Download.progressFacts(locale: Locale = Locale.getDefault()): List<String> {
    val percent = knownProgress()?.let { formatPercent(it) }
    val size = when {
        (totalBytes ?: 0) > 0 -> formatBytesOf(doneBytes, totalBytes, locale)
        doneBytes > 0 -> "${formatBytes(doneBytes, locale)} so far"
        else -> null
    }
    return when (state) {
        // The server says why: "Waiting for the VPN to connect.", "Waiting for a free slot (5 at once)."
        DownloadState.Queued -> listOfNotNull(error?.takeIf { it.isNotBlank() } ?: "Waiting to start.")
        DownloadState.Paused -> listOfNotNull("Paused", percent, size)
        DownloadState.Seeding -> listOfNotNull(
            "Downloaded. Sharing with peers before sorting.",
            if (upBps > 0) "Sharing at ${formatSpeed(upBps, locale)}" else null,
        )
        else -> {
            if (fetchingTorrentDetails()) return listOf("Fetching torrent details. Waiting for a peer.")
            val lookingForPeers = kind == DownloadKind.Torrent && (peers ?: 0) == 0L && downBps == 0L
            listOfNotNull(
                percent,
                if (lookingForPeers) "Looking for peers" else formatSpeed(downBps, locale),
                formatEta(etaS),
            )
        }
    }
}

/** True when the facts are a sentence about waiting, not numbers. */
fun Download.isWaiting(): Boolean = state == DownloadState.Queued

// ------------------------------------------------------------------ finished ----

/** Where a finished download's files went, or why it did not finish. */
sealed interface FinishedOutcome {
    /** "In Kids", "In Kids and Other files"; [places] empty reads just "Sorted". */
    data class Sorted(val places: List<OutcomePlace>) : FinishedOutcome

    /** "2 waiting to import": the files wait in Ready to import. */
    data class Waiting(val count: Long) : FinishedOutcome

    data class Note(val text: String, val tone: NoteTone = NoteTone.Neutral) : FinishedOutcome
}

enum class NoteTone { Neutral, Error }

/** A place a sorted download's files are in now: a library (with its icon) or Other files. */
data class OutcomePlace(val name: String, val libraryId: String?, val icon: String?)

/**
 * Where a finished download's files went, from the server's `destinations` (only places
 * this person can see). A library the person cannot open is never named; a sorted
 * download with nothing left to show reads only "Sorted".
 */
fun Download.finishedOutcome(libraries: List<Library>, isOwner: Boolean): FinishedOutcome {
    when (state) {
        DownloadState.Failed -> return FinishedOutcome.Note(
            "Failed. ${error?.takeIf { it.isNotBlank() } ?: "The download stopped with an error."}",
            NoteTone.Error,
        )
        DownloadState.Cancelled -> return FinishedOutcome.Note("Cancelled before it finished.")
        DownloadState.Completed -> return FinishedOutcome.Note(
            if (isOwner) "Finished. Waiting in Downloads/Completed to be sorted." else "Finished. Waiting to be sorted.",
        )
        DownloadState.Sorting -> return FinishedOutcome.Note("Sorting into your library.")
        else -> Unit
    }
    if (destinations.isEmpty()) {
        // Owners see every file, so nothing to show means nothing came in. Anyone else may
        // have had it sorted into a library they cannot see: that is only ever "Sorted".
        return if (isOwner && itemIds.isEmpty()) {
            FinishedOutcome.Note("No videos or photos inside. The files stay in Downloads/Completed.")
        } else {
            FinishedOutcome.Sorted(emptyList())
        }
    }
    val waiting = destinations.filter { it.place == Place.Incoming }.sumOf { it.count }
    if (waiting > 0) return FinishedOutcome.Waiting(waiting)
    val places = destinations.mapNotNull { destination ->
        when (destination.place) {
            Place.OtherFiles -> OutcomePlace("Other files", null, null)
            Place.Library -> libraries.firstOrNull { it.id == destination.libraryId }
                ?.let { OutcomePlace(it.name, it.id, it.icon) }
            else -> null
        }
    }.distinct()
    return FinishedOutcome.Sorted(places)
}

/** The outcome as one line of text ("In Kids and Other files", "2 waiting to import"). */
fun FinishedOutcome.text(): String = when (this) {
    is FinishedOutcome.Sorted -> if (places.isEmpty()) "Sorted" else "In ${joinWords(places.map { it.name })}"
    is FinishedOutcome.Waiting -> "$count waiting to import"
    is FinishedOutcome.Note -> text
}

/** "Finished 3 min ago", or "Started 3 min ago" for one that failed. */
fun Download.finishedWhen(nowSeconds: Long, locale: Locale = Locale.getDefault()): String {
    val failed = state == DownloadState.Failed || state == DownloadState.Cancelled
    val at = if (failed) createdAt else completedAt ?: createdAt
    val ago = formatAgo(at, nowSeconds, locale)
    return "${if (failed) "Started" else "Finished"} ${ago.replaceFirstChar { it.lowercase(locale) }}"
}

// ---------------------------------------------------------------- formatting ----

private val BYTE_UNITS = listOf("B", "KB", "MB", "GB", "TB", "PB")

/** 0 B, 812 KB, 4.7 GB, 1.21 TB (decimal units, as disks report them). */
fun formatBytes(bytes: Long, locale: Locale = Locale.getDefault()): String {
    if (bytes < 0) return "Unknown size"
    if (bytes < 1000) return "$bytes B"
    var value = bytes.toDouble()
    var unit = 0
    while (value >= 1000 && unit < BYTE_UNITS.lastIndex) {
        value /= 1000
        unit++
    }
    val digits = when {
        value >= 100 -> 0
        value >= 10 -> 1
        else -> 2
    }
    val format = NumberFormat.getNumberInstance(locale).apply {
        maximumFractionDigits = digits
        minimumFractionDigits = 0
    }
    return "${format.format(value)} ${BYTE_UNITS[unit]}"
}

/** "3.4 GB of 7.9 GB" */
fun formatBytesOf(done: Long, total: Long?, locale: Locale = Locale.getDefault()): String =
    if (total == null || total <= 0) formatBytes(done, locale) else "${formatBytes(done, locale)} of ${formatBytes(total, locale)}"

/** "2.1 MB/s"; nothing moving reads "Idle". */
fun formatSpeed(bytesPerSecond: Long, locale: Locale = Locale.getDefault()): String =
    if (bytesPerSecond <= 0) "Idle" else "${formatBytes(bytesPerSecond, locale)}/s"

/** "64%", never rounding up to 100% before the end. */
fun formatPercent(fraction: Double): String = "${floor(fraction.coerceIn(0.0, 1.0) * 100).toInt()}%"

/** "12 min left", "Less than a minute left", "2 h 5 min left", "3 days left"; null when unknown. */
fun formatEta(seconds: Long?): String? {
    if (seconds == null || seconds < 0) return null
    if (seconds < 60) return "Less than a minute left"
    val minutes = (seconds / 60.0).roundToLong()
    if (minutes < 60) return "$minutes min left"
    val hours = minutes / 60
    val rest = minutes % 60
    if (hours < 48) return if (rest > 0) "$hours h $rest min left" else "$hours h left"
    return "${(hours / 24.0).roundToLong()} days left"
}

/** "Just now", "3 min ago", "2 h ago", "Yesterday", "4 days ago", then the date ("12 Oct"). */
fun formatAgo(unixSeconds: Long, nowSeconds: Long, locale: Locale = Locale.getDefault()): String {
    val delta = (nowSeconds - unixSeconds).coerceAtLeast(0)
    return when {
        delta < 45 -> "Just now"
        delta < 3_600 -> "${(delta / 60.0).roundToLong().coerceAtLeast(1)} min ago"
        delta < 86_400 -> "${delta / 3_600} h ago"
        delta < 2 * 86_400 -> "Yesterday"
        delta < 7 * 86_400 -> "${delta / 86_400} days ago"
        // A moment, so in the phone's own zone; the day and month stay on one line.
        else -> MediaFormat.unbroken(
            DateTimeFormatter.ofPattern("d MMM", locale).format(Instant.ofEpochSecond(unixSeconds).atZone(ZoneId.systemDefault())),
        )
    }
}

/** "1 file", "3 files". */
fun countOf(count: Long, singular: String, plural: String = "${singular}s"): String =
    "$count ${if (count == 1L) singular else plural}"

/** "Kids", "Kids and Films", "Kids, Films and Other files". */
fun joinWords(words: List<String>): String = when (words.size) {
    0 -> ""
    1 -> words[0]
    else -> "${words.dropLast(1).joinToString(", ")} and ${words.last()}"
}

/**
 * A file name that may break after dots, underscores and hyphens
 * ("Harbor.Lights.2019.1080p" wraps at its dots instead of overflowing). The extension stays
 * with the word before it, so a line never holds "zip" or "mp4" alone.
 */
fun breakableName(name: String): String = name.replace(NAME_BREAK, "$1​")

/** Dots, underscores and hyphens, but not the dot of an extension (".zip" at the end or before a space). */
private val NAME_BREAK = Regex("([_-]|\\.(?![A-Za-z0-9]{1,5}(?:\\s|$)))")
