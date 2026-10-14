package com.thekozugroup.popcornstash.feature.player

import android.graphics.Rect
import android.os.Looper
import android.util.Rational
import androidx.media3.common.MimeTypes
import androidx.media3.common.PlaybackException
import androidx.media3.common.util.UnstableApi
import androidx.media3.datasource.DataSpec
import androidx.media3.datasource.HttpDataSource
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.thekozugroup.popcornstash.data.api.AuthInterceptor
import com.thekozugroup.popcornstash.data.api.baseHttpClient
import com.thekozugroup.popcornstash.testing.Fixtures
import com.thekozugroup.popcornstash.testing.next
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import okhttp3.HttpUrl.Companion.toHttpUrl
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Shadows.shadowOf
import java.io.IOException
import java.util.concurrent.TimeUnit

/**
 * The real ExoPlayer pieces on Robolectric: requests go through the session's client (the
 * bearer token for the server only, Range for the original file), ExoPlayer's errors become
 * what the player acts on, and the picture-in-picture window keeps the video's shape.
 */
@androidx.annotation.OptIn(UnstableApi::class)
@RunWith(AndroidJUnit4::class)
class ExoPlayerEngineTest {
    private val server = MockWebServer()

    @Before
    fun setUp() = server.start()

    @After
    fun tearDown() = server.close()

    private fun sessionClient() = server.url("/").let { base ->
        baseHttpClient("PopcornStash-Android/test").newBuilder().addInterceptor(AuthInterceptor(base, { Fixtures.TOKEN }, {})).build()
    }

    @Test
    fun theOriginalFileIsReadWithTheTokenAndARangeAndTheClientWaitsForAConversion() {
        server.enqueue(MockResponse.Builder().code(206).body("partial").addHeader("Content-Range", "bytes 1000-1006/5000").build())
        val client = sessionClient()
        assertEquals(60_000, ExoPlayerEngine.playerClient(client).readTimeoutMillis)
        val engine = ExoPlayerEngine(ApplicationProvider.getApplicationContext(), client)
        try {
            val source = engine.dataSourceFactory.createDataSource()
            val url = server.url("/api/items/i-paper/file")
            source.open(DataSpec.Builder().setUri(url.toString()).setPosition(1000).build())
            source.close()
            val request = server.next()
            assertEquals("/api/items/i-paper/file", request.url.encodedPath)
            assertEquals("Bearer ${Fixtures.TOKEN}", request.headers["Authorization"])
            assertEquals("bytes=1000-", request.headers["Range"])
        } finally {
            engine.release()
        }
    }

    @Test
    fun aRequestToAnotherHostCarriesNoToken() {
        val other = MockWebServer().apply { start() }
        try {
            other.enqueue(MockResponse.Builder().code(200).body("x").build())
            val engine = ExoPlayerEngine(ApplicationProvider.getApplicationContext(), sessionClient())
            try {
                val source = engine.dataSourceFactory.createDataSource()
                // Same machine, another port: another origin.
                source.open(DataSpec(android.net.Uri.parse(other.url("/segment.ts").toString())))
                source.close()
                assertNull(other.takeRequest(5, TimeUnit.SECONDS)!!.headers["Authorization"])
            } finally {
                engine.release()
            }
        } finally {
            other.close()
        }
    }

    @Test
    fun aForgottenSessionReadsAsA404ThePlayerCanActOn() {
        server.enqueue(MockResponse.Builder().code(404).body("""{"error":{"code":"not_found","message":"That no longer exists."}}""").build())
        val engine = ExoPlayerEngine(ApplicationProvider.getApplicationContext(), sessionClient())
        try {
            val source = engine.dataSourceFactory.createDataSource()
            try {
                source.open(DataSpec(android.net.Uri.parse(server.url("/api/stream/s-1/42.ts").toString())))
                fail("a 404 must not open")
            } catch (e: HttpDataSource.InvalidResponseCodeException) {
                val error = PlaybackException("segment", e, PlaybackException.ERROR_CODE_IO_BAD_HTTP_STATUS)
                assertEquals(EngineError.Http(404), ExoPlayerEngine.engineError(error))
            }
        } finally {
            engine.release()
        }
    }

