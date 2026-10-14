package com.thekozugroup.popcornstash.feature.settings

import com.thekozugroup.popcornstash.testing.plain
import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.ApiException
import com.thekozugroup.popcornstash.data.api.AuthInterceptor
import com.thekozugroup.popcornstash.data.api.Category
import com.thekozugroup.popcornstash.data.api.ContentType
import com.thekozugroup.popcornstash.data.api.StashEvent
import com.thekozugroup.popcornstash.data.api.Structure
import com.thekozugroup.popcornstash.data.api.Visibility
import com.thekozugroup.popcornstash.data.api.baseHttpClient
import com.thekozugroup.popcornstash.data.repo.Loadable
import com.thekozugroup.popcornstash.data.repo.StashData
import com.thekozugroup.popcornstash.testing.SoftPasskey
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import okhttp3.HttpUrl
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import org.junit.After
import org.junit.AfterClass
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.BeforeClass
import org.junit.Test
import java.io.File
import java.net.ServerSocket
import java.nio.file.Files
import java.util.concurrent.TimeUnit

/**
 * Settings' library management against a real `popcornstash serve` on a scripts/dev-fixture.sh
 * stash, through the same controllers the screens use and an owner's `full` token: make a
 * library from the fixture's own folder (taking camera videos from Home Videos after the
 * confirmation), rename it and share it with everyone, move it, see a name the server refuses
 * under the name field, make one with a new folder, and delete both, with the files still on
 * the disk.
 *
 * Runs only when the server binary is named, like ServerIntegrationTest:
 *   ./gradlew :app:testDebugUnitTest --tests '*SettingsServerIntegrationTest*' -PpopcornstashServer=../target/debug/popcornstash
 */
