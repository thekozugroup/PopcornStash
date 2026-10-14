package com.thekozugroup.popcornstash.feature.home

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.SemanticsNodeInteraction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollToIndex
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.thekozugroup.popcornstash.TabRequests
import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.Home
import com.thekozugroup.popcornstash.data.api.ItemDetail
import com.thekozugroup.popcornstash.data.api.ItemPage
import com.thekozugroup.popcornstash.data.api.Library
import com.thekozugroup.popcornstash.data.api.LibraryList
import com.thekozugroup.popcornstash.data.api.PhotoTimeline
import com.thekozugroup.popcornstash.data.api.Role
import com.thekozugroup.popcornstash.data.api.ShowDetail
import com.thekozugroup.popcornstash.data.api.ShowSummary
import com.thekozugroup.popcornstash.data.api.StashEvent
import com.thekozugroup.popcornstash.data.api.User
import com.thekozugroup.popcornstash.data.api.baseHttpClient
import com.thekozugroup.popcornstash.data.repo.StashData
import com.thekozugroup.popcornstash.feature.library.FileFetcher
import com.thekozugroup.popcornstash.feature.library.LibraryServices
import com.thekozugroup.popcornstash.navigation.AppShell
import com.thekozugroup.popcornstash.navigation.Tab
import com.thekozugroup.popcornstash.navigation.ViewModelStores
import com.thekozugroup.popcornstash.testing.SampleMedia
import com.thekozugroup.popcornstash.testing.json
import com.thekozugroup.popcornstash.testing.jsonList
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.emptyFlow
import mockwebserver3.Dispatcher
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import mockwebserver3.RecordedRequest
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import java.util.concurrent.CopyOnWriteArrayList

/**
 * The Home tab against a server: its stack of pages (a library, a show, an item), Back, tapping
 * Home again, the photo viewer hiding the toolbar, a library that goes away, and the hand-off
 * to the player.
 */
