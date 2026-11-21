package com.thekozugroup.popcornstash.feature.player

import android.content.Context
import android.content.ContextWrapper
import android.graphics.Bitmap
import androidx.test.core.app.ApplicationProvider
import coil3.asImage
import coil3.network.HttpException
import coil3.network.NetworkResponse
import coil3.request.ErrorResult
import coil3.request.SuccessResult
import com.thekozugroup.popcornstash.data.api.AuthInterceptor
import com.thekozugroup.popcornstash.data.api.baseHttpClient
import com.thekozugroup.popcornstash.testing.Fixtures
import com.thekozugroup.popcornstash.ui.media.Pictures
import com.thekozugroup.popcornstash.ui.media.sessionImageLoader
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.runBlocking
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import okio.Buffer
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import java.io.ByteArrayOutputStream
import java.io.File
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger

@RunWith(RobolectricTestRunner::class)
class SeekPreviewImageTest {
    @get:Rule val folder = TemporaryFolder()

    @Test fun decoderBusyRetriesOnceOnlyForTheCurrentOpenPreview() = runTest {
        val request = previewRequest(ApplicationProvider.getApplicationContext(), Pictures.Placeholder, "clip", 0)
        val image = Bitmap.createBitmap(16, 9, Bitmap.Config.ARGB_8888).asImage()
        val calls = mutableListOf<Long>()
        val frames = SeekPreviewFrames { sample ->
            calls += sample
            val status = when (sample) {
                2L -> if (calls.count { it == sample } == 1) 409 else 200
                6L -> 403
                10L -> 200
                else -> 409
            }
            if (status == 200) SuccessResult(image, request)
            else ErrorResult(null, request, HttpException(NetworkResponse(code = status)))
        }
        frames.load(2)
        assertNotNull(frames.imageAt(2))
        frames.load(4)
        assertNull(frames.imageAt(4))
        frames.load(6)
        assertNull(frames.imageAt(2))

        var selected = 8L
        val moved = launch(start = CoroutineStart.UNDISPATCHED) { frames.load(8) { selected == 8L } }
        selected = 10L
        moved.join()
        frames.load(selected)
        val released = launch(start = CoroutineStart.UNDISPATCHED) { frames.load(12) }
        released.cancelAndJoin()
        val locked = launch(start = CoroutineStart.UNDISPATCHED) { frames.load(14) }
        frames.close()
        locked.join()

        assertEquals(listOf(2L, 2L, 4L, 4L, 6L, 8L, 10L, 12L, 14L), calls)
        assertNull(frames.imageAt(10))
    }

    @Test fun previewUsesTheSessionBearerAndNeverReusesADiskOrMemoryFrameAfterA401() = runBlocking(Dispatchers.Default) {
        val server = MockWebServer()
        server.start()
        val unauthorized = AtomicInteger()
        val origin = server.url("/")
        val http = baseHttpClient("preview-test").newBuilder()
            .addInterceptor(AuthInterceptor(origin, { Fixtures.TOKEN }, { unauthorized.incrementAndGet() })).build()
        val context = object : ContextWrapper(ApplicationProvider.getApplicationContext<Context>()) {
            override fun getCacheDir(): File = folder.root
        }
        val images = sessionImageLoader(context, http)
        try {
            val bitmap = Bitmap.createBitmap(16, 9, Bitmap.Config.ARGB_8888)
            val jpeg = ByteArrayOutputStream().use { bytes ->
                bitmap.compress(Bitmap.CompressFormat.JPEG, 80, bytes)
                bitmap.recycle()
                bytes.toByteArray()
            }
            server.enqueue(MockResponse.Builder().addHeader("Content-Type", "image/jpeg")
                .addHeader("Cache-Control", "private, no-store").body(Buffer().write(jpeg)).build())
            val frames = SeekPreviewFrames { sample ->
                images.execute(previewRequest(context, Pictures(origin), "clip", sample))
            }
            frames.load(4)
            assertNotNull(frames.imageAt(4))
            val fetched = requireNotNull(server.takeRequest(5, TimeUnit.SECONDS))
            assertEquals("/api/items/clip/preview?time_s=4", fetched.url.encodedPath + "?" + fetched.url.encodedQuery)
            assertEquals("Bearer ${Fixtures.TOKEN}", fetched.headers["Authorization"])
            server.enqueue(MockResponse.Builder().code(401).build())
            frames.load(6)
            assertNull(frames.imageAt(6))
            assertNull(frames.imageAt(4))
            assertEquals(1, unauthorized.get())
            assertEquals(0L, images.diskCache?.size ?: 0L)
            assertEquals(0L, images.memoryCache?.size ?: 0L)
        } finally {
            images.shutdown()
            server.close()
        }
    }
}
