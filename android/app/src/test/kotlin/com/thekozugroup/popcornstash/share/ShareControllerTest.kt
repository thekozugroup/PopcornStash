package com.thekozugroup.popcornstash.share

import android.net.Uri
import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.EventStream
import com.thekozugroup.popcornstash.data.api.Role
import com.thekozugroup.popcornstash.data.api.User
import com.thekozugroup.popcornstash.data.api.baseHttpClient
import com.thekozugroup.popcornstash.data.session.Session
import com.thekozugroup.popcornstash.data.session.SessionState
import com.thekozugroup.popcornstash.feature.downloads.findLink
import com.thekozugroup.popcornstash.testing.respond
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import mockwebserver3.MockWebServer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/** The share sheet: a link after a confirm, files as the signed-in person only. */
@RunWith(RobolectricTestRunner::class)
class ShareControllerTest {
    private val server = MockWebServer()
    private lateinit var scope: CoroutineScope
    private lateinit var session: Session
    private val sessions = MutableStateFlow<SessionState>(SessionState.Restoring)
    private val enqueued = mutableListOf<UploadBatch>()

    @Before
    fun setUp() {
        server.start()
        scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        val http = baseHttpClient("test")
        val api = ApiClient(server.url("/"), http)
        session = Session(7, server.url("/"), User("u-ana", "Ana Ribeiro", Role.Owner), http, api, EventStream(api, http, scope), scope)
    }

    @After
    fun tearDown() {
        scope.cancel()
        server.close()
    }

    private fun controller(request: ShareRequest) = ShareController(
        request = request,
        sessions = sessions,
        describe = { uris -> uris.mapIndexed { i, uri -> SharedFile(uri.toString(), "PXL_2026101$i.jpg", 2_400_000L + i, "image/jpeg", 1_760_000_000) } },
        enqueue = { enqueued += it },
        scope = scope,
        now = { 1_791_993_600 },
        loadDestinationLibraries = { emptyList() },
    )

    private fun <T> eventually(block: suspend () -> T): T = runBlocking { withTimeout(5_000) { block() } }

    @Test
    fun nobodySignedInIsAskedToSignInFirst() {
        val controller = controller(ShareRequest.Link(findLink("https://example.org/a.mp4")!!))
        assertEquals(ShareSheetState.Checking, controller.state.value)
        sessions.value = SessionState.SignedOut(lastServer = null)
        assertEquals(ShareSheetState.SignedOut, controller.state.value)
        controller.addLink()
        assertEquals(0, server.requestCount)
    }

    @Test
    fun aLinkIsAddedOnlyAfterTheConfirm() {
        server.respond("""{"configured":false,"reachable":true,"state":"not_configured","allow_no_vpn":true,"torrents_off":false,"kill_switch":false}""")
        val controller = controller(ShareRequest.Link(findLink("magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Harbor")!!))
        sessions.value = SessionState.SignedIn(session)
        val shown = eventually { controller.state.first { it is ShareSheetState.Link && !it.viaVpn } } as ShareSheetState.Link
        assertEquals("Harbor", shown.link.name)
        assertEquals(1, server.requestCount)
        server.respond(
            """{"id":"d-9","kind":"torrent","name":"Harbor","state":"queued","progress":0,"done_bytes":0,"down_bps":0,"up_bps":0,
            "created_at":1,"item_ids":[],"destinations":[]}""",
        )
        controller.chooseLibrary("lib-movies")
        controller.addLink()
        val added = eventually { controller.state.first { (it as? ShareSheetState.Link)?.addedName != null } } as ShareSheetState.Link
        assertEquals("Harbor", added.addedName)
        server.takeRequest()
        val post = server.takeRequest()
        assertEquals("/api/downloads", post.url.encodedPath)
        val sent = post.body!!.utf8()
        assertTrue(sent.contains("\"library_id\":\"lib-movies\""))
        assertTrue(sent.contains("magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567"))
    }

    @Test
    fun aRefusedLinkSaysWhyAndCanBeTriedAgain() {
        server.respond("""{"error":{"code":"internal","message":"x"}}""", code = 500)
        val controller = controller(ShareRequest.Link(findLink("https://example.org/a.mp4")!!))
        sessions.value = SessionState.SignedIn(session)
        eventually { controller.state.first { it is ShareSheetState.Link } }
        // The VPN status question took the first answer before the link is added.
        eventually { while (server.requestCount < 1) kotlinx.coroutines.delay(10) }
        server.respond("""{"error":{"code":"bad_request","message":"That link cannot be downloaded."}}""", code = 400)
        controller.addLink()
        val failed = eventually { controller.state.first { (it as? ShareSheetState.Link)?.error != null } } as ShareSheetState.Link
        assertEquals("That link cannot be downloaded.", failed.error)
        assertFalse(failed.busy)
    }

    @Test
    fun filesAreNamedThenUploadedAsTheSignedInPerson() {
        val uris = listOf(Uri.parse("content://media/external/images/1"), Uri.parse("content://media/external/images/2"))
        val controller = controller(ShareRequest.Files(uris, refused = 1))
        sessions.value = SessionState.SignedIn(session)
        val files = eventually { controller.state.first { it is ShareSheetState.Files } } as ShareSheetState.Files
        assertEquals("Ana Ribeiro", files.userName)
        assertEquals(1, files.refused)
        assertEquals(2, files.files.size)
        controller.chooseLibrary("lib-photos")
        assertTrue(controller.upload())
        val batch = enqueued.single()
        assertEquals("u-ana", batch.userId)
        assertTrue(batch.files.all { it.libraryId == "lib-photos" })
        assertEquals(server.url("/").toString(), batch.server)
        assertEquals(uris.map { it.toString() }, batch.files.map { it.uri })
        assertEquals(1_791_993_600, batch.createdAt)
        // A second tap does not start a second job.
        assertFalse(controller.upload())
        assertEquals(1, enqueued.size)
    }

    @Test
    fun signingOutWhileTheSheetIsOpenTakesTheChoiceAway() {
        val controller = controller(ShareRequest.Files(listOf(Uri.parse("content://media/1")), refused = 0))
        sessions.value = SessionState.SignedIn(session)
        eventually { controller.state.first { it is ShareSheetState.Files } }
        sessions.value = SessionState.SignedOut(lastServer = null, generation = 1)
        assertEquals(ShareSheetState.SignedOut, controller.state.value)
        assertFalse(controller.upload())
        assertTrue(enqueued.isEmpty())
    }

    @Test
    fun nothingUsableSaysSo() {
        val controller = controller(ShareRequest.Unusable(refused = 2))
        sessions.value = SessionState.SignedIn(session)
        assertEquals(ShareSheetState.Unusable(2), controller.state.value)
    }
}
