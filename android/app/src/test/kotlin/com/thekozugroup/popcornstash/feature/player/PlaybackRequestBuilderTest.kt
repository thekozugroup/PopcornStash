package com.thekozugroup.popcornstash.feature.player

import com.thekozugroup.popcornstash.data.api.AudioStream
import com.thekozugroup.popcornstash.data.api.MediaSummary
import com.thekozugroup.popcornstash.data.api.PlaybackRequest
import com.thekozugroup.popcornstash.data.api.StashJson
import com.thekozugroup.popcornstash.data.api.VideoStream
import com.thekozugroup.popcornstash.data.device.DecoderInfo
import com.thekozugroup.popcornstash.data.device.PlaybackCapabilities
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.Locale

class PlaybackRequestBuilderTest {
    /** A mid-range phone's decoders as MediaCodecList lists them, on a 1080p screen. */
    private val phone = PlaybackCapabilities.decide(
        decoders = listOf(
            DecoderInfo("video/avc", maxHeight = 2160),
            DecoderInfo("video/hevc", maxHeight = 2160, hdr10 = true),
            DecoderInfo("video/x-vnd.on2.vp9", maxHeight = 2160),
            DecoderInfo("audio/mp4a-latm", null),
            DecoderInfo("audio/opus", null),
            DecoderInfo("audio/flac", null),
            DecoderInfo("audio/mpeg", null),
        ),
        displayShortSide = 1080,
        displayHdr = false,
    )

    private val film = MediaSummary(
        container = "mov,mp4,m4a,3gp,3g2,mj2",
        durationS = 3136.0,
        video = VideoStream(index = 0, codec = "hevc", width = 3840, height = 1608),
        audio = listOf(
            AudioStream(index = 1, codec = "eac3", channels = 6, language = "eng"),
            AudioStream(index = 2, codec = "aac", channels = 2, language = "eng", title = "Commentary"),
            AudioStream(index = 3, codec = "aac", channels = 2, language = "fre"),
        ),
    )

    @Test
    fun sendsThisPhonesDecodersAndTheResumePointInWholeSeconds() {
        val request = PlaybackRequestBuilder.build(phone, StreamChoice(), startS = 724.6, media = film)
        assertEquals(listOf("h264", "hevc", "vp9"), request.videoCodecs)
        assertEquals(listOf("aac", "mp3", "opus", "flac"), request.audioCodecs)
        assertEquals(false, request.hdr)
        assertEquals(1080, request.maxHeight)
        assertEquals(724.0, request.startS!!, 0.0)
        assertNull("the first track plays by default, so it is never named", request.audioIndex)
    }

    @Test
    fun theWireShapeIsTheContractsSnakeCaseWithNothingForDefaults() {
        val fromStart = PlaybackRequestBuilder.build(phone, StreamChoice(), startS = 0.0, media = film)
        assertEquals(
            """{"video_codecs":["h264","hevc","vp9"],"audio_codecs":["aac","mp3","opus","flac"],"hdr":false,"max_height":1080}""",
            StashJson.encodeToString(PlaybackRequest.serializer(), fromStart),
        )
        val chosen = PlaybackRequestBuilder.build(phone, StreamChoice(qualityHeight = 720, audioIndex = 2), startS = 61.0, media = film)
        assertEquals(
            """{"video_codecs":["h264","hevc","vp9"],"audio_codecs":["aac","mp3","opus","flac"],"hdr":false,"max_height":720,"start_s":61.0,"audio_index":2}""",
            StashJson.encodeToString(PlaybackRequest.serializer(), chosen),
        )
    }

    @Test
    fun aChosenQualityLowersTheHeightButNeverRaisesItAboveWhatThePhoneShows() {
        assertEquals(720, PlaybackRequestBuilder.build(phone, StreamChoice(qualityHeight = 720), 0.0, film).maxHeight)
        assertEquals(1080, PlaybackRequestBuilder.build(phone, StreamChoice(qualityHeight = 1440), 0.0, film).maxHeight)
        val noScreen = phone.copy(maxHeight = null)
        assertEquals(480, PlaybackRequestBuilder.build(noScreen, StreamChoice(qualityHeight = 480), 0.0, film).maxHeight)
        assertNull(PlaybackRequestBuilder.build(noScreen, StreamChoice(), 0.0, film).maxHeight)
    }