class SettingsServerIntegrationTest {
    companion object {
        private var process: Process? = null
        private lateinit var work: File
        private lateinit var stash: File
        private lateinit var log: File
        private lateinit var base: HttpUrl
        private lateinit var origin: String
        private lateinit var token: String
        private val http = baseHttpClient("PopcornStash-Android/settings-integration-test")

        @BeforeClass
        @JvmStatic
        fun startServer() {
            val binary = System.getProperty("popcornstash.serverBin")?.let(::File)
            assumeTrue("set -PpopcornstashServer to run the server integration test", binary?.canExecute() == true)
            val repo = File(requireNotNull(System.getProperty("popcornstash.repoRoot")))
            work = Files.createTempDirectory("popcornstash-android-settings-").toFile()
            stash = work.resolve("stash")
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
                val cookie = makeOwner(waitForSetupCode())
                // The app's token: an owner's full-access device token, made from the owner's browser session.
                val made = browserPost(
                    "/api/tokens",
                    buildJsonObject {
                        put("name", "Settings integration test")
                        put("scopes", buildJsonArray { add(kotlinx.serialization.json.JsonPrimitive("full")) })
                    },
                    cookie,
                ).first
                token = made.getValue("token").jsonPrimitive.content
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

        private fun makeOwner(setupCode: String): String {
            val passkey = SoftPasskey(origin)
            val started = browserPost("/api/auth/setup/start", buildJsonObject {
                put("setup_code", setupCode)
                put("display_name", "Ana Ribeiro")
            }).first
            val options = started.getValue("options").jsonObject.getValue("publicKey").jsonObject
            val (_, cookie) = browserPost("/api/auth/setup/finish", buildJsonObject {
                put("flow_id", started.getValue("flow_id").jsonPrimitive.content)
                put("credential", passkey.register(options))
                put("passkey_name", "Test laptop")
            })
            return requireNotNull(cookie) { "setup/finish set no cookie:\n${logText()}" }
        }

        private fun browserPost(path: String, body: JsonObject, cookie: String? = null): Pair<JsonObject, String?> {
            val request = Request.Builder()
                .url(base.resolve(path)!!)
                .header("Origin", origin)
                .apply { if (cookie != null) header("Cookie", cookie) }
                .post(body.toString().toRequestBody("application/json".toMediaType()))
                .build()
            http.newCall(request).execute().use { response ->
                val text = response.body.string()
                check(response.isSuccessful) { "$path answered ${response.code}: $text\n${logText()}" }
                val setCookie = response.headers("Set-Cookie").firstOrNull()?.substringBefore(';')
                return (if (text.isBlank()) JsonObject(emptyMap()) else Json.parseToJsonElement(text).jsonObject) to setCookie
            }
        }
    }

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val events = MutableSharedFlow<StashEvent>(extraBufferCapacity = 16)
    private lateinit var api: ApiClient
    private lateinit var data: StashData

    @Before
    fun setUp() {
        api = ApiClient(base, http.newBuilder().addInterceptor(AuthInterceptor(base, { token }, {})).build())
        data = StashData(api, events, scope, settleMillis = 0)
    }

    @After
    fun tearDown() = scope.cancel()

    private suspend fun <T> StateFlow<T>.until(check: (T) -> Boolean): T = withTimeout(30_000) { first(check) }

    private suspend fun editor(id: String? = null, folder: String? = null): LibraryEditorController {
        val editor = LibraryEditorController(api, data, scope, id, folder)
        editor.state.until { it.load == EditorLoad.Ready && it.libraries.isNotEmpty() && it.people.isNotEmpty() }
        return editor
    }

    @Test
    fun theOwnerManagesLibrariesFromSettings() = runBlocking {
        val settings = LibrarySettingsController(api, data, scope)
        settings.refresh()
        var list = settings.state.until { it.libraries is Loadable.Ready && it.people != null && it.folders.isNotEmpty() }
        assertEquals(listOf("Movies", "Shows", "Home Videos", "Photos"), list.shown!!.map { it.name })
        assertEquals("the fixture's own folder is not a library", listOf("Family 2019"), list.folders.map { it.name })

        // Make a library from the folder that is already there: private until the owner shares it.
        val family = editor(folder = "Family 2019")
        var form = family.state.value
        assertEquals("Family 2019", form.draft!!.name)
        assertEquals(Visibility.Selected, form.draft!!.visibility)
        assertEquals(ContentType.Video, form.draft!!.contentType)
        assertEquals("Sorted into Home Videos now", claimDescription(form.holderOf(Category.HomeVideo), null, checked = false))
        family.toggleClaim(Category.HomeVideo)
        family.save()
        assertEquals(ClaimMove(listOf(Category.HomeVideo), listOf("Home Videos")), family.state.value.confirmingClaims)
        family.confirmClaims()
        assertEquals("Family 2019 is ready.", family.state.until { it.finished != null || it.formError != null }.let { it.formError ?: it.finished!!.message })

        val made = api.libraries().first { it.folder == "Family 2019" }
        assertEquals(Visibility.Selected, made.visibility)
        assertEquals(emptyList<String>(), made.memberIds)
        assertEquals(listOf(Category.HomeVideo), made.claims)
        assertTrue("Home Videos gave up camera videos", api.libraries().first { it.name == "Home Videos" }.claims.isEmpty())

        // Rename it, give it an icon and share it with everyone; its folder stays.
        val edit = editor(id = made.id)
        edit.state.until { it.library?.isEmpty == false }
        assertTrue("its files lock the type", edit.state.value.locked)
        edit.setName("Holidays")
        edit.setIcon("plane")
        edit.setVisibility(Visibility.Everyone)
        edit.setType(ContentType.Photos, null)
        assertEquals("a locked type does not change", ContentType.Video, edit.state.value.draft!!.contentType)
        edit.save()
        assertEquals("Changes saved.", edit.state.until { it.finished != null || it.formError != null }.let { it.formError ?: it.finished!!.message })
        val renamed = api.library(made.id)
        assertEquals("Holidays", renamed.name)
        assertEquals("plane", renamed.icon)
        assertEquals(Visibility.Everyone, renamed.visibility)
        assertEquals("Family 2019", renamed.folder)

        // It was added last; move it above Photos.
        settings.refresh()
        settings.state.until { state -> state.shown?.any { it.name == "Holidays" } == true && !state.moving }
        settings.move(made.id, -1)
        settings.state.until { !it.moving && it.pendingOrder == null }
        assertEquals(listOf("Movies", "Shows", "Home Videos", "Holidays", "Photos"), api.libraries().map { it.name })

        // A name another library has: the server's sentence under the name field.
        val taken = editor()
        taken.setName("Movies")
        taken.setNewFolder("Movies again")
        taken.save()
        form = taken.state.until { !it.saving && it.errors.isNotEmpty() }
        assertTrue(form.errors[EditorField.Name], form.errors[EditorField.Name]!!.isNotBlank())
        assertNull(form.finished)

        // A library with a new folder: the folder is made on the disk.
        val kids = editor()
        kids.setName("Kids")
        kids.setIcon("baby")
        kids.setType(ContentType.Video, Structure.Episodes)
        kids.save()
        assertEquals("Kids is ready.", kids.state.until { it.finished != null || it.formError != null }.let { it.formError ?: it.finished!!.message })
        assertTrue(stash.resolve("Kids").isDirectory)
        val kidsLibrary = api.libraries().first { it.name == "Kids" }
        assertEquals(Visibility.Everyone, kidsLibrary.visibility)
        assertEquals(Structure.Episodes, kidsLibrary.structure)

        // Deleting takes them out of the app and leaves every file where it is.
        settings.refresh()
        list = settings.state.until { state -> state.shown?.size == 6 }
        settings.delete(list.shown!!.first { it.name == "Holidays" })
        assertEquals("Holidays is no longer in the app. Its files stay in Family 2019/.", plain(withTimeout(30_000) { settings.notices.first() }))
        val kidsEditor = editor(id = kidsLibrary.id)
        kidsEditor.askDelete()
        kidsEditor.confirmDelete()
        assertEquals(
            "Kids is no longer in the app. Its files stay in Kids/.",
            kidsEditor.state.until { it.finished != null || it.deleteError != null }.let { it.deleteError ?: it.finished!!.message },
        )
        assertEquals(listOf("Movies", "Shows", "Home Videos", "Photos"), api.libraries().map { it.name })
        assertTrue(stash.resolve("Family 2019/beach.mp4").isFile)
        assertTrue(stash.resolve("Kids").isDirectory)
        settings.refresh()
        list = settings.state.until { state -> state.folders.map { it.name }.containsAll(listOf("Family 2019", "Kids")) }
        assertFalse(list.shown!!.any { it.name == "Holidays" })
    }
}
