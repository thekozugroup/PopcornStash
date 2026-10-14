package com.thekozugroup.popcornstash.feature.library

import com.thekozugroup.popcornstash.data.api.AudioStream
import com.thekozugroup.popcornstash.data.api.Category
import com.thekozugroup.popcornstash.data.api.Item
import com.thekozugroup.popcornstash.data.api.MediaKind
import com.thekozugroup.popcornstash.data.api.Progress
import com.thekozugroup.popcornstash.data.api.SubtitleStream
import com.thekozugroup.popcornstash.data.api.VideoStream
import java.text.NumberFormat
import java.time.Instant
import java.time.YearMonth
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.Locale
import kotlin.math.roundToInt
import kotlin.math.roundToLong

/**
 * The words and numbers the library pages show, the same as the web app's
 * (web/src/lib/format.ts, web/src/components/library/libraryFormat.ts): sentence case,
 * decimal sizes, the phone's own date and number style, two facts joined by a middle dot.
 */
class MediaFormat(
    private val locale: Locale = Locale.getDefault(),
    private val zone: ZoneId = ZoneId.systemDefault(),
) {
    private val numbers: NumberFormat get() = NumberFormat.getIntegerInstance(locale)

    fun number(value: Long): String = numbers.format(value)

    /** formatCount(1, "photo") is "1 photo"; formatCount(3, "photo") is "3 photos". */
    fun count(value: Long, singular: String, plural: String = "${singular}s"): String =
        "${number(value)} ${if (value == 1L) singular else plural}"

    /** "0 B", "812 KB", "4.7 GB" (1 GB = 1000 MB, as disks report). */
    fun bytes(value: Long): String {
        if (value < 0) return "Unknown size"
        if (value < 1000) return "$value B"
        var amount = value.toDouble()
        var unit = 0
        while (amount >= 1000 && unit < UNITS.lastIndex) {
            amount /= 1000
            unit++
        }
        val digits = if (amount >= 100) 0 else if (amount >= 10) 1 else 2
        val format = NumberFormat.getNumberInstance(locale).apply {
            maximumFractionDigits = digits
            minimumFractionDigits = 0
        }
        return "${format.format(amount)} ${UNITS[unit]}"
    }

    /** Runtime in words: "1 h 42 min", "42 min", "38 s". */
    fun duration(seconds: Double?): String {
        if (seconds == null || seconds.isNaN() || seconds < 0) return ""
        val total = seconds.roundToLong()
        val hours = total / 3600
        val minutes = (total % 3600) / 60
        return when {
            hours > 0 && minutes > 0 -> "$hours h $minutes min"
            hours > 0 -> "$hours h"
            minutes > 0 -> "$minutes min"
            else -> "$total s"
        }
    }

    /** The same, for a screen reader: "1 hour 42 minutes", "12 minutes". */
    fun spokenDuration(seconds: Double): String {
        val total = seconds.roundToLong().coerceAtLeast(0)
        val hours = total / 3600
        val minutes = (total % 3600) / 60
        val parts = buildList {
            if (hours > 0) add(count(hours, "hour"))
            if (minutes > 0) add(count(minutes, "minute"))
        }
        return parts.joinToString(" ").ifEmpty { "less than a minute" }
    }

    /** Player clock: "1:42:05", "42:05", "0:38". */
    fun clock(seconds: Double): String {
        val total = seconds.toLong().coerceAtLeast(0)
        val hours = total / 3600
        val minutes = (total % 3600) / 60
        val secs = total % 60
        return if (hours > 0) "%d:%02d:%02d".format(locale, hours, minutes, secs) else "%d:%02d".format(locale, minutes, secs)
    }

    /** "12 Oct 2026", in the phone's order. */
    fun date(unixSeconds: Long): String =
        DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM).withLocale(locale).format(at(unixSeconds))

    /** "12 Oct 2026, 14:05", in the phone's order. */
    fun dateTime(unixSeconds: Long): String =
        DateTimeFormatter.ofLocalizedDateTime(FormatStyle.MEDIUM, FormatStyle.SHORT).withLocale(locale).format(at(unixSeconds))

    /** "14:05" or "2:05 PM", as the phone tells time. */
    fun time(unixSeconds: Long): String =
        DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).withLocale(locale).format(at(unixSeconds))

    /** "2024-06" is "June 2024". */
    fun month(month: String): String = runCatching {
        DateTimeFormatter.ofPattern("LLLL yyyy", locale).format(YearMonth.parse(month))
            .replaceFirstChar { it.titlecase(locale) }
    }.getOrDefault(month)

    private fun at(unixSeconds: Long) = Instant.ofEpochSecond(unixSeconds).atZone(zone)

    /** The watched part of a video, or null when there is nothing worth a bar. */
    fun watchedFraction(progress: Progress?): Float? {
        if (progress == null || progress.finished || progress.durationS <= 0) return null
        val fraction = progress.positionS / progress.durationS
        if (fraction <= 0.01) return null
        return fraction.coerceAtMost(1.0).toFloat()
    }

    /** What is left of a started video: "12 min left", "Less than a minute left"; empty when unknown. */
    fun timeLeft(item: Item): String {
        val left = secondsLeft(item) ?: return ""
        return if (left < 60) "Less than a minute left" else "${duration(left)} left"
    }

    fun secondsLeft(item: Item): Double? {
        val duration = item.progress?.durationS?.takeIf { it > 0 } ?: item.durationS ?: return null
        val left = duration - (item.progress?.positionS ?: 0.0)
        return left.takeIf { duration > 0 && it > 0 }
    }

    /** One line under a tile: year and runtime, the date, or the type and size. */
    fun tileLine(item: Item): String = when (item.category) {
        Category.Movie -> join(item.year?.toString(), duration(item.durationS))
        Category.Episode -> join(episodeCode(item.season, item.episode), duration(item.durationS))
        Category.HomeVideo -> join(date(item.takenAt ?: item.addedAt), duration(item.durationS))
        Category.Photo -> if (hasFileNameTitle(item)) item.camera.orEmpty() else date(item.takenAt ?: item.addedAt)
        else -> fileLine(item)
    }

    /** "PDF · 592 B", "MP3 · 3 min". */
    fun fileLine(item: Item): String =
        join(fileType(item.relPath), if (item.durationS != null && item.durationS > 0) duration(item.durationS) else bytes(item.size))

    /** The line under an item's title on its page. */
    fun detailLine(item: Item): String = when (item.category) {
        Category.Movie -> join(item.year?.toString(), duration(item.durationS))
        Category.Episode -> {
            val where = if (item.season != null && item.episode != null) {
                if (item.season == 0) "Special ${item.episode}" else "Season ${item.season}, episode ${item.episode}"
            } else {
                null
            }
            join(where, duration(item.durationS))
        }
        Category.HomeVideo -> join(
            item.takenAt?.let { "Recorded ${dateTime(it)}" } ?: "Added ${date(item.addedAt)}",
            duration(item.durationS),
        )
        Category.Photo -> when {
            item.takenAt == null -> "Added ${date(item.addedAt)}"
            hasFileNameTitle(item) -> "Taken at ${time(item.takenAt)}"
            else -> "Taken ${dateTime(item.takenAt)}"
        }
        else -> fileLine(item)
    }

    /** "1080p, H.264"; "4K HDR10, HEVC". */
    fun videoSummary(video: VideoStream?): String? {
        if (video == null) return null
        val quality = join(resolutionName(video.width, video.height), hdrName(video.hdr), separator = " ")
        return join(quality, videoCodecName(video.codec), separator = ", ").ifEmpty { null }
    }

    /** The first track in words, and how many more: "English, AAC stereo, 1 more". */
    fun audioSummary(audio: List<AudioStream>): String? {
        val first = audio.firstOrNull() ?: return null
        val detail = join(audioCodecName(first.codec), channelsName(first.channels), separator = " ")
        val head = join(languageName(first.language), detail, separator = ", ")
        return if (audio.size > 1) "$head, ${audio.size - 1} more" else head
    }

    fun subtitleSummary(subtitles: List<SubtitleStream>): String? {
        if (subtitles.isEmpty()) return null
        val languages = subtitles.map { languageName(it.language) ?: "Unknown" }.distinct()
        return if (languages.size > 3) "${languages.take(3).joinToString(", ")} and ${languages.size - 3} more" else languages.joinToString(", ")
    }

    /** "eng" is "English"; null for an unknown or undetermined language. */
    fun languageName(code: String?): String? {
        if (code.isNullOrBlank() || code in setOf("und", "mul", "zxx")) return null
        val tag = ISO_639_2[code.lowercase(Locale.ROOT)] ?: code
        val name = Locale.forLanguageTag(tag).getDisplayLanguage(locale)
        return name.takeIf { it.isNotBlank() && !it.equals(tag, ignoreCase = true) }
            ?.replaceFirstChar { it.titlecase(locale) } ?: code.uppercase(Locale.ROOT)
    }

    fun join(vararg parts: String?, separator: String = " · "): String =
        parts.filterNot { it.isNullOrBlank() }.joinToString(separator)

    companion object {
        private val UNITS = listOf("B", "KB", "MB", "GB", "TB", "PB")

        /** The file's name from its stash path. */
        fun fileName(relPath: String): String = relPath.substringAfterLast('/')

        /** The extension in capitals ("PDF", "MP3"), or null. */
        fun fileType(relPath: String): String? {
            val name = fileName(relPath)
            val dot = name.lastIndexOf('.')
            if (dot <= 0 || dot == name.lastIndex) return null
            return name.substring(dot + 1).uppercase(Locale.ROOT)
        }

        /** "S01E02", or "S01" without an episode, or empty. */
        fun episodeCode(season: Int?, episode: Int?): String {
            if (season == null) return ""
            val s = "S%02d".format(Locale.ROOT, season)
            return if (episode == null) s else s + "E%02d".format(Locale.ROOT, episode)
        }

        /** "Season 2", or "Specials" for season 0. */
        fun seasonName(season: Int): String = if (season == 0) "Specials" else "Season $season"

        /** A photo still named after its camera file (IMG_2041): its date is a better name. */
        fun hasFileNameTitle(item: Item): Boolean {
            if (item.category != Category.Photo || item.takenAt == null) return false
            val name = fileName(item.relPath)
            val dot = name.lastIndexOf('.')
            return item.title == (if (dot > 0) name.substring(0, dot) else name)
        }

        /** An episode's name inside its show: its own title, else "Episode 3" ("Special 1" for season 0). */
        fun episodeName(item: Item): String {
            item.episodeTitle?.takeIf { it.isNotBlank() }?.let { return it }
            val episode = item.episode ?: return item.title
            return if (item.season == 0) "Special $episode" else "Episode $episode"
        }

        fun resolutionName(width: Int, height: Int): String? {
            if (width <= 0 || height <= 0) return null
            val long = maxOf(width, height)
            val short = minOf(width, height)
            return when {
                long >= 3800 -> "4K"
                long >= 2500 -> "1440p"
                long >= 1900 || short >= 1060 -> "1080p"
                long >= 1260 || short >= 700 -> "720p"
                else -> "${short}p"
            }
        }

        fun hdrName(hdr: String?): String? = hdr?.let { HDR_NAMES[it.lowercase(Locale.ROOT)] ?: "HDR" }

        fun videoCodecName(codec: String): String = VIDEO_CODECS[codec.lowercase(Locale.ROOT)] ?: codec.uppercase(Locale.ROOT)

        fun audioCodecName(codec: String): String = AUDIO_CODECS[codec.lowercase(Locale.ROOT)] ?: codec.uppercase(Locale.ROOT)

        fun channelsName(channels: Int?): String? = when (channels) {
            null, 0 -> null
            1 -> "mono"
            2 -> "stereo"
            6 -> "5.1"
            8 -> "7.1"
            else -> "$channels channels"
        }

        /** Percent watched for a screen reader, 0 to 100. */
        fun percent(fraction: Float): Int = (fraction * 100).roundToInt()

        private val VIDEO_CODECS = mapOf(
            "h264" to "H.264", "hevc" to "HEVC", "h265" to "HEVC", "av1" to "AV1", "vp9" to "VP9", "vp8" to "VP8",
            "mpeg4" to "MPEG-4", "mpeg2video" to "MPEG-2", "prores" to "ProRes",
        )
        private val AUDIO_CODECS = mapOf(
            "aac" to "AAC", "mp3" to "MP3", "opus" to "Opus", "flac" to "FLAC", "vorbis" to "Vorbis",
            "ac3" to "Dolby Digital", "eac3" to "Dolby Digital Plus", "truehd" to "Dolby TrueHD", "dts" to "DTS",
            "pcm_s16le" to "PCM", "pcm_s24le" to "PCM", "alac" to "Apple Lossless",
        )
        private val HDR_NAMES = mapOf(
            "hdr10" to "HDR10", "pq" to "HDR10", "hdr10+" to "HDR10+", "hlg" to "HLG",
            "dolby_vision" to "Dolby Vision", "dovi" to "Dolby Vision",
        )
        private val ISO_639_2 = mapOf(
            "eng" to "en", "spa" to "es", "fra" to "fr", "fre" to "fr", "deu" to "de", "ger" to "de", "ita" to "it",
            "jpn" to "ja", "por" to "pt", "nld" to "nl", "dut" to "nl", "swe" to "sv", "nor" to "no", "nob" to "nb",
            "dan" to "da", "fin" to "fi", "pol" to "pl", "rus" to "ru", "kor" to "ko", "zho" to "zh", "chi" to "zh",
            "ara" to "ar", "hin" to "hi", "tur" to "tr", "ces" to "cs", "cze" to "cs", "ell" to "el", "gre" to "el",
            "heb" to "he", "hun" to "hu", "ukr" to "uk", "ron" to "ro", "rum" to "ro", "tha" to "th", "vie" to "vi",
            "ind" to "id",
        )
    }
}

/** The name people know an item by: episode titles for episodes, the date for camera-named photos. */
fun MediaFormat.displayTitle(item: Item): String = when {
    item.category == Category.Episode && !item.episodeTitle.isNullOrBlank() -> item.episodeTitle
    MediaFormat.hasFileNameTitle(item) -> "Photo from ${date(item.takenAt!!)}"
    else -> item.title
}

/** Whether the item plays in the player (videos, wherever they are). */
val Item.isVideo: Boolean get() = kind == MediaKind.Video
