package com.thekozugroup.popcornstash.feature.player

import android.content.Context
import android.content.ContextWrapper
import android.graphics.Bitmap
import androidx.test.core.app.ApplicationProvider
import coil3.request.CachePolicy
import coil3.request.ErrorResult
import coil3.request.ImageRequest
import coil3.request.SuccessResult
import com.thekozugroup.popcornstash.data.api.AuthInterceptor
import com.thekozugroup.popcornstash.data.api.baseHttpClient
import com.thekozugroup.popcornstash.testing.Fixtures
import com.thekozugroup.popcornstash.ui.media.Pictures
import com.thekozugroup.popcornstash.ui.media.sessionImageLoader
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import okio.Buffer
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
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
            val request = ImageRequest.Builder(context).data(Pictures(origin).preview("clip", 4))
                .diskCachePolicy(CachePolicy.DISABLED).memoryCachePolicy(CachePolicy.DISABLED).build()
            assertTrue(images.execute(request) is SuccessResult)
            val fetched = requireNotNull(server.takeRequest(5, TimeUnit.SECONDS))
            assertEquals("/api/items/clip/preview?time_s=4", fetched.url.encodedPath + "?" + fetched.url.encodedQuery)
            assertEquals("Bearer ${Fixtures.TOKEN}", fetched.headers["Authorization"])
            server.enqueue(MockResponse.Builder().code(401).build())
            assertTrue(images.execute(request) is ErrorResult)
            assertEquals(1, unauthorized.get())
            assertEquals(0L, images.diskCache?.size ?: 0L)
            assertEquals(0L, images.memoryCache?.size ?: 0L)
        } finally {
            images.shutdown()
            server.close()
        }
    }
}
