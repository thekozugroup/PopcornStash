package com.thekozugroup.popcornstash.feature.settings

import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.StashEvent
import com.thekozugroup.popcornstash.data.api.baseHttpClient
import com.thekozugroup.popcornstash.data.repo.Loadable
import com.thekozugroup.popcornstash.data.repo.StashData
import com.thekozugroup.popcornstash.testing.bodyText
import com.thekozugroup.popcornstash.testing.path
import com.thekozugroup.popcornstash.testing.withinRealSeconds
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.Json
import mockwebserver3.MockWebServer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import com.thekozugroup.popcornstash.feature.settings.SettingsFixtures as F

/** The owner's library list: order, delete, people and the folders that are not a library. */
@OptIn(ExperimentalCoroutinesApi::class)
class LibrarySettingsControllerTest {
    private val server = MockWebServer()
    private val events = MutableSharedFlow<StashEvent>(extraBufferCapacity = 16)
    private lateinit var routes: Routes
    private lateinit var api: ApiClient

    @Before
    fun setUp() {
        server.start()
        api = ApiClient(server.url("/"), baseHttpClient("t"))
    }

    @After
    fun tearDown() = server.close()

    private fun serve(routes: Routes) {
        this.routes = routes
        server.dispatcher = routes
    }

    private fun TestScope.controller(): LibrarySettingsController {
        val scope = CoroutineScope(UnconfinedTestDispatcher(testScheduler))
        return LibrarySettingsController(api, StashData(api, events, scope, settleMillis = 0), scope)
    }

    private suspend fun LibrarySettingsController.until(check: (LibrarySettingsUi) -> Boolean): LibrarySettingsUi =
        withinRealSeconds { state.first(check) }

    private fun LibrarySettingsUi.names() = shown?.map { it.name }

    @Test
    fun refreshLoadsTheLibrariesThePeopleAndTheFolders() = runTest {
        serve(F.ownerServer())
        val settings = controller()
        assertNull(settings.state.value.shown)
        settings.refresh()
        val state = settings.until { it.libraries is Loadable.Ready && it.people != null && it.folders.isNotEmpty() }
        assertEquals(listOf("Movies", "Shows", "Home Videos", "Photos"), state.names())
        assertEquals(3, state.people!!.size)
        assertEquals(listOf("Family 2019", "Grandma's scans", "Cartoons"), state.folders.map { it.name })
        assertTrue(state.canMakeMore)
    }

    @Test
    fun aFailedLoadSaysWhy() = runTest {
        serve(
            F.ownerServer().only(
                "GET",
                "/api/libraries",
                """{"error":{"code":"internal","message":"PopcornStash could not read its database."}}""",
                500,
            ),
        )
        val settings = controller()
        settings.refresh()
        val state = settings.until { it.libraries is Loadable.Failed }
        assertEquals("PopcornStash could not read its database.", (state.libraries as Loadable.Failed).message)
        assertNull(state.shown)
    }

    @Test
    fun movingALibraryShowsAtOnceAndSavesTheWholeOrder() = runTest {
        serve(F.ownerServer().on("PUT", "/api/libraries/order", F.list(F.SHOWS, F.MOVIES, F.HOME_VIDEOS, F.PHOTOS)))
        val settings = controller()
        settings.refresh()
        settings.until { it.libraries is Loadable.Ready }
        routes.only("GET", "/api/libraries", F.list(F.SHOWS, F.MOVIES, F.HOME_VIDEOS, F.PHOTOS))
        settings.move(F.SHOWS_ID, -1)
        assertEquals(listOf("Shows", "Movies", "Home Videos", "Photos"), settings.state.value.names())
        val request = routes.nextWrite()
        assertEquals("PUT /api/libraries/order", "${request.method} ${request.path}")
        assertEquals(
            Json.parseToJsonElement("""{"ids":["${F.SHOWS_ID}","${F.MOVIES_ID}","${F.HOME_VIDEOS_ID}","${F.PHOTOS_ID}"]}"""),
            Json.parseToJsonElement(request.bodyText),
        )
        val state = settings.until { !it.moving }
        assertNull(state.pendingOrder)
        assertEquals(listOf("Shows", "Movies", "Home Videos", "Photos"), state.names())
    }

