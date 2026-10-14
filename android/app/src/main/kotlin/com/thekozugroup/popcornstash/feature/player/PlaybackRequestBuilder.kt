package com.thekozugroup.popcornstash.feature.player

import com.thekozugroup.popcornstash.data.api.MediaSummary
import com.thekozugroup.popcornstash.data.api.PlaybackRequest
import kotlin.math.floor

/** What the person picked in the player's menu. Null means the default: the original picture, the file's first audio track. */
data class StreamChoice(val qualityHeight: Int? = null, val audioIndex: Int? = null)

/** One line of the quality menu: null [height] is "Original". */
data class QualityChoice(val height: Int?, val label: String, val detail: String)

/** One line of the audio menu: the file's stream [index] (sent as `audio_index`). */
data class AudioChoice(val index: Int, val label: String, val detail: String)

/**
 * The body of `POST /api/items/:id/playback` (docs/architecture.md section 10, Item and
 * player): this phone's real decoders from `MediaCodecList` ([device], read by
 * PlaybackCapabilities), the person's choices, and where to start.
 */
object PlaybackRequestBuilder {
    /** The web player's steps, tallest first. */
    val QualitySteps = listOf(2160, 1440, 1080, 720, 480)

    /**
     * The most widely played pair, asked for once when this phone fails to decode a file it
     * claimed to play directly (the web player does the same).
     */
    val SafeDevice = PlaybackRequest(videoCodecs = listOf("h264"), audioCodecs = listOf("aac"), hdr = false)

    fun build(device: PlaybackRequest, choice: StreamChoice, startS: Double, media: MediaSummary?): PlaybackRequest {
        val maxHeight = listOfNotNull(device.maxHeight, choice.qualityHeight).minOrNull()
        // The web sends whole seconds; the server starts the stream at the segment holding it.
        val start = startS.takeIf { it.isFinite() && it >= 1.0 }?.let { floor(it) }
        return PlaybackRequest(
            videoCodecs = device.videoCodecs,
            audioCodecs = device.audioCodecs,
            hdr = device.hdr,
            maxHeight = maxHeight,
            startS = start,
            audioIndex = audioIndex(choice.audioIndex, media),
        )
    }

    /**
     * The `audio_index` to send: only a track the file has, and never the first one (the
     * server plays that by default, and naming it would force a conversion).
     */
    fun audioIndex(chosen: Int?, media: MediaSummary?): Int? {
        val tracks = media?.audio.orEmpty()
        if (chosen == null || tracks.isEmpty() || tracks.first().index == chosen) return null
        return chosen.takeIf { index -> tracks.any { it.index == index } }
    }

    /**
     * The quality menu, or nothing when there is nothing smaller to offer: "Original", then
     * each step below the picture's short side (40 pixels of slack, as on the web) and below
     * what this phone shows anyway ([deviceMaxHeight]).
     */
    fun qualities(media: MediaSummary?, itemWidth: Int?, itemHeight: Int?, deviceMaxHeight: Int?): List<QualityChoice> {
        val width = media?.video?.width?.takeIf { it > 0 } ?: itemWidth ?: 0
        val height = media?.video?.height?.takeIf { it > 0 } ?: itemHeight ?: 0
        val short = minOf(width, height).takeIf { it > 0 } ?: return emptyList()
        val ceiling = minOf(short - 40, deviceMaxHeight ?: Int.MAX_VALUE)
        val steps = QualitySteps.filter { it < ceiling }
        if (steps.isEmpty()) return emptyList()
        val source = PlayerFormat.resolutionName(width, height)
        val original = QualityChoice(
            height = null,
            label = "Original",
            detail = if (source != null) "$source, converted only if needed" else "Converted only if needed",
        )
        return listOf(original) + steps.map { QualityChoice(it, "${it}p", "Smaller stream for slow connections") }
    }

    /** The audio menu, or nothing when the file has one track. */
    fun audioChoices(media: MediaSummary?): List<AudioChoice> {
        val tracks = media?.audio.orEmpty()
        if (tracks.size < 2) return emptyList()
        return tracks.mapIndexed { position, track ->
            AudioChoice(track.index, PlayerFormat.audioLabel(track, position), PlayerFormat.audioDetail(track))
        }
    }
}
