package com.thekozugroup.popcornstash.integration

import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.ApiException
import com.thekozugroup.popcornstash.data.api.AuthInterceptor
import com.thekozugroup.popcornstash.data.api.DownloadKind
import com.thekozugroup.popcornstash.data.api.EventStream
import com.thekozugroup.popcornstash.data.api.ItemQuery
import com.thekozugroup.popcornstash.data.api.Place
import com.thekozugroup.popcornstash.data.api.UploadCreate
import com.thekozugroup.popcornstash.data.api.User
import com.thekozugroup.popcornstash.data.api.baseHttpClient
import com.thekozugroup.popcornstash.data.auth.PasskeySignIn
import com.thekozugroup.popcornstash.data.repo.StashData
import com.thekozugroup.popcornstash.feature.downloads.DownloadsController
import com.thekozugroup.popcornstash.feature.downloads.FinishedOutcome
import com.thekozugroup.popcornstash.feature.downloads.ImportTarget
import com.thekozugroup.popcornstash.feature.downloads.text
import com.thekozugroup.popcornstash.share.SharedFile
import com.thekozugroup.popcornstash.share.UploadBatch
import com.thekozugroup.popcornstash.share.UploadBatchStore
import com.thekozugroup.popcornstash.share.UploadOutcome
import com.thekozugroup.popcornstash.share.UploadRunner
import com.thekozugroup.popcornstash.share.ChunkSource
import com.thekozugroup.popcornstash.testing.SoftPasskey
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import okhttp3.HttpUrl
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import org.junit.AfterClass
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.BeforeClass
import org.junit.Test
import java.io.File
import java.net.ServerSocket
import java.nio.file.Files
import java.util.concurrent.TimeUnit

/**
 * The Downloads tab and the share sheet's uploads against a real `popcornstash serve` on a
 * scripts/dev-fixture.sh stash, signed in with a passkey for a bearer session as the app is:
 * the VPN line of a server without torrents, the fixture's file that waits for review
 * accepted into the library its row shows, and a shared file uploaded in two runs (the
 * second continuing the first) that then waits in Ready to import and goes to Other files.
 *
 * Runs only when the server binary is named (android/README.md, Tests):
 *   ./gradlew :app:testDebugUnitTest --tests '*DownloadsServerIntegrationTest*' -PpopcornstashServer=../target/debug/popcornstash
 */