    @Test
    fun aStartBeforeTheFirstSecondOrNotANumberStartsAtTheBeginning() {
        assertNull(PlaybackRequestBuilder.build(phone, StreamChoice(), 0.4, film).startS)
        assertNull(PlaybackRequestBuilder.build(phone, StreamChoice(), Double.NaN, film).startS)
        assertNull(PlaybackRequestBuilder.build(phone, StreamChoice(), -3.0, film).startS)
    }

    @Test
    fun onlyATrackTheFileHasAndNotTheFirstIsSentAsAudioIndex() {
        assertEquals(3, PlaybackRequestBuilder.audioIndex(3, film))
        assertNull(PlaybackRequestBuilder.audioIndex(1, film))
        assertNull(PlaybackRequestBuilder.audioIndex(9, film))
        assertNull(PlaybackRequestBuilder.audioIndex(2, null))
        assertNull(PlaybackRequestBuilder.audioIndex(null, film))
    }

    @Test
    fun theSafeRetryAsksForTheMostWidelyPlayedPair() {
        val safe = PlaybackRequestBuilder.build(PlaybackRequestBuilder.SafeDevice, StreamChoice(), 30.0, film)
        assertEquals(listOf("h264"), safe.videoCodecs)
        assertEquals(listOf("aac"), safe.audioCodecs)
        assertEquals(false, safe.hdr)
    }

    @Test
    fun qualitiesOfferWhatIsSmallerThanThePictureAndTheScreen() {
        val wide = PlaybackRequestBuilder.qualities(film, null, null, deviceMaxHeight = 2160)
        assertEquals(listOf(null, 1440, 1080, 720, 480), wide.map { it.height })
        assertEquals("Original", wide.first().label)
        assertEquals("4K, converted only if needed", wide.first().detail)
        assertEquals("720p", wide[3].label)
        assertEquals("Smaller stream for slow connections", wide[3].detail)

        // A 1080p screen gets nothing from 1440p or 1080p over the original.
        assertEquals(listOf(null, 720, 480), PlaybackRequestBuilder.qualities(film, null, null, deviceMaxHeight = 1080).map { it.height })

        // A phone video held upright is judged by its short side, as the server does.
        val upright = MediaSummary(video = VideoStream(0, "h264", width = 1080, height = 1920))
        assertEquals(listOf(null, 720, 480), PlaybackRequestBuilder.qualities(upright, null, null, null).map { it.height })

        val small = MediaSummary(video = VideoStream(0, "h264", width = 640, height = 360))
        assertTrue("nothing smaller to offer", PlaybackRequestBuilder.qualities(small, null, null, 1080).isEmpty())
        assertEquals(listOf(null, 480), PlaybackRequestBuilder.qualities(null, 1280, 720, 1080).map { it.height })
        assertTrue(PlaybackRequestBuilder.qualities(null, null, null, 1080).isEmpty())
    }

    @Test
    fun audioChoicesNameEachTrackLikeTheWeb() {
        val previous = Locale.getDefault()
        Locale.setDefault(Locale.ENGLISH)
        try {
            val choices = PlaybackRequestBuilder.audioChoices(film)
            assertEquals(listOf(1, 2, 3), choices.map { it.index })
            assertEquals(listOf("English", "Commentary (English)", "French"), choices.map { it.label })
            assertEquals(listOf("Dolby Digital Plus 5.1", "AAC stereo", "AAC stereo"), choices.map { it.detail })
            assertTrue("one track is no choice", PlaybackRequestBuilder.audioChoices(film.copy(audio = film.audio.take(1))).isEmpty())
            val unnamed = PlaybackRequestBuilder.audioChoices(
                MediaSummary(audio = listOf(AudioStream(4, "ac3", 1, "und"), AudioStream(5, "dts", 8, null))),
            )
            assertEquals(listOf("Track 1", "Track 2"), unnamed.map { it.label })
            assertEquals(listOf("Dolby Digital mono", "DTS 7.1"), unnamed.map { it.detail })
        } finally {
            Locale.setDefault(previous)
        }
    }
}