    @Test
    fun exoPlayerErrorsBecomeWhatThePlayerDoes() {
        fun error(code: Int, cause: Throwable? = null) = ExoPlayerEngine.engineError(PlaybackException("x", cause, code))
        assertEquals(EngineError.Network, error(PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_FAILED, IOException()))
        assertEquals(EngineError.Network, error(PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_TIMEOUT))
        assertEquals(EngineError.Network, error(PlaybackException.ERROR_CODE_TIMEOUT))
        assertEquals(EngineError.Decoding, error(PlaybackException.ERROR_CODE_DECODER_INIT_FAILED))
        assertEquals(EngineError.Decoding, error(PlaybackException.ERROR_CODE_DECODING_FORMAT_UNSUPPORTED))
        assertEquals(EngineError.Decoding, error(PlaybackException.ERROR_CODE_PARSING_CONTAINER_UNSUPPORTED))
        assertEquals(EngineError.Other, error(PlaybackException.ERROR_CODE_AUDIO_TRACK_INIT_FAILED))
        val forbidden = HttpDataSource.InvalidResponseCodeException(
            403, "Forbidden", null, emptyMap(), DataSpec(android.net.Uri.parse("https://stash.example.com/x")), ByteArray(0),
        )
        // Wrapped, as ExoPlayer wraps a loader's error.
        assertEquals(EngineError.Http(403), error(PlaybackException.ERROR_CODE_IO_BAD_HTTP_STATUS, IOException(forbidden)))
    }

    @Test
    fun aConversionIsLoadedAsHlsAndTheOriginalAsAFileAtTheStartAsked() {
        val engine = ExoPlayerEngine(ApplicationProvider.getApplicationContext(), sessionClient())
        try {
            val base = "https://stash.example.com/".toHttpUrl()
            engine.load(StreamSource(base.resolve("/api/stream/s-1/index.m3u8")!!, hls = true), startMs = 724_000, playWhenReady = false)
            val hls = engine.player.currentMediaItem!!.localConfiguration!!
            assertEquals(MimeTypes.APPLICATION_M3U8, hls.mimeType)
            assertEquals("https://stash.example.com/api/stream/s-1/index.m3u8", hls.uri.toString())
            assertEquals(724_000L, engine.positionMs)
            assertFalse(engine.playWhenReady)

            engine.load(StreamSource(base.resolve("/api/items/i-paper/file")!!, hls = false), startMs = 0, playWhenReady = true)
            assertNull(engine.player.currentMediaItem!!.localConfiguration!!.mimeType)
            assertTrue(engine.playWhenReady)
            engine.pause()
            assertFalse(engine.playWhenReady)
            shadowOf(Looper.getMainLooper()).idle()
        } finally {
            engine.release()
        }
    }

    @Test
    fun thePictureInPictureWindowKeepsTheVideosShapeWithinWhatAndroidAllows() {
        assertEquals(Rational(1920, 1080), PictureInPicture.aspect(1920, 1080))
        assertEquals(Rational(1080, 1920), PictureInPicture.aspect(1080, 1920))
        assertEquals("a very wide film is held at 2.38:1", Rational(2380, 1000), PictureInPicture.aspect(3840, 1200))
        assertEquals(Rational(420, 1000), PictureInPicture.aspect(400, 1200))
        assertNull(PictureInPicture.aspect(0, 1080))

        // A 16:9 picture fitted in a 20:9 landscape screen sits between two black bars.
        assertEquals(Rect(240, 0, 2160, 1080), PictureInPicture.fittedRect(Rect(0, 0, 2400, 1080), 1920, 1080))
        // An upright phone video in the same screen is a narrow column in the middle.
        assertEquals(Rect(896, 0, 1504, 1080), PictureInPicture.fittedRect(Rect(0, 0, 2400, 1080), 1080, 1920))
        assertEquals(Rect(0, 0, 2400, 1080), PictureInPicture.fittedRect(Rect(0, 0, 2400, 1080), 0, 0))
    }
}
