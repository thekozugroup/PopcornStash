package com.thekozugroup.popcornstash.integration

import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.ApiException
import com.thekozugroup.popcornstash.data.api.AuthInterceptor
import com.thekozugroup.popcornstash.data.api.ContentType
import com.thekozugroup.popcornstash.data.api.EventStream
import com.thekozugroup.popcornstash.data.api.Layout
import com.thekozugroup.popcornstash.data.api.Role
import com.thekozugroup.popcornstash.data.api.StashEvent
import com.thekozugroup.popcornstash.data.api.Structure
import com.thekozugroup.popcornstash.data.api.baseHttpClient
import com.thekozugroup.popcornstash.data.auth.PairingFlow
import com.thekozugroup.popcornstash.data.auth.PairingState
import com.thekozugroup.popcornstash.data.auth.PasskeySignIn
import com.thekozugroup.popcornstash.data.auth.ServerAddress
import com.thekozugroup.popcornstash.data.api.ItemQuery
import com.thekozugroup.popcornstash.data.api.Place
import com.thekozugroup.popcornstash.data.repo.Loadable
import com.thekozugroup.popcornstash.feature.item.ItemViewModel
import com.thekozugroup.popcornstash.feature.library.FileFetcher
import com.thekozugroup.popcornstash.feature.library.ItemSort
import com.thekozugroup.popcornstash.feature.library.LibraryViewModel
import com.thekozugroup.popcornstash.feature.library.MediaFormat
import com.thekozugroup.popcornstash.feature.library.ShowViewModel
import com.thekozugroup.popcornstash.testing.MainDispatcherRule
import com.thekozugroup.popcornstash.testing.keep
import com.thekozugroup.popcornstash.testing.SoftPasskey
import com.thekozugroup.popcornstash.testing.awaitUntil
import com.thekozugroup.popcornstash.ui.media.Pictures
import com.thekozugroup.popcornstash.ui.icons.LibraryIcons
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.emptyFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.onEach
import kotlinx.coroutines.flow.toList
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
import org.junit.Rule
import org.junit.Test
import java.io.File
import java.net.ServerSocket
import java.nio.file.Files
import java.util.concurrent.TimeUnit

/**
 * The app's own client against a real `popcornstash serve` on a scripts/dev-fixture.sh
 * stash. The owner is made the way tests/e2e makes it, with a software passkey and the
 * setup code from the server's log; the app then pairs (approved with the owner's browser
 * session) and signs in with a passkey (`want_token`), and reads the libraries, Home and
 * live events with its token.
 *
 * Runs only when the server binary is named (android/README.md, Tests):
 *   ./gradlew :app:testDebugUnitTest --tests '*ServerIntegrationTest*' -PpopcornstashServer=../target/debug/popcornstash
 */
class ServerIntegrationTest {
    /** View models run their work on a test dispatcher (Home and library pages, below). */
    @get:Rule
    val main = MainDispatcherRule()