class DownloadsServerIntegrationTest {
    companion object {
        private var process: Process? = null
        private lateinit var work: File
        private lateinit var log: File
        private lateinit var base: HttpUrl
        private lateinit var origin: String
        private lateinit var token: String
        private lateinit var owner: User
        private val http: OkHttpClient = baseHttpClient("PopcornStash-Android/integration-test")

        @BeforeClass
        @JvmStatic
        fun startServer() {
            val binary = System.getProperty("popcornstash.serverBin")?.let(::File)
            assumeTrue("set -PpopcornstashServer to run the server integration test", binary?.canExecute() == true)
            val repo = File(requireNotNull(System.getProperty("popcornstash.repoRoot")))
            work = Files.createTempDirectory("popcornstash-android-downloads-").toFile()
            val stash = work.resolve("stash")
            val config = work.resolve("config").apply { mkdirs() }
            log = work.resolve("server.log")
            val fixture = ProcessBuilder(repo.resolve("scripts/dev-fixture.sh").path, stash.path)
                .redirectErrorStream(true)
                .redirectOutput(work.resolve("fixture.log"))
                .start()
            check(fixture.waitFor(5, TimeUnit.MINUTES) && fixture.exitValue() == 0) {
                "scripts/dev-fixture.sh failed:\n${work.resolve("fixture.log").readText()}"
            }
            val port = ServerSocket(0).use { it.localPort }
            origin = "http://localhost:$port"
            base = "$origin/".toHttpUrl()
            val builder = ProcessBuilder(binary!!.path, "serve").directory(repo).redirectErrorStream(true).redirectOutput(log)
            builder.environment().keys.removeAll { it.startsWith("STASH_") }
            builder.environment().putAll(
                mapOf(
                    "STASH_ROOT" to stash.path,
                    "STASH_CONFIG" to config.path,
                    "STASH_PUBLIC_URL" to origin,
                    "STASH_BIND" to "127.0.0.1:$port",
                    "STASH_LOG" to "info,tower_http=warn",
                    "NO_COLOR" to "1",
                ),
            )
            process = builder.start()
            runBlocking {
                waitForHealth()
                val passkey = SoftPasskey(origin)
                makeOwner(passkey, waitForSetupCode())
                val signedIn = PasskeySignIn(ApiClient(base, http), passkey, "Pixel 9 (downloads test)").signIn()
                token = signedIn.token!!
                owner = signedIn.user
            }
        }

        @AfterClass
        @JvmStatic
        fun stopServer() {
            process?.let {
                it.destroy()
                if (!it.waitFor(10, TimeUnit.SECONDS)) it.destroyForcibly()
            }
            if (::work.isInitialized && System.getenv("POPCORNSTASH_KEEP") == null) work.deleteRecursively()
        }

        private fun logText() = if (log.exists()) log.readText() else ""

        private suspend fun waitForHealth() {
            val api = ApiClient(base, http)
            repeat(300) {
                check(process!!.isAlive) { "The server exited during startup:\n${logText()}" }
                try {
                    if (api.health().status == "ok") return
                } catch (_: ApiException) {
                    // Not listening yet.
                }
                delay(200)
            }
            error("The server did not answer within a minute:\n${logText()}")
        }

        private suspend fun waitForSetupCode(): String {
            repeat(150) {
                Regex("""\b([A-Z0-9]{4}-[A-Z0-9]{4}-[A-Z0-9]{4})\b""").find(logText())?.let { return it.groupValues[1] }
                delay(200)
            }
            error("No setup code in the server log:\n${logText()}")
        }

        private fun makeOwner(passkey: SoftPasskey, setupCode: String) {
            val started = browserPost("/api/auth/setup/start", buildJsonObject {
                put("setup_code", setupCode)
                put("display_name", "Ana Ribeiro")
            })
            val credential = passkey.register(started.getValue("options").jsonObject.getValue("publicKey").jsonObject)
            browserPost("/api/auth/setup/finish", buildJsonObject {
                put("flow_id", started.getValue("flow_id").jsonPrimitive.content)
                put("credential", credential)
                put("passkey_name", "Test laptop")
            })
        }

        private fun browserPost(path: String, body: JsonObject): JsonObject {
            val request = Request.Builder().url(base.resolve(path)!!).header("Origin", origin)
                .post(body.toString().toRequestBody("application/json".toMediaType())).build()
            http.newCall(request).execute().use { response ->
                val text = response.body.string()
                check(response.isSuccessful) { "$path answered ${response.code}: $text\n${logText()}" }
                return if (text.isBlank()) JsonObject(emptyMap()) else Json.parseToJsonElement(text).jsonObject
            }
        }
    }

    private fun api(): ApiClient = ApiClient(base, http.newBuilder().addInterceptor(AuthInterceptor(base, { token }, {})).build())

    private fun <T> withController(block: suspend (DownloadsController) -> T): T = runBlocking {
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
        try {
            val api = api()
            val data = StashData(api, MutableSharedFlow(), scope)
            val controller = DownloadsController(
                api, data, MutableStateFlow(owner), MutableStateFlow(EventStream.Status.Connected), MutableStateFlow(emptyList()), {}, scope,
            )
            withTimeout(90_000) { block(controller) }
        } finally {
            scope.cancel()
        }
    }

    @Test
    fun aServerWithoutTorrentsSaysSoOnTheVpnLine() = withController { controller ->
        controller.refreshNow()
        val state = controller.state.first { it.vpn.label != "Checking VPN" }
        assertEquals("Torrents off", state.vpn.label)
        assertTrue(state.vpn.detail.endsWith("Web links and uploads still work."))
    }

