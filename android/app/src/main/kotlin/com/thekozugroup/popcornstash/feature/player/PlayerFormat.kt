package com.thekozugroup.popcornstash.feature.player

import com.thekozugroup.popcornstash.data.api.AudioStream
import com.thekozugroup.popcornstash.data.api.Category
import com.thekozugroup.popcornstash.data.api.ItemDetail
import java.util.Locale
import kotlin.math.floor

/**
 * The player's words and numbers, the same as the web player's
 * (web/src/components/library/libraryFormat.ts and web/src/lib/format.ts).
 */
object PlayerFormat {
    /** "0:05", "12:04", "1:02:03". */
    fun clock(seconds: Double): String {
        if (!seconds.isFinite() || seconds < 0) return "0:00"
        val total = floor(seconds).toLong()
        val hours = total / 3600
        val minutes = (total % 3600) / 60
        val secs = total % 60
        return if (hours > 0) {
            "%d:%02d:%02d".format(Locale.ROOT, hours, minutes, secs)
        } else {
            "%d:%02d".format(Locale.ROOT, minutes, secs)
        }
    }

    /** "12 minutes 4 seconds" for TalkBack, where "12:04" would be read as a time of day. */
    fun spokenDuration(seconds: Double): String {
        val total = if (seconds.isFinite() && seconds > 0) floor(seconds).toLong() else 0L
        val hours = total / 3600
        val minutes = (total % 3600) / 60
        val secs = total % 60
        val parts = buildList {
            if (hours > 0) add(plural(hours, "hour"))
            if (minutes > 0) add(plural(minutes, "minute"))
            if (secs > 0 || (hours == 0L && minutes == 0L)) add(plural(secs, "second"))
        }
        return parts.joinToString(" ")
    }

    private fun plural(count: Long, unit: String) = if (count == 1L) "1 $unit" else "$count ${unit}s"

    /**
     * Where to resume [item], in seconds; 0 starts from the beginning. The web's rule: a
     * finished item, one stopped in its first 15 seconds and one within 2 seconds of its end
     * start again.
     */
    fun resumePosition(item: ItemDetail): Double {
        val progress = item.progress ?: return 0.0
        if (progress.finished) return 0.0
        val duration = progress.durationS.takeIf { it > 0 } ?: item.durationS ?: 0.0
        if (progress.positionS < 15 || (duration > 0 && progress.positionS > duration - 2)) return 0.0
        return progress.positionS
    }

    /** "S01E02", or null when the item is not numbered as an episode. */
    fun episodeCode(item: ItemDetail): String? {
        val episode = item.episode ?: return null
        return "S%02dE%02d".format(Locale.ROOT, item.season ?: 1, episode)
    }

    /** The two lines beside the close button: the show (or the title), then the episode. */
    fun titleLines(item: ItemDetail): Pair<String, String?> {
        if (item.category == Category.Episode && item.showName != null) {
            val code = episodeCode(item)
            val name = item.episodeTitle?.takeIf { it.isNotBlank() }
            val second = listOfNotNull(code, name).joinToString(" · ").ifEmpty { null }
            return item.showName to second
        }
        return item.title to null
    }

    /** "S01E03 · The lantern keepers" for the end of an episode, or the next item's title. */
    fun nextLabel(next: ItemDetail): String {
        val code = episodeCode(next)
        val name = next.episodeTitle?.takeIf { it.isNotBlank() }
        return when {
            code != null && name != null -> "$code · $name"
            code != null -> code
            else -> next.title
        }
    }

    // ------------------------------------------------------------ audio ----

    private val iso6392 = mapOf(
        "eng" to "en", "spa" to "es", "fra" to "fr", "fre" to "fr", "deu" to "de", "ger" to "de", "ita" to "it",
        "jpn" to "ja", "por" to "pt", "nld" to "nl", "dut" to "nl", "swe" to "sv", "nor" to "no", "nob" to "nb",
        "dan" to "da", "fin" to "fi", "pol" to "pl", "rus" to "ru", "kor" to "ko", "zho" to "zh", "chi" to "zh",
        "ara" to "ar", "hin" to "hi", "tur" to "tr", "ces" to "cs", "cze" to "cs", "ell" to "el", "gre" to "el",
        "heb" to "he", "hun" to "hu", "ukr" to "uk", "ron" to "ro", "rum" to "ro", "tha" to "th", "vie" to "vi",
        "ind" to "id",
    )

    private val audioCodecs = mapOf(
        "aac" to "AAC", "mp3" to "MP3", "opus" to "Opus", "flac" to "FLAC", "vorbis" to "Vorbis",
        "ac3" to "Dolby Digital", "eac3" to "Dolby Digital Plus", "truehd" to "Dolby TrueHD", "dts" to "DTS",
        "pcm_s16le" to "PCM", "pcm_s24le" to "PCM", "alac" to "Apple Lossless",
    )

    /** "eng" -> "English" in the phone's language; null for unknown or undetermined. */
    fun languageName(code: String?, locale: Locale = Locale.getDefault()): String? {
        if (code.isNullOrBlank() || code in setOf("und", "mul", "zxx")) return null
        val tag = iso6392[code.lowercase(Locale.ROOT)] ?: code
        val name = Locale.forLanguageTag(tag).getDisplayLanguage(locale)
        return if (name.isNullOrBlank() || name.equals(tag, ignoreCase = true)) code.uppercase(Locale.ROOT) else name
    }

    /** "English", or the track's own title, with the language added when the title does not say it. */
    fun audioLabel(track: AudioStream, position: Int, locale: Locale = Locale.getDefault()): String {
        val language = languageName(track.language, locale)
        val title = track.title?.takeIf { it.isNotBlank() }
        val base = title ?: language ?: "Track ${position + 1}"
        return if (title != null && language != null && !title.contains(language)) "$base ($language)" else base
    }

    /** "Dolby Digital Plus 5.1", "AAC stereo". */
    fun audioDetail(track: AudioStream): String =
        listOfNotNull(audioCodecs[track.codec.lowercase(Locale.ROOT)] ?: track.codec.uppercase(Locale.ROOT), channels(track.channels))
            .joinToString(" ")

    private fun channels(count: Int?): String? = when (count) {
        null, 0 -> null
        1 -> "mono"
        2 -> "stereo"
        6 -> "5.1"
        8 -> "7.1"
        else -> "$count channels"
    }

    // ---------------------------------------------------------- quality ----

    /** "4K", "1080p", "720p" from the frame size (wide films are judged by width), as on the web. */
    fun resolutionName(width: Int?, height: Int?): String? {
        if (width == null || height == null || width <= 0 || height <= 0) return null
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
}
