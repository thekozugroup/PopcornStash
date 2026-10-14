package com.thekozugroup.popcornstash.feature.home

import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.Home
import com.thekozugroup.popcornstash.data.api.HomeLibrary
import com.thekozugroup.popcornstash.data.api.StashEvent
import com.thekozugroup.popcornstash.data.api.baseHttpClient
import com.thekozugroup.popcornstash.data.repo.Loadable
import com.thekozugroup.popcornstash.data.repo.StashData
import com.thekozugroup.popcornstash.feature.library.MediaFormat
import com.thekozugroup.popcornstash.testing.MainDispatcherRule
import com.thekozugroup.popcornstash.testing.keep
import com.thekozugroup.popcornstash.testing.SampleMedia
import com.thekozugroup.popcornstash.testing.awaitUntil
import com.thekozugroup.popcornstash.testing.json
import com.thekozugroup.popcornstash.testing.next
import com.thekozugroup.popcornstash.testing.respond
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.runTest
import mockwebserver3.MockWebServer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import java.time.ZoneId
import java.util.Locale

/** Home: loading through the session's data, reloading after a gap, and the shelves' tiles. */
@OptIn(ExperimentalCoroutinesApi::class)
class HomeViewModelTest {
    @get:Rule
    val main = MainDispatcherRule()

    private val server = MockWebServer()
    private val events = MutableSharedFlow<StashEvent>(extraBufferCapacity = 16)
    private val reconnects = MutableSharedFlow<Unit>(extraBufferCapacity = 4)
    private lateinit var api: ApiClient
    private val format = MediaFormat(Locale.US, ZoneId.of("UTC"))

    @Before
    fun setUp() {
        server.start()
        api = ApiClient(server.url("/"), baseHttpClient("t"))
    }

    @After
    fun tearDown() = server.close()

    @Test
    fun loadsHomeWhenNothingIsLoadedAndAgainAfterAGap() = runTest {
        val data = StashData(api, events, CoroutineScope(UnconfinedTestDispatcher(testScheduler)))
        server.respond(json(Home.serializer(), SampleMedia.home))
        val model = main.keep { HomeViewModel(data, reconnects, MutableStateFlow(false)) }
        model.home.awaitUntil { it is Loadable.Ready }
        assertEquals("/api/home", server.next().target)

        server.respond(json(Home.serializer(), SampleMedia.home.copy(otherFilesCount = 13)))
        reconnects.emit(Unit)
        model.home.awaitUntil { it.valueOrNull?.otherFilesCount == 13L }
        assertEquals("/api/home", server.next().target)
    }

    @Test
    fun theConnectionLineFollowsTheStream() = runTest {
        val data = StashData(api, events, CoroutineScope(UnconfinedTestDispatcher(testScheduler)))
        server.respond(json(Home.serializer(), SampleMedia.home))
        val lost = MutableStateFlow(false)
        val model = main.keep { HomeViewModel(data, reconnects, lost) }
        lost.value = true
        assertTrue(model.connectionLost.awaitUntil { it })
    }

    @Test
    fun aSeriesShelfShowsEachShowOnceWithItsPoster() {
        val tiles = shelfTiles(SampleMedia.home.libraries[1])
        assertEquals(listOf("show:The Lantern Keepers", "show:Tidepool Detectives", "show:Garden Hour"), tiles.map { it.key })
        val lantern = tiles[0] as ShelfTile.Show
        assertEquals("e-l101", lantern.posterId)
        assertEquals("2 new episodes", format.showTileLine(lantern))
        assertEquals("New: S03E01", format.showTileLine(tiles[2] as ShelfTile.Show))
    }

    @Test
    fun otherShelvesShowTheirItemsAsTheyCome() {
        val tiles = shelfTiles(HomeLibrary(SampleMedia.movies, SampleMedia.recentMovies))
        assertEquals(SampleMedia.recentMovies.map { it.id }, tiles.map { it.key })
    }

    @Test
    fun continueWatchingNamesTheShowForAnEpisode() {
        val episode = SampleMedia.lantern[1]
        assertEquals("The Lantern Keepers", format.resumeTitle(episode))
        assertEquals("S01E02 · 10 min left", format.resumeLine(episode))
        assertEquals("Resume The Lantern Keepers S01E02, 10 minutes left", format.resumeDescription(episode))
        assertEquals("Paper Boats", format.resumeTitle(SampleMedia.paperBoats))
        assertEquals("1 h 4 min left", format.resumeLine(SampleMedia.paperBoats))
        assertEquals("Resume Paper Boats, 1 hour 4 minutes left", format.resumeDescription(SampleMedia.paperBoats))
    }
}