    @Test
    fun theFileWaitingForReviewGoesWhereItsRowSays() = withController { controller ->
        // The startup scan puts clip_final_v2.mkv in Ready to import, suggested for Movies.
        var row = controller.state.value.imports.firstOrNull { it.fileName == "clip_final_v2.mkv" }
        while (row == null) {
            controller.refreshNow()
            row = controller.state.value.imports.firstOrNull { it.fileName == "clip_final_v2.mkv" }
            if (row == null) delay(500)
        }
        // The server's own sentence about the file, as the web shows it.
        assertEquals(row.item.suggestion?.reason, row.reason)
        assertTrue("a video may go to any video library: ${row.choices}", row.choices.map { it.name }.containsAll(listOf("Movies", "Shows", "Home Videos")))
        // Whatever the row shows is where it goes; here the person picks Movies in the picker.
        controller.choose(row.item.id, row.choices.first { it.name == "Movies" })
        val chosen = controller.state.first { s -> s.imports.first { it.item.id == row.item.id }.target?.name == "Movies" }
            .imports.first { it.item.id == row.item.id }
        controller.accept(chosen)
        assertEquals("Moved to Movies.", controller.messages.first())
        val movies = api().libraries().first { it.name == "Movies" }
        val items = api().items(ItemQuery(libraryId = movies.id, limit = 100)).items
        assertTrue(items.map { it.relPath }.toString(), items.any { it.relPath.startsWith("Movies/") && it.relPath.endsWith(".mkv") && "clip" in it.relPath })
    }

    @Test
    fun aSharedFileUploadsAcrossTwoRunsThenWaitsAndGoesToOtherFiles() {
        val bytes = "Packing list for the lake: towels, sun hats, the blue cooler.\n".repeat(400).toByteArray()
        val store = UploadBatchStore(work.resolve("uploads-${System.nanoTime()}"))
        val batch = UploadBatch(
            id = "batch-it",
            userId = owner.id,
            userName = owner.displayName,
            server = base.toString(),
            files = listOf(SharedFile("content://test/packing-list.txt", "Lake packing list.txt", bytes.size.toLong(), "text/plain", 1_560_000_000)),
        )
        store.save(batch)
        runBlocking {
            // A first run that the phone cut off after half of the file arrived.
            val api = api()
            val started = api.startUpload(UploadCreate("Lake packing list.txt", bytes.size.toLong(), "text/plain", 1_560_000_000))
            api.uploadChunk(started.id, 0, bytes.copyOfRange(0, bytes.size / 2).toRequestBody("application/octet-stream".toMediaType()))
            store.update("batch-it", 0) { it.copy(uploadId = started.id) }
            // The job runs again and continues from what the server has.
            val outcome = UploadRunner(
                store = store,
                apiFor = { if (it.userId == owner.id) api else null },
                open = { InMemory(bytes) },
                measure = { bytes.size.toLong() },
            ).run("batch-it") {}
            assertEquals(UploadOutcome.Done(1), outcome)
            // The second run used the same upload: the server made no other one for this file.
            val uploads = api.downloads("all").filter { it.kind == DownloadKind.Upload && it.name == "Lake packing list.txt" }
            assertEquals(1, uploads.size)
        }
        withController { controller ->
            var row = controller.state.value.imports.firstOrNull { it.fileName == "Lake packing list.txt" }
            while (row == null) {
                controller.refreshNow()
                row = controller.state.value.imports.firstOrNull { it.fileName == "Lake packing list.txt" }
                if (row == null) delay(500)
            }
            assertEquals(listOf(ImportTarget.OtherFiles), row.choices)
            assertEquals(ImportTarget.OtherFiles, row.target)
            val upload = controller.state.value.finished.first { it.download.kind == DownloadKind.Upload && it.download.name == "Lake packing list.txt" }
            assertTrue("waiting: ${upload.outcome}", upload.outcome is FinishedOutcome.Waiting)
            controller.accept(row)
            assertEquals("Moved to Other files.", controller.messages.first())
            // The finished row now says where it went.
            var outcome = upload.outcome
            while (outcome.text() != "In Other files") {
                delay(300)
                controller.refreshNow()
                outcome = controller.state.value.finished.first { it.download.id == upload.download.id }.outcome
            }
            val other = api().items(ItemQuery(place = Place.OtherFiles, limit = 100)).items
            val stored = other.first { it.title.startsWith("Lake packing list") }
            assertEquals(bytes.size.toLong(), stored.size)
        }
    }

    private class InMemory(private val bytes: ByteArray) : ChunkSource {
        override fun read(offset: Long, length: Int): ByteArray =
            bytes.copyOfRange(offset.toInt(), (offset + length).toInt().coerceAtMost(bytes.size))

        override fun close() = Unit
    }
}