@RunWith(AndroidJUnit4::class)
@Config(qualifiers = "w411dp-h891dp-xxhdpi")
class HomeTabTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    @get:Rule
    val folder = TemporaryFolder()

    private val server = MockWebServer()
    private val events = MutableSharedFlow<StashEvent>(extraBufferCapacity = 16)
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val played = CopyOnWriteArrayList<Pair<String, Boolean>>()
    private val managed = CopyOnWriteArrayList<String?>()
    private val pages = ViewModelStores()

    @Volatile
    private var visible = listOf(SampleMedia.movies, SampleMedia.shows, SampleMedia.homeVideos, SampleMedia.photos)

    private val owner = User("u-owner", "Ana Ribeiro", Role.Owner)

    @Before
    fun setUp() {
        server.dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse = answer(request)
        }
        server.start()
    }

    @After
    fun tearDown() {
        // As at the end of a session: the pages' view models go, with their work.
        compose.runOnUiThread { pages.clearAll() }
        scope.cancel()
        server.close()
    }

    private fun ok(body: String) = MockResponse.Builder().code(200).addHeader("Content-Type", "application/json").body(body).build()

    private fun notFound() = MockResponse.Builder().code(404).addHeader("Content-Type", "application/json")
        .body("""{"error":{"code":"not_found","message":"That library no longer exists."}}""").build()

    private fun answer(request: RecordedRequest): MockResponse {
        val path = request.url.encodedPath
        val libraries = visible
        fun library(id: String) = libraries.firstOrNull { it.id == id }
        return when {
            path == "/api/home" -> ok(json(Home.serializer(), SampleMedia.home.copy(libraries = SampleMedia.home.libraries.filter { library(it.library.id) != null })))
            path == "/api/libraries" -> ok(json(LibraryList.serializer(), LibraryList(libraries)))
            path.startsWith("/api/libraries/") -> {
                val id = path.removePrefix("/api/libraries/").substringBefore('/')
                val found = library(id) ?: return notFound()
                when {
                    path.endsWith("/shows") -> ok(jsonList(ShowSummary.serializer(), SampleMedia.showSummaries))
                    path.contains("/shows/") -> ok(json(ShowDetail.serializer(), SampleMedia.lanternShow))
                    path.endsWith("/timeline") -> ok(json(PhotoTimeline.serializer(), PhotoTimeline(SampleMedia.timeline)))
                    else -> ok(json(Library.serializer(), found))
                }
            }
            path == "/api/items" -> ok(json(ItemPage.serializer(), ItemPage(SampleMedia.recentMovies)))
            path.startsWith("/api/items/") && !path.endsWith("/thumb") -> ok(json(ItemDetail.serializer(), SampleMedia.paperBoatsDetail))
            else -> MockResponse.Builder().code(404).build()
        }
    }

    private fun show(requests: TabRequests? = null) {
        val api = ApiClient(server.url("/"), baseHttpClient("t"))
        val services = LibraryServices(
            api = api,
            data = StashData(api, events, scope),
            events = events,
            reconnects = emptyFlow(),
            files = FileFetcher(baseHttpClient("t")),
            openedDir = folder.newFolder("opened"),
            pages = pages,
        )
        compose.setContent {
            PopcornTheme(darkTheme = false, reduceMotion = true) {
                AppShell { page ->
                    if (page.tab == Tab.Home) {
                        HomeTab(
                            page,
                            services,
                            owner,
                            onPlay = { id, fromStart -> played += id to fromStart },
                            onManageLibrary = { managed += it },
                            open = requests?.home,
                            onOpened = { requests?.homeOpened() },
                        )
                    }
                }
            }
        }
    }

    private fun waitFor(matcher: androidx.compose.ui.test.SemanticsMatcher): SemanticsNodeInteraction {
        compose.waitUntil(5_000) { compose.onAllNodes(matcher).fetchSemanticsNodes().isNotEmpty() }
        return compose.onAllNodes(matcher)[0]
    }

    private fun gone(matcher: androidx.compose.ui.test.SemanticsMatcher) =
        compose.waitUntil(5_000) { compose.onAllNodes(matcher).fetchSemanticsNodes().isEmpty() }

    private fun back() = compose.runOnUiThread { compose.activity.onBackPressedDispatcher.onBackPressed() }

    private fun openMovies() {
        waitFor(hasText("Movies")).performClick()
        waitFor(hasText("A Quiet Orchard"))
        compose.onNodeWithTag("library grid").assertExists()
    }

    @Test
    fun aLibraryOpensOnTopOfHomeAndBackCloses() {
        show()
        openMovies()
        compose.onNodeWithText("Continue watching").assertDoesNotExist()
        back()
        waitFor(hasText("Continue watching"))
        gone(hasTestTag("library grid"))
    }

    @Test
    fun aLibraryAnotherTabAsksForOpensOnTopOfHomeOnce() {
        val requests = TabRequests()
        show(requests)
        waitFor(hasText("Continue watching"))
        val movies = SampleMedia.movies
        compose.runOnUiThread { requests.openHome(HomeRoute.Library(movies.id, movies.name, movies.icon)) }
        waitFor(hasTestTag("library grid"))
        waitFor(hasText("A Quiet Orchard"))
        compose.runOnIdle { assertEquals(null, requests.home) }
        back()
        waitFor(hasText("Continue watching"))
        gone(hasTestTag("library grid"))
    }

    @Test
    fun tappingHomeAgainClosesThePagesAboveHome() {
        show()
        openMovies()
        waitFor(hasText("Paper Boats")).performClick()
        waitFor(hasTestTag("item page"))
        compose.onNodeWithTag("tab Home").performClick()
        waitFor(hasText("Continue watching"))
        gone(hasTestTag("item page"))
    }

    @Test
    fun aTabKeepsItsPagesWhileAnotherTabShows() {
        show()
        openMovies()
        compose.onNodeWithTag("tab Settings").performClick()
        gone(hasTestTag("library grid"))
        compose.onNodeWithTag("tab Home").performClick()
        waitFor(hasTestTag("library grid"))
        waitFor(hasText("A Quiet Orchard"))
    }

    @Test
    fun aShowOpensFromItsShelfAndPlaysWhatComesNext() {
        show()
        waitFor(hasText("The Lantern Keepers"))
        // Shows to the top of the page, clear of the floating toolbar (top, continue watching, Movies, Shows).
        compose.onNodeWithTag("list Home").performScrollToIndex(3)
        compose.onNodeWithText("Tidepool Detectives").performClick()
        waitFor(hasText("Resume S01E02")).performClick()
        compose.waitUntil(5_000) { played.isNotEmpty() }
        assertEquals("e-l102" to false, played.single())
    }

    @Test
    fun continueWatchingResumesAtOnceOrStartsOver() {
        show()
        waitFor(hasContentDescription("Resume The Lantern Keepers S01E02, 10 minutes left")).performClick()
        compose.waitUntil(5_000) { played.size == 1 }
        compose.onNodeWithContentDescription("More for The Lantern Keepers").performClick()
        waitFor(hasText("Start from the beginning")).performClick()
        compose.waitUntil(5_000) { played.size == 2 }
        assertEquals(listOf("e-l102" to false, "e-l102" to true), played.toList())
    }

    @Test
    fun thePhotoViewerTakesTheWholeScreenAndBackClosesIt() {
        show()
        waitFor(hasText("Movies"))
        compose.onNodeWithTag("list Home").performScrollToIndex(5)
        waitFor(hasContentDescription("Photo from", substring = true)).performClick()
        waitFor(hasTestTag("photo viewer"))
        gone(hasTestTag("toolbar"))
        back()
        gone(hasTestTag("photo viewer"))
        waitFor(hasTestTag("toolbar"))
    }

    @Test
    fun aLibraryThatGoesAwayClosesWithItsPagesAndSaysSo() {
        show()
        openMovies()
        waitFor(hasText("Paper Boats")).performClick()
        waitFor(hasTestTag("item page"))
        visible = visible.filterNot { it.id == SampleMedia.movies.id }
        events.tryEmit(StashEvent.LibrariesChanged)
        waitFor(hasText("Movies is not available any more."))
        waitFor(hasText("Continue watching"))
        gone(hasTestTag("item page"))
    }

    @Test
    fun anOwnerEditsALibraryFromItsPage() {
        show()
        openMovies()
        compose.onNodeWithContentDescription("Edit library").performClick()
        assertEquals(listOf<String?>(SampleMedia.movies.id), managed.toList())
    }
}
