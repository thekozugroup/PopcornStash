package com.thekozugroup.popcornstash.feature.photos

import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.AuthInterceptor
import com.thekozugroup.popcornstash.data.api.baseHttpClient
import com.thekozugroup.popcornstash.feature.library.FileFetcher
import com.thekozugroup.popcornstash.testing.Fixtures
import com.thekozugroup.popcornstash.testing.MainDispatcherRule
import com.thekozugroup.popcornstash.testing.keep
import com.thekozugroup.popcornstash.testing.awaitUntil
import com.thekozugroup.popcornstash.testing.next
import com.thekozugroup.popcornstash.testing.respond
import com.thekozugroup.popcornstash.testing.withinRealSeconds
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.test.runTest
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import okio.Buffer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import java.io.ByteArrayOutputStream

/** Saving a photo's original where the person chose, with the app's own token. */
class SaveOriginalViewModelTest {
    @get:Rule
    val main = MainDispatcherRule()

    private val server = MockWebServer()
    private lateinit var api: ApiClient
    private lateinit var files: FileFetcher

    @Before
    fun setUp() {
        server.start()
        val client = baseHttpClient("t").newBuilder()
            .addInterceptor(AuthInterceptor(server.url("/"), { Fixtures.TOKEN }, {}))
            .build()
        api = ApiClient(server.url("/"), client)
        files = FileFetcher(client)
    }

    @After
    fun tearDown() = server.close()

    @Test
    fun theOriginalIsCopiedIntoTheChosenDocument() = runTest {
        val out = ByteArrayOutputStream()
        val model = main.keep { SaveOriginalViewModel(api, files) { uri -> if (uri == "content://saved/1") out else null } }
        server.enqueue(MockResponse.Builder().code(200).body(Buffer().write(ByteArray(50_000) { (it % 251).toByte() })).build())
        val message = backgroundScope.async(Dispatchers.Unconfined) { withinRealSeconds { model.messages.first() } }
        model.save("p1", "content://saved/1")
        assertEquals("p1", model.saving.value)
        assertEquals("Original saved.", message.await())
        val request = server.next()
        assertEquals("/api/items/p1/file?download=1", request.target)
        assertEquals("Bearer ${Fixtures.TOKEN}", request.headers["Authorization"])
        assertEquals(50_000, out.size())
        model.saving.awaitUntil { it == null }
    }

    @Test
    fun aPlaceThatCannotBeWrittenSaysSo() = runTest {
        val model = main.keep { SaveOriginalViewModel(api, files) { null } }
        val message = backgroundScope.async(Dispatchers.Unconfined) { withinRealSeconds { model.messages.first() } }
        model.save("p1", "content://nowhere")
        assertEquals("The original could not be saved there. Choose another place.", message.await())
    }

    @Test
    fun aPhotoThatIsGoneSaysSo() = runTest {
        val model = main.keep { SaveOriginalViewModel(api, files) { ByteArrayOutputStream() } }
        server.respond("""{"error":{"code":"not_found","message":"That item no longer exists."}}""", 404)
        val message = backgroundScope.async(Dispatchers.Unconfined) { withinRealSeconds { model.messages.first() } }
        model.save("p1", "content://saved/2")
        assertEquals("That photo is not available any more.", message.await())
    }
}
