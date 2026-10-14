package com.thekozugroup.popcornstash.feature.downloads

import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.Download
import com.thekozugroup.popcornstash.data.api.DownloadList
import com.thekozugroup.popcornstash.data.api.DownloadState
import com.thekozugroup.popcornstash.data.api.EventStream
import com.thekozugroup.popcornstash.data.api.Inbox
import com.thekozugroup.popcornstash.data.api.ItemDetail
import com.thekozugroup.popcornstash.data.api.LibraryList
import com.thekozugroup.popcornstash.data.api.Role
import com.thekozugroup.popcornstash.data.api.StashEvent
import com.thekozugroup.popcornstash.data.api.StashJson
import com.thekozugroup.popcornstash.data.api.User
import com.thekozugroup.popcornstash.data.api.VpnStatus
import com.thekozugroup.popcornstash.data.api.baseHttpClient
import com.thekozugroup.popcornstash.data.repo.StashData
import com.thekozugroup.popcornstash.share.UploadBatchProgress
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import mockwebserver3.Dispatcher
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import mockwebserver3.RecordedRequest
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import java.util.concurrent.CopyOnWriteArrayList

/**
 * The Downloads tab against a server that answers like PopcornStash (MockWebServer): what it
 * loads, and exactly what each action sends (docs/architecture.md section 8).
 */
class DownloadsControllerTest {
    private val s = DownloadsSamples
    private val server = MockWebServer()
    private val requests = CopyOnWriteArrayList<RecordedRequest>()
    private val answers = mutableMapOf<String, () -> MockResponse>()
    private lateinit var scope: CoroutineScope
    private lateinit var api: ApiClient
    private lateinit var data: StashData
    private val events = MutableSharedFlow<StashEvent>(extraBufferCapacity = 16)
    private val user = MutableStateFlow(User("u-owner", "Ana Ribeiro", Role.Owner))
    private val connection = MutableStateFlow(EventStream.Status.Connected)
    private val uploads = MutableStateFlow<List<UploadBatchProgress>>(emptyList())
    private val stopped = mutableListOf<String>()

    private var downloads = listOf(s.downloading, s.paused, s.queued, s.sorted, s.failed)
    private var inbox = Inbox(items = listOf(s.lowConfidence, s.episode, s.photo, s.document), ignored = listOf(s.ignoredOne))

    private fun json(body: String, code: Int = 200) =
        MockResponse.Builder().code(code).addHeader("Content-Type", "application/json").body(body).build()

    private fun error(code: Int, kind: String, message: String, field: String? = null) =
        json("""{"error":{"code":"$kind","message":"$message"${field?.let { ""","field":"$it"""" } ?: ""}}}""", code)

