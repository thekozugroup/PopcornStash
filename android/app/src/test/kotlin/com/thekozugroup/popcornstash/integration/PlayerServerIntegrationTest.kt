package com.thekozugroup.popcornstash.integration

import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.Category
import com.thekozugroup.popcornstash.data.api.Item
import com.thekozugroup.popcornstash.data.api.ItemQuery
import com.thekozugroup.popcornstash.data.api.PlaybackRequest
import com.thekozugroup.popcornstash.data.auth.PasskeySignIn
import com.thekozugroup.popcornstash.feature.player.ApiPlayerApi
import com.thekozugroup.popcornstash.feature.player.ExoPlayerEngine
import com.thekozugroup.popcornstash.feature.player.FakeEngine
import com.thekozugroup.popcornstash.feature.player.PlayerController
import com.thekozugroup.popcornstash.feature.player.PlayerRequest
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout
import okhttp3.OkHttpClient
import okhttp3.Request
import org.junit.AfterClass
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.BeforeClass
import org.junit.Test
import java.util.concurrent.Executors

/**
 * The player's rules against a real `popcornstash serve` on the dev fixture (the same server
 * [ServerIntegrationTest] starts): this phone's decoders get the H.264 MP4 directly and the
 * MKV through a conversion; the original answers a Range request and the playlist and its
 * first segment answer the app's token; the place is saved where the person paused; leaving
 * ends the conversion on the server.
 *
 * Runs only when the server binary is named (android/README.md, Tests).
 */
class PlayerServerIntegrationTest {
    companion object {
        @BeforeClass
        @JvmStatic
        fun startServer() = ServerIntegrationTest.startServer()

        @AfterClass
        @JvmStatic
        fun stopServer() = ServerIntegrationTest.stopServer()
    }

    /** A phone with H.264 and HEVC decoders on a 1080p screen, as PlaybackCapabilities reads it. */
    private val phone = PlaybackRequest(listOf("h264", "hevc"), listOf("aac", "mp3", "opus", "flac"), hdr = false, maxHeight = 1080)

    /** The controller runs on one thread, as on a phone's main thread. */
    private val main = Executors.newSingleThreadExecutor().asCoroutineDispatcher()

    @Test
    fun playsDirectlyWhenThePhoneCanAndThroughAConversionWhenItCannot() = runBlocking {
        val token = PasskeySignIn(ApiClient(ServerIntegrationTest.base, ServerIntegrationTest.http), ServerIntegrationTest.passkey, "Pixel 9 (player)")
            .signIn().token!!
        val (api, session) = ServerIntegrationTest.clientWith(token)
        val player = ExoPlayerEngine.playerClient(session)
        val movies = waitForMovies(api)
        val paper = movies.getValue("Paper Boats")
        val glass = movies.getValue("Glass Orchard")

        val engine = FakeEngine()
        val scope = CoroutineScope(SupervisorJob() + main)
        try {
            val controller = withContext(main) {
                PlayerController(ApiPlayerApi(api), engine, { phone }, scope, scope).also { it.open(PlayerRequest(paper.id)) }
            }

            // Paper Boats is an H.264/AAC MP4: the original file, with Range.
            val direct = waitForLoad(engine, 1).first
            assertFalse(direct.hls)
            assertEquals("/api/items/${paper.id}/file", direct.url.encodedPath)
            player.newCall(Request.Builder().url(direct.url).header("Range", "bytes=0-99").build()).execute().use {
                assertEquals(206, it.code)
                assertEquals(100, it.body.bytes().size)
            }

            // Paused at 3 seconds: the server keeps the place.
            withContext(main) {
                engine.ready(6_000)
                engine.positionMs = 3_000
                controller.pause()
            }
            withTimeout(10_000) {
                while (api.item(paper.id).progress?.positionS != 3.0) delay(100)
            }

            // Glass Orchard is an HEVC/AC-3 MKV: converted to HLS while it plays.
            withContext(main) { controller.open(PlayerRequest(glass.id)) }
            val converted = waitForLoad(engine, 2).first
            assertTrue(converted.hls)
            val sessionId = converted.url.pathSegments[2]
            assertEquals(sessionId, withContext(main) { controller.activeStream })
            val playlist = get(player, converted.url.toString())
            assertEquals(200, playlist.first)
            assertTrue(playlist.second.startsWith("#EXTM3U"))
            val firstSegment = playlist.second.lines().first { it.isNotBlank() && !it.startsWith("#") }
            val segment = get(player, converted.url.resolve(firstSegment)!!.toString())
            assertEquals("the first segment of the conversion", 200, segment.first)

            // Leaving ends the conversion on the server.
            withContext(main) { controller.close() }
            withTimeout(10_000) {
                while (get(player, converted.url.toString()).first != 404) delay(100)
            }
        } finally {
            scope.cancel()
            main.close()
        }
    }

    /** The startup scan indexes the fixture in the background. */
    private suspend fun waitForMovies(api: ApiClient): Map<String, Item> = withTimeout(60_000) {
        var movies = emptyMap<String, Item>()
        while ("Paper Boats" !in movies || "Glass Orchard" !in movies) {
            delay(500)
            movies = api.items(ItemQuery(category = Category.Movie, limit = 50)).items.associateBy { it.title }
        }
        movies
    }

    private suspend fun waitForLoad(engine: FakeEngine, count: Int) = withTimeout(30_000) {
        while (withContext(main) { engine.loads.size } < count) delay(50)
        withContext(main) { engine.loads[count - 1] }
    }

    private fun get(client: OkHttpClient, url: String): Pair<Int, String> =
        client.newCall(Request.Builder().url(url).build()).execute().use { it.code to it.body.string() }
}