    companion object {
        private var process: Process? = null
        private lateinit var work: File
        private lateinit var log: File
        internal lateinit var base: HttpUrl
        private lateinit var origin: String
        internal lateinit var passkey: SoftPasskey
        private lateinit var ownerCookie: String
        internal val http: OkHttpClient = baseHttpClient("PopcornStash-Android/integration-test")

        /** The signing certificate the server's assetlinks lists for the app (any 32 bytes do for the test). */
        private const val CERT_SHA256 =
            "14:6D:E9:83:C5:73:06:50:D8:EE:B9:95:2F:34:FC:64:16:A0:83:42:E6:1D:BE:A8:8A:04:96:B2:3F:CF:44:E5"

        /** The WebAuthn origin Credential Manager gives the app signed with that certificate. */
        private val APP_ORIGIN = "android:apk-key-hash:" + java.util.Base64.getUrlEncoder().withoutPadding()
            .encodeToString(CERT_SHA256.split(':').map { it.toInt(16).toByte() }.toByteArray())

        private val ASSETLINKS = """[{"relation":["delegate_permission/common.handle_all_urls",
            "delegate_permission/common.get_login_creds"],"target":{"namespace":"android_app",
            "package_name":"com.thekozugroup.popcornstash","sha256_cert_fingerprints":["$CERT_SHA256"]}}]"""

        @BeforeClass
        @JvmStatic
        fun startServer() {
            val binary = System.getProperty("popcornstash.serverBin")?.let(::File)
            assumeTrue("set -PpopcornstashServer to run the server integration test", binary?.canExecute() == true)
            val repo = File(requireNotNull(System.getProperty("popcornstash.repoRoot")))
            work = Files.createTempDirectory("popcornstash-android-it-").toFile()
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
            val builder = ProcessBuilder(binary!!.path, "serve")
                .directory(repo)
                .redirectErrorStream(true)
                .redirectOutput(log)
            builder.environment().keys.removeAll { it.startsWith("STASH_") }
            builder.environment().putAll(
                mapOf(
                    "STASH_ROOT" to stash.path,
                    "STASH_CONFIG" to config.path,
                    "STASH_PUBLIC_URL" to origin,
                    "STASH_BIND" to "127.0.0.1:$port",
                    "STASH_LOG" to "info,tower_http=warn",
                    "NO_COLOR" to "1",
                    "STASH_ANDROID_ASSETLINKS" to ASSETLINKS,
                ),
            )
            process = builder.start()
            runBlocking {
                waitForHealth()
                passkey = SoftPasskey(origin)
                ownerCookie = makeOwner(waitForSetupCode())
                // From here on the passkey answers as Credential Manager does in the app.
                passkey.signInOrigin = APP_ORIGIN
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

        /** Registers the owner with the setup code and the software passkey; returns the browser session cookie. */
        private fun makeOwner(setupCode: String): String {
            val started = browserPost("/api/auth/setup/start", buildJsonObject {
                put("setup_code", setupCode)
                put("display_name", "Ana Ribeiro")
            }).first
            val flowId = started.getValue("flow_id").jsonPrimitive.content
            val options = started.getValue("options").jsonObject.getValue("publicKey").jsonObject
            val credential = passkey.register(options)
            val (_, cookie) = browserPost("/api/auth/setup/finish", buildJsonObject {
                put("flow_id", flowId)
                put("credential", credential)
                put("passkey_name", "Test laptop")
            })
            return requireNotNull(cookie) { "setup/finish set no cookie:\n${logText()}" }
        }

        /** A request as the owner's browser makes it: same-origin, JSON, with the session cookie once there is one. */
        fun browserPost(path: String, body: JsonObject, cookie: String? = null): Pair<JsonObject, String?> {
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

        fun clientWith(token: String): Pair<ApiClient, OkHttpClient> {
            val session = http.newBuilder().addInterceptor(AuthInterceptor(base, { token }, {})).build()
            return ApiClient(base, session) to session
        }
    }

    @Test
    fun theAppChecksTheAddressLikeSignInDoes() = runBlocking {
        val parsed = ServerAddress.parse("$origin/settings", allowLocalHttp = true) as ServerAddress.Result.Valid
        assertEquals(base, parsed.url)
        val api = ApiClient(parsed.url, http)
        assertEquals("ok", api.health().status)
        val state = api.authState()
        assertTrue("passkeys work on localhost", state.passkeysReady)
        assertTrue("the owner exists", !state.setupRequired)
        assertTrue("the server lists the app for passkeys", api.listsAppForPasskeys("com.thekozugroup.popcornstash"))
    }

    @Test
    fun pairsWithACodeTheOwnerApprovesThenReadsLibrariesAndHome() = runBlocking {
        var approved = false
        val states = withTimeout(60_000) {
            PairingFlow(ApiClient(base, http), "Pixel 9 (integration test)").states()
                .onEach { state ->
                    if (state is PairingState.Waiting && !approved) {
                        assertEquals("$origin/pair?code=${state.userCode}", state.verifyUrl)
                        // The owner types the code on /pair in a signed-in browser and approves it.
                        browserPost("/api/pairing/approve", buildJsonObject { put("user_code", state.userCode) }, ownerCookie)
                        approved = true
                    }
                }
                .toList()
        }
        val last = states.last()
        assertTrue("pairing ended with $last", last is PairingState.Approved)
        last as PairingState.Approved
        assertEquals("an Android phone gets the full scope", listOf("full"), last.scopes)
        readLibrariesAndHome(last.token)
    }

    /**
     * The app's passkey sign-in: no Origin header (it is not a browser), the app's
     * `android:apk-key-hash:` origin in the signed client data, `want_token: true`.
     */
    @Test
    fun signsInWithAPasskeyForABearerSessionThenReadsLibrariesAndHome() = runBlocking {
        val finished = PasskeySignIn(ApiClient(base, http), passkey, "Pixel 9 (integration test)").signIn()
        assertEquals("Ana Ribeiro", finished.user.displayName)
        assertEquals(Role.Owner, finished.user.role)
        readLibrariesAndHome(finished.token!!)
    }

    @Test
    fun liveEventsArriveOverTheBearerStream() = runBlocking {
        val token = PasskeySignIn(ApiClient(base, http), passkey, "Pixel 9 (events)").signIn().token!!
        val (api, session) = clientWith(token)
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
        try {
            val stream = EventStream(api, session, scope)
            stream.start()
            withTimeout(15_000) { stream.status.first { it == EventStream.Status.Connected } }
            val event = scope.async { withTimeout(15_000) { stream.events.first { it is StashEvent.DownloadChanged } } }
            // 203.0.113.0/24 is a documentation range: a public address that nothing answers.
            val added = api.addDownload("http://203.0.113.10/android/holiday-clip.mp4")
            val seen = event.await() as StashEvent.DownloadChanged
            assertEquals(added.id, seen.download.id)
            api.removeDownload(added.id)
            stream.stop()
        } finally {
            scope.cancel()
        }
    }

    /**
     * Home and the pages it opens, through the app's own view models, on the fixture: Movies as
     * posters, Shows with The Lantern Keepers and what plays next, the photo timeline, an item
     * with its favourite, thumbnails and an Other file fetched with the bearer client.
     */
    @Test
    fun homeAndTheLibraryPagesReadARealStash() = runBlocking {
        val token = PasskeySignIn(ApiClient(base, http), passkey, "Pixel 9 (libraries)").signIn().token!!
        val (api, session) = clientWith(token)
        readLibrariesAndHome(token)
        val libraries = api.libraries().associateBy { it.name }

        val movies = main.keep { LibraryViewModel(api, emptyFlow(), emptyFlow(), libraries.getValue("Movies").id) }
        val items = (movies.ui.awaitUntil { it.content is LibraryViewModel.Content.Items }.content as LibraryViewModel.Content.Items)
        assertEquals(ItemSort.Added, items.sort)
        assertTrue(items.page.items.map { it.title }.containsAll(listOf("Paper Boats", "Glass Orchard", "Tin Lighthouse")))

        val shows = main.keep { LibraryViewModel(api, emptyFlow(), emptyFlow(), libraries.getValue("Shows").id) }
        val summaries = (shows.ui.awaitUntil { it.content is LibraryViewModel.Content.Shows }.content as LibraryViewModel.Content.Shows).shows
        val lantern = summaries.first { it.showName == "The Lantern Keepers" }
        assertEquals(2L, lantern.seasonCount)
        val show = main.keep { ShowViewModel(api, emptyFlow(), emptyFlow(), libraries.getValue("Shows").id, lantern.showName) }
        val showUi = show.ui.awaitUntil { it.show is Loadable.Ready }
        assertEquals(listOf(1, 2), showUi.show.valueOrNull!!.seasons.map { it.season })
        assertEquals("S01E01", MediaFormat.episodeCode(showUi.upNext!!.episode.season, showUi.upNext!!.episode.episode))

        val photos = main.keep { LibraryViewModel(api, emptyFlow(), emptyFlow(), libraries.getValue("Photos").id) }
        val timeline = photos.ui.awaitUntil { it.content is LibraryViewModel.Content.Timeline }.content as LibraryViewModel.Content.Timeline
        assertTrue("the fixture's three photos from May 2022", timeline.page.items.any { it.month == "2022-05" && it.count >= 3 })

        val paperBoats = items.page.items.first { it.title == "Paper Boats" }
        val item = main.keep { ItemViewModel(api, emptyFlow(), emptyFlow(), paperBoats.id) }
        val detail = item.ui.awaitUntil { it.item is Loadable.Ready }.item.valueOrNull!!
        assertEquals(2011, detail.year)
        item.toggleFavourite()
        assertTrue(item.ui.awaitUntil { !it.savingFavourite }.item.valueOrNull!!.favorite)
        assertTrue("the server kept it", api.item(paperBoats.id).favorite)
        item.toggleFavourite()
        item.ui.awaitUntil { !it.savingFavourite }

        // Thumbnails come with the session's token; an Other file is fetched the same way.
        val pictures = Pictures(base)
        val withThumb = withTimeout(60_000) {
            var found = api.items(ItemQuery(libraryId = libraries.getValue("Movies").id)).items.firstOrNull { it.thumb != null }
            while (found == null) {
                delay(500)
                found = api.items(ItemQuery(libraryId = libraries.getValue("Movies").id)).items.firstOrNull { it.thumb != null }
            }
            found
        }
        session.newCall(Request.Builder().url(pictures.thumb(withThumb, large = true)!!).build()).execute().use {
            assertEquals(200, it.code)
            assertEquals("image/webp", it.header("Content-Type"))
        }
        val other = api.items(ItemQuery(place = Place.OtherFiles)).items.first { it.title == "Recipe Card" }
        val copy = File(work, "opened/${other.id}/Recipe Card.pdf")
        FileFetcher(session).download(api.fileUrl(other.id), copy)
        assertTrue(copy.readBytes().copyOfRange(0, 4).contentEquals("%PDF".toByteArray()))
    }

    private suspend fun readLibrariesAndHome(token: String) {
        val (api, _) = clientWith(token)
        val me = api.me()
        assertEquals(Role.Owner, me.role)

        val libraries = api.libraries()
        assertEquals(listOf("Movies", "Shows", "Home Videos", "Photos"), libraries.map { it.name })
        assertEquals(listOf("film", "tv", "clapperboard", "image"), libraries.map { it.icon })
        assertTrue(libraries.all { it.icon in LibraryIcons.names })
        assertEquals(listOf(ContentType.Video, ContentType.Video, ContentType.Video, ContentType.Photos), libraries.map { it.contentType })
        assertEquals(listOf(Structure.OneOffs, Structure.Episodes, Structure.OneOffs, null), libraries.map { it.structure })
        assertEquals(listOf(Layout.Posters, Layout.Series, Layout.Frames, Layout.Timeline), libraries.map { it.layout })
        assertEquals(listOf(0, 1, 2, 3), libraries.map { it.position })
        assertEquals("the owner sees each folder", listOf("Movies", "Shows", "Home Videos", "Photos"), libraries.map { it.folder })
        assertEquals(LibraryIcons.names, api.libraryIcons())

        // The startup scan indexes the fixture in the background: wait until Movies shows its films.
        val home = withTimeout(60_000) {
            var latest = api.home()
            while (latest.libraries.firstOrNull { it.library.name == "Movies" }?.recent.orEmpty().size < 3) {
                delay(500)
                latest = api.home()
            }
            latest
        }
        assertEquals(listOf("Movies", "Shows", "Home Videos", "Photos"), home.libraries.map { it.library.name })
        val movies = home.libraries.first { it.library.name == "Movies" }
        assertTrue(movies.recent.map { it.title }.containsAll(listOf("Paper Boats", "Glass Orchard", "Tin Lighthouse")))
        val shows = home.libraries.first { it.library.name == "Shows" }
        assertTrue("The Lantern Keepers" in shows.showPosters)
        // The series shelf has a tile for every show the heading counts (up to 12).
        val shelf = shows.shows.orEmpty()
        assertTrue("The Lantern Keepers" in shelf.map { it.showName })
        assertEquals(minOf(shows.library.showCount ?: 0L, 12L), shelf.size.toLong())
        assertTrue("Other files are counted", home.otherFilesCount >= 2)
        val firstThumb = movies.recent.first().thumb
        if (firstThumb != null) assertEquals(base.host, api.absolute(firstThumb).host)
    }
}