    @Before
    fun setUp() {
        answers["GET /api/downloads"] = { json(StashJson.encodeToString(DownloadList.serializer(), DownloadList(downloads))) }
        answers["GET /api/inbox"] = { json(StashJson.encodeToString(Inbox.serializer(), inbox)) }
        answers["GET /api/libraries"] = { json(StashJson.encodeToString(LibraryList.serializer(), LibraryList(s.libraries))) }
        answers["GET /api/vpn/status"] = { json(StashJson.encodeToString(VpnStatus.serializer(), s.vpn)) }
        server.dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse {
                requests += request
                val key = "${request.method} ${request.url.encodedPath}"
                return answers[key]?.invoke() ?: MockResponse.Builder().code(404).build()
            }
        }
        server.start()
        api = ApiClient(server.url("/"), baseHttpClient("test"))
        scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        data = StashData(api, events, scope)
    }

    @After
    fun tearDown() {
        scope.cancel()
        server.close()
    }

    private fun controller() = DownloadsController(api, data, user, connection, uploads, { stopped += it }, scope)

    private fun <T> eventually(block: suspend () -> T): T = runBlocking { withTimeout(5_000) { block() } }

    private fun DownloadsController.loaded(): DownloadsUiState = eventually {
        refreshNow()
        // Every row has its place once the libraries are in (the state follows the loads a moment later).
        state.first { !it.loading && it.imports.size == 4 && it.vpn.label.startsWith("Protected") }
    }

    private fun sent(method: String, path: String): RecordedRequest =
        requests.lastOrNull { it.method == method && it.url.encodedPath == path } ?: error("No $method $path reached the server")

    private fun RecordedRequest.json() = StashJson.parseToJsonElement(body!!.utf8()).jsonObject

    @Test
    fun theTabShowsWhatIsDownloadingWhatWaitsAndWhatFinished() {
        val state = controller().loaded()
        assertEquals(listOf("d-lantern", "d-trailer", "d-puddle"), state.inProgress.map { it.id })
        assertEquals(listOf("d-harbor", "d-iso"), state.finished.map { it.download.id })
        assertEquals("In Movies", state.finished.first().outcome.text())
        assertEquals(listOf("Movies", "Shows", "Photos", "Other files"), state.imports.map { it.target?.name })
        assertEquals("Not sure this is a movie.", state.imports.first().reason)
        assertEquals(1, state.ignored.size)
        assertEquals("Protected · NordVPN · Netherlands", state.vpn.label)
        assertEquals(4, state.readyToAccept.size)
        assertTrue(state.isOwner)
    }

    @Test
    fun acceptSendsThePlaceAndLibraryTheRowShowsThenHidesTheRow() {
        val controller = controller()
        val row = controller.loaded().imports.first()
        answers["POST /api/inbox/i-clip"] = { json(StashJson.encodeToString(ItemDetail.serializer(), s.lowConfidence)) }
        inbox = inbox.copy(items = inbox.items.drop(1))
        controller.accept(row)
        assertEquals("Moved to Movies.", eventually { controller.messages.first() })
        val body = sent("POST", "/api/inbox/i-clip").json()
        assertEquals("accept", body["action"]!!.jsonPrimitive.content)
        assertEquals("library", body["place"]!!.jsonPrimitive.content)
        assertEquals("lib-movies", body["library_id"]!!.jsonPrimitive.content)
        assertTrue(eventually { controller.state.first { s -> s.imports.none { it.item.id == "i-clip" } } }.imports.isNotEmpty())
    }

    @Test
    fun aLibraryChosenInThePickerIsTheOneSent() {
        val controller = controller()
        val row = controller.loaded().imports.first()
        controller.choose(row.item.id, ImportTarget(s.kids))
        val chosen = eventually { controller.state.first { it.imports.first().target?.name == "Kids" } }.imports.first()
        // Kids is a series: without a show and an episode, Accept opens Edit instead of sending.
        assertTrue(chosen.wantsDetails)
        controller.accept(chosen)
        val edit = eventually { controller.state.first { it.edit != null } }.edit!!
        assertEquals("lib:lib-kids", edit.form.targetKey)
        controller.choose(row.item.id, ImportTarget.OtherFiles)
        answers["POST /api/inbox/i-clip"] = { json(StashJson.encodeToString(ItemDetail.serializer(), s.lowConfidence)) }
        controller.closeEdit()
        controller.accept(eventually { controller.state.first { it.imports.first().target == ImportTarget.OtherFiles } }.imports.first())
        assertEquals("Moved to Other files.", eventually { controller.messages.first() })
        val body = sent("POST", "/api/inbox/i-clip").json()
        assertEquals("other_files", body["place"]!!.jsonPrimitive.content)
        assertNull(body["library_id"])
    }

    @Test
    fun aTargetThatChangedMeanwhileSaysSoAndReloads() {
        val controller = controller()
        val row = controller.loaded().imports[1]
        answers["POST /api/inbox/i-puddle"] = {
            error(409, "conflict", "Where this file goes changed. Check it and accept again.", "library_id")
        }
        val before = requests.count { it.url.encodedPath == "/api/inbox" }
        controller.accept(row)
        assertEquals("Where this file goes changed. Check it and accept again.", eventually { controller.messages.first() })
        eventually { while (requests.count { it.url.encodedPath == "/api/inbox" } <= before) kotlinx.coroutines.delay(20) }
    }

    @Test
    fun ignoreLeavesTheFileWhereItIs() {
        val controller = controller()
        val row = controller.loaded().imports.last()
        answers["POST /api/inbox/i-doc"] = { json(StashJson.encodeToString(ItemDetail.serializer(), s.document)) }
        controller.ignore(row)
        assertEquals("Ignored. School trip permission.pdf stays where it is.", eventually { controller.messages.first() })
        assertEquals("ignore", sent("POST", "/api/inbox/i-doc").json()["action"]!!.jsonPrimitive.content)
    }

    @Test
    fun acceptAllSendsEachRowsOwnTargetOldestFirstAndCountsFailures() {
        val controller = controller()
        controller.loaded()
        answers["POST /api/inbox/i-clip"] = { json(StashJson.encodeToString(ItemDetail.serializer(), s.lowConfidence)) }
        answers["POST /api/inbox/i-puddle"] = { json(StashJson.encodeToString(ItemDetail.serializer(), s.episode)) }
        answers["POST /api/inbox/i-photo"] = {
            error(409, "conflict", "This library's folder is missing or cannot be written, so nothing can move there now.", "library_id")
        }
        answers["POST /api/inbox/i-doc"] = { json(StashJson.encodeToString(ItemDetail.serializer(), s.document)) }
        controller.askAcceptAll()
        assertEquals(4, eventually { controller.state.first { it.acceptAll != null } }.acceptAll?.total)
        controller.confirmAcceptAll()
        assertEquals("3 files moved. 1 file did not move and is still here.", eventually { controller.messages.first() })
        eventually { controller.state.first { it.acceptAll == null } }
        val order = requests.filter { it.method == "POST" && it.url.encodedPath.startsWith("/api/inbox/") }.map { it.url.encodedPath }
        assertEquals(listOf("/api/inbox/i-clip", "/api/inbox/i-puddle", "/api/inbox/i-photo", "/api/inbox/i-doc"), order)
        assertEquals("lib-shows", sent("POST", "/api/inbox/i-puddle").json()["library_id"]!!.jsonPrimitive.content)
    }

    @Test
    fun editChecksTheFormThenPutsTheServersVerdictUnderItsField() {
        val controller = controller()
        val row = controller.loaded().imports.first()
        controller.openEdit(row)
        val form = eventually { controller.state.first { it.editing != null } }.edit!!.form
        controller.onEditForm(form.copy(title = ""))
        controller.submitEdit()
        assertEquals(
            mapOf(MoveField.Title to "Enter the movie title."),
            eventually { controller.state.first { it.edit?.errors?.isNotEmpty() == true } }.edit!!.errors,
        )
        controller.onEditForm(form.copy(title = "Paper Boats", year = "2011"))
        answers["POST /api/inbox/i-clip"] = { error(400, "bad_request", "A movie needs a title.", "title") }
        controller.submitEdit()
        val failed = eventually { controller.state.first { it.edit?.errors?.isNotEmpty() == true && it.edit.busy.not() } }.edit!!
        assertEquals(mapOf(MoveField.Title to "A movie needs a title."), failed.errors)
        val body = sent("POST", "/api/inbox/i-clip").json()
        assertEquals("movie", body["category"]!!.jsonPrimitive.content)
        assertEquals("Paper Boats", body["title"]!!.jsonPrimitive.content)
        assertEquals("2011", body["year"]!!.jsonPrimitive.content)
        answers["POST /api/inbox/i-clip"] = { json(StashJson.encodeToString(ItemDetail.serializer(), s.lowConfidence)) }
        controller.onEditForm(form.copy(title = "Paper Boats", year = "2011"))
        controller.submitEdit()
        assertEquals("Moved to Movies.", eventually { controller.messages.first() })
        assertNull(eventually { controller.state.first { it.edit == null } }.edit)
    }

    @Test
    fun pauseAndResumeShowTheServersAnswerAtOnce() {
        val controller = controller()
        controller.loaded()
        answers["POST /api/downloads/d-lantern/pause"] = {
            json(StashJson.encodeToString(Download.serializer(), s.downloading.copy(state = DownloadState.Paused)))
        }
        controller.pauseOrResume(s.downloading)
        val paused = eventually { controller.state.first { s -> s.inProgress.first { it.id == "d-lantern" }.state == DownloadState.Paused } }
        assertTrue("d-lantern" !in paused.busyDownloads)
        answers["POST /api/downloads/d-trailer/resume"] = {
            json(StashJson.encodeToString(Download.serializer(), s.paused.copy(state = DownloadState.Downloading)))
        }
        controller.pauseOrResume(s.paused)
        eventually { controller.state.first { s -> s.inProgress.first { it.id == "d-trailer" }.state == DownloadState.Downloading } }
        sent("POST", "/api/downloads/d-trailer/resume")
    }

    @Test
    fun removingInProgressDeletesItsFilesButASortedOneKeepsThem() {
        val controller = controller()
        controller.loaded()
        answers["DELETE /api/downloads/d-trailer"] = { MockResponse.Builder().code(204).build() }
        answers["DELETE /api/downloads/d-harbor"] = { MockResponse.Builder().code(204).build() }
        controller.askRemove(s.paused)
        controller.confirmRemove()
        assertEquals("Download removed.", eventually { controller.messages.first() })
        assertEquals("true", sent("DELETE", "/api/downloads/d-trailer").url.queryParameter("delete_files"))
        eventually { controller.state.first { s -> s.inProgress.none { it.id == "d-trailer" } && s.removing == null } }
        controller.askRemove(s.sorted)
        controller.confirmRemove()
        assertEquals("Download removed.", eventually { controller.messages.first() })
        assertEquals("false", sent("DELETE", "/api/downloads/d-harbor").url.queryParameter("delete_files"))
    }

    @Test
    fun aFailedLinkThatCannotRestartIsAddedAgain() {
        val controller = controller()
        controller.loaded()
        answers["POST /api/downloads/d-iso/resume"] = { error(409, "conflict", "That download cannot be resumed.") }
        answers["POST /api/downloads"] = {
            json(StashJson.encodeToString(Download.serializer(), s.failed.copy(id = "d-iso-2", state = DownloadState.Downloading)))
        }
        answers["DELETE /api/downloads/d-iso"] = { MockResponse.Builder().code(204).build() }
        controller.retry(s.failed)
        val state = eventually { controller.state.first { s -> s.inProgress.any { it.id == "d-iso-2" } && s.finished.none { it.download.id == "d-iso" } } }
        assertTrue(state.inProgress.any { it.id == "d-iso-2" })
        assertEquals(s.failed.source, sent("POST", "/api/downloads").json()["url"]!!.jsonPrimitive.content)
        assertEquals("false", sent("DELETE", "/api/downloads/d-iso").url.queryParameter("delete_files"))
    }

    @Test
    fun aLinkFromTheClipboardFillsTheSheetAndIsAdded() {
        val controller = controller()
        controller.loaded()
        controller.openAdd("Watch this later: https://media.example.org/harbor-lights-trailer.mp4.")
        assertEquals(
            AddLinkState(text = "https://media.example.org/harbor-lights-trailer.mp4", fromClipboard = true),
            eventually { controller.state.first { it.add?.fromClipboard == true } }.add,
        )
        controller.openAdd("my password is hunter2")
        assertEquals(AddLinkState(), eventually { controller.state.first { it.add == AddLinkState() } }.add)
        controller.onAddText("example.org/clip.mp4")
        controller.submitAdd()
        assertEquals(
            "Add https:// to the start of the web address.",
            eventually { controller.state.first { it.add?.error != null } }.add?.error,
        )
        answers["POST /api/downloads"] = { json(StashJson.encodeToString(Download.serializer(), s.paused.copy(id = "d-new", name = "clip.mp4"))) }
        controller.onAddText("https://example.org/clip.mp4")
        controller.submitAdd()
        assertEquals("Added clip.mp4.", eventually { controller.messages.first() })
        eventually { controller.state.first { s -> s.add == null && s.inProgress.any { it.id == "d-new" } } }
    }

    @Test
    fun aTorrentOnAServerWithTorrentsOffSaysWhatStillWorks() {
        val controller = controller()
        controller.loaded()
        answers["POST /api/downloads"] = { error(503, "vpn_unavailable", "Torrents are not available.") }
        controller.openAdd(null)
        controller.onAddText("magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567")
        controller.submitAdd()
        val add = eventually { controller.state.first { it.add?.error != null } }.add!!
        assertEquals("Torrents are turned off on this server. Web links and uploads still work.", add.error)
    }

    @Test
    fun aMemberSeesOnlyTheClaimingLibraryForSomeoneElsesFile() {
        user.value = User("u-kid", "Tomas", Role.Member)
        val state = controller().loaded()
        val episode = state.imports.first { it.item.id == "i-puddle" }
        assertEquals(listOf("Shows"), episode.choices.map { it.name })
        assertTrue(!state.isOwner)
        assertTrue(!state.vpn.opensSettings)
    }

    @Test
    fun aLostConnectionShowsTheLineAndComingBackLoadsEverythingAgain() {
        val controller = controller()
        controller.loaded()
        connection.value = EventStream.Status.Waiting
        assertTrue(eventually { controller.state.first { it.connectionLost } }.connectionLost)
        val before = requests.count { it.url.encodedPath == "/api/downloads" }
        connection.value = EventStream.Status.Connected
        eventually { while (requests.count { it.url.encodedPath == "/api/downloads" } <= before) kotlinx.coroutines.delay(20) }
    }

    @Test
    fun anUnreadableVpnStatusSaysSo() {
        answers["GET /api/vpn/status"] = { error(500, "internal", "PopcornStash ran into a problem.") }
        val controller = controller()
        eventually { controller.refreshNow() }
        eventually { controller.state.first { it.vpn.label == "VPN status unavailable" } }
    }

    @Test
    fun downloadsThatCannotLoadSayWhyWithNothingElseToShow() {
        answers["GET /api/downloads"] = { error(500, "internal", "PopcornStash could not read its database.") }
        val controller = controller()
        eventually { controller.refreshNow() }
        eventually { controller.state.first { it.loadError == "PopcornStash could not read its database." } }
    }

    @Test
    fun uploadsFromThisPhoneShowAndCanBeStopped() {
        val controller = controller()
        controller.loaded()
        uploads.value = listOf(s.upload)
        assertEquals(listOf("batch-1"), eventually { controller.state.first { it.uploads.isNotEmpty() } }.uploads.map { it.id })
        controller.stopUpload("batch-1")
        assertEquals(listOf("batch-1"), stopped)
        assertEquals("Upload stopped.", eventually { controller.messages.first() })
    }

    @Test
    fun filesToImportWaitForTheLibrariesSoAVideoIsNeverOfferedOnlyOtherFiles() {
        answers["GET /api/libraries"] = { error(500, "internal", "PopcornStash could not read its database.") }
        val controller = controller()
        eventually { controller.refreshNow() }
        val state = eventually { controller.state.first { it.importsError != null } }
        assertTrue(state.imports.isEmpty())
        assertEquals("PopcornStash could not read its database.", state.importsError)
    }

    @Test
    fun aDownloadEventThatRemovesARowDropsItWithoutAMessage() {
        val controller = controller()
        controller.loaded()
        events.tryEmit(StashEvent.DownloadRemoved("d-harbor"))
        eventually { controller.state.first { s -> s.finished.none { it.download.id == "d-harbor" } } }
    }
}
