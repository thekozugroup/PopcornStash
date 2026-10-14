package com.thekozugroup.popcornstash.data.device

import android.annotation.SuppressLint
import android.content.Context
import android.hardware.display.DisplayManager
import android.media.MediaCodecInfo
import android.media.MediaCodecList
import android.view.Display
import com.thekozugroup.popcornstash.data.api.PlaybackRequest
import kotlin.math.min

/** One decoder as far as the playback decision cares. */
data class DecoderInfo(
    val mime: String,
    /** The tallest picture it decodes, when it says. */
    val maxHeight: Int?,
    /** It decodes HEVC Main 10 with HDR10 or HDR10+. */
    val hdr10: Boolean = false,
)

/**
 * This phone's real playback abilities for `POST /api/items/:id/playback`
 * (docs/architecture.md section 10, Item and player), from `MediaCodecList` and the display.
 */
object PlaybackCapabilities {
    private val videoCodecs = linkedMapOf(
        "video/avc" to "h264",
        "video/hevc" to "hevc",
        "video/av01" to "av1",
        "video/x-vnd.on2.vp9" to "vp9",
    )
    private val audioCodecs = linkedMapOf(
        "audio/mp4a-latm" to "aac",
        "audio/mpeg" to "mp3",
        "audio/opus" to "opus",
        "audio/flac" to "flac",
    )
    private val heightLadder = listOf(480, 720, 1080, 1440, 2160, 4320)

    fun request(context: Context, startS: Double? = null, audioIndex: Int? = null): PlaybackRequest {
        val display = context.getSystemService(DisplayManager::class.java)?.getDisplay(Display.DEFAULT_DISPLAY)
        val metrics = display?.mode
        val shortSide = metrics?.let { min(it.physicalWidth, it.physicalHeight) }
        return decide(decoders(), shortSide, display?.isHdr == true, startS, audioIndex)
    }

    /** The decision itself, separate from Android so it can be tested on its own. */
    fun decide(
        decoders: List<DecoderInfo>,
        displayShortSide: Int?,
        displayHdr: Boolean,
        startS: Double? = null,
        audioIndex: Int? = null,
    ): PlaybackRequest {
        val mimes = decoders.map { it.mime }.toSet()
        val video = videoCodecs.filterKeys { it in mimes }.values.toList()
        val audio = audioCodecs.filterKeys { it in mimes }.values.toList()
        val decoderMax = decoders
            .filter { it.mime == "video/avc" || it.mime == "video/hevc" }
            .mapNotNull { it.maxHeight }
            .maxOrNull()
        val screenMax = displayShortSide?.let { side -> heightLadder.firstOrNull { it >= side } ?: heightLadder.last() }
        val maxHeight = listOfNotNull(decoderMax, screenMax).minOrNull()
        val hdr = displayHdr && decoders.any { it.mime == "video/hevc" && it.hdr10 }
        return PlaybackRequest(video, audio, hdr, maxHeight, startS, audioIndex)
    }

    // HEVCProfileMain10HDR10Plus is a constant compiled into the app; on Android 9 no decoder
    // reports it, so comparing with it is harmless there.
    @SuppressLint("InlinedApi")
    private fun decoders(): List<DecoderInfo> =
        MediaCodecList(MediaCodecList.REGULAR_CODECS).codecInfos
            .filterNot { it.isEncoder }
            .flatMap { info ->
                info.supportedTypes.map { type ->
                    val caps = runCatching { info.getCapabilitiesForType(type) }.getOrNull()
                    DecoderInfo(
                        mime = type.lowercase(),
                        maxHeight = caps?.videoCapabilities?.supportedHeights?.upper,
                        hdr10 = caps?.profileLevels.orEmpty().any {
                            it.profile == MediaCodecInfo.CodecProfileLevel.HEVCProfileMain10HDR10 ||
                                it.profile == MediaCodecInfo.CodecProfileLevel.HEVCProfileMain10HDR10Plus
                        },
                    )
                }
            }
}
