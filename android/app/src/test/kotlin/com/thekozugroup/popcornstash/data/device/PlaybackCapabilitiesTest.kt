package com.thekozugroup.popcornstash.data.device

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PlaybackCapabilitiesTest {
    private val phone = listOf(
        DecoderInfo("video/avc", maxHeight = 2160),
        DecoderInfo("video/hevc", maxHeight = 2160, hdr10 = true),
        DecoderInfo("video/x-vnd.on2.vp9", maxHeight = 2160),
        DecoderInfo("audio/mp4a-latm", null),
        DecoderInfo("audio/opus", null),
        DecoderInfo("audio/flac", null),
        DecoderInfo("audio/mpeg", null),
        DecoderInfo("audio/raw", null),
    )

    @Test
    fun namesTheCodecsThisPhoneDecodesInTheServersWords() {
        val request = PlaybackCapabilities.decide(phone, displayShortSide = 1080, displayHdr = false, startS = 12.0, audioIndex = 1)
        assertEquals(listOf("h264", "hevc", "vp9"), request.videoCodecs)
        assertEquals(listOf("aac", "mp3", "opus", "flac"), request.audioCodecs)
        assertEquals(12.0, request.startS!!, 0.0)
        assertEquals(1, request.audioIndex)
    }

    @Test
    fun maxHeightIsTheSmallerOfTheDecoderAndTheScreen() {
        assertEquals(1080, PlaybackCapabilities.decide(phone, displayShortSide = 1080, displayHdr = false).maxHeight)
        assertEquals(1440, PlaybackCapabilities.decide(phone, displayShortSide = 1200, displayHdr = false).maxHeight)
        assertEquals(720, PlaybackCapabilities.decide(listOf(DecoderInfo("video/avc", 720)), displayShortSide = 1440, displayHdr = false).maxHeight)
        assertEquals(2160, PlaybackCapabilities.decide(phone, displayShortSide = null, displayHdr = false).maxHeight)
    }

    @Test
    fun hdrNeedsAnHdrScreenAndAnHdr10Decoder() {
        assertTrue(PlaybackCapabilities.decide(phone, 1080, displayHdr = true).hdr)
        assertFalse(PlaybackCapabilities.decide(phone, 1080, displayHdr = false).hdr)
        assertFalse(PlaybackCapabilities.decide(phone.map { it.copy(hdr10 = false) }, 1080, displayHdr = true).hdr)
    }
}