    @Test
    fun aMoveThatFailsPutsTheRowBackAndSaysWhy() = runTest {
        serve(
            F.ownerServer().on(
                "PUT",
                "/api/libraries/order",
                """{"error":{"code":"bad_request","message":"List every library exactly once.","field":"ids"}}""",
                400,
            ),
        )
        val settings = controller()
        settings.refresh()
        settings.until { it.libraries is Loadable.Ready }
        settings.move(F.PHOTOS_ID, -1)
        assertEquals("The order was not saved. List every library exactly once.", withinRealSeconds { settings.notices.first() })
        val state = settings.until { !it.moving }
        assertEquals(listOf("Movies", "Shows", "Home Videos", "Photos"), state.names())
    }

    @Test
    fun theEndsOfTheListDoNotMoveFurther() = runTest {
        serve(F.ownerServer())
        val settings = controller()
        settings.refresh()
        settings.until { it.libraries is Loadable.Ready }
        settings.move(F.MOVIES_ID, -1)
        settings.move(F.PHOTOS_ID, 1)
        assertFalse(settings.state.value.moving)
        assertTrue(routes.drain().none { it.startsWith("PUT") })
    }

    @Test
    fun deletingSaysTheFilesStayAndTheFolderComesBack() = runTest {
        serve(F.ownerServer(F.FIVE).on("DELETE", "/api/libraries/${F.KIDS_ID}", code = 204))
        val settings = controller()
        settings.refresh()
        val before = settings.until { it.libraries is Loadable.Ready && it.folders.isNotEmpty() }
        val kids = before.shown!!.first { it.id == F.KIDS_ID }
        routes.only("GET", "/api/libraries", F.FOUR)
        routes.only(
            "GET",
            "/api/libraries/folders",
            """{"folders":[{"name":"Kids","item_count":0,"suggested":{"content_type":"video","structure":"one_offs"}}]}""",
        )
        settings.delete(kids)
        assertEquals("DELETE /api/libraries/${F.KIDS_ID}", routes.nextWrite().let { "${it.method} ${it.path}" })
        assertEquals("Kids is no longer in the app. Its files stay in Kids/.", withinRealSeconds { settings.notices.first() })
        val after = settings.until { it.deletingId == null && it.folders.map { f -> f.name } == listOf("Kids") }
        assertEquals(4, after.shown!!.size)
    }

    @Test
    fun aLibraryMadeElsewhereReloadsTheFolders() = runTest {
        serve(F.ownerServer())
        val settings = controller()
        settings.refresh()
        settings.until { it.libraries is Loadable.Ready && it.folders.size == 3 }
        routes.only("GET", "/api/libraries", F.list(F.MOVIES, F.SHOWS, F.HOME_VIDEOS, F.PHOTOS, F.library(F.KIDS_ID, "Cartoons", "tv", "video", "episodes", emptyList(), "series", 4)))
        routes.only("GET", "/api/libraries/folders", """{"folders":[]}""")
        events.emit(StashEvent.LibrariesChanged)
        val state = settings.until { it.shown?.size == 5 && it.folders.isEmpty() }
        assertEquals("Cartoons", state.shown!!.last().name)
    }

    @Test
    fun twentyFourLibrariesAreTheMost() {
        val many = F.libraries().let { five -> List(24) { five[it % 5].copy(id = "lib-$it") } }
        assertFalse(LibrarySettingsUi(libraries = Loadable.Ready(many)).canMakeMore)
        assertTrue(LibrarySettingsUi(libraries = Loadable.Ready(many.drop(1))).canMakeMore)
    }
    @Test
    fun folderRemovalUsesAnExplicitPostAndNeverFallsBackToDeletingOnlyTheDefinition() = runTest {
        serve(F.ownerServer(F.FIVE).on("POST", "/api/libraries/${F.KIDS_ID}/trash", code = 404))
        val settings = controller()
        settings.refresh()
        val kids = settings.until { it.libraries is Loadable.Ready }.shown!!.first { it.id == F.KIDS_ID }
        settings.delete(kids, removeFolder = true)
        val request = routes.nextWrite()
        assertEquals("POST /api/libraries/${F.KIDS_ID}/trash", "${request.method} ${request.path}")
        assertEquals(Json.parseToJsonElement("""{"confirm_folder":"Kids"}"""), Json.parseToJsonElement(request.bodyText))
        settings.until { it.deletingId == null }
        assertTrue(settings.state.value.shown!!.any { it.id == F.KIDS_ID })
    }

}
