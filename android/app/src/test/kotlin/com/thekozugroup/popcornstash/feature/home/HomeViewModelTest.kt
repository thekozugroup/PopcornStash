package com.thekozugroup.popcornstash.feature.home

import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.Home
import com.thekozugroup.popcornstash.data.api.HomeLibrary
import com.thekozugroup.popcornstash.data.api.HomeShow
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
    fun aSeriesShelfHasATileForEveryShowItsHeadingCounts() {
        val entry = SampleMedia.home.libraries[1]
        val tiles = shelfTiles(entry)
        // "6 shows" in the heading, six tiles, though the newest episodes belong to three of them.
        assertEquals(entry.library.showCount, tiles.size.toLong())
        assertEquals(
            listOf("The Lantern Keepers", "Tidepool Detectives", "Garden Hour", "Night Bakery", "Small Planet", "Puddle Club").map { "show:$it" },
            tiles.map { it.key },
        )
        val shows = tiles.map { it as ShelfTile.Show }
        assertEquals("e-l101", shows[0].posterId)
        assertEquals("2 new episodes", format.showTileLine(shows[0]))
        assertEquals("New: S03E01", format.showTileLine(shows[2]))
        // A show with nothing new still has its tile, with how many episodes it has.
        assertEquals("8 episodes", format.showTileLine(shows[3]))
        assertEquals("e-n108", shows[3].latest.id)
    }

    @Test
    fun threeShowsWhoseNewestEpisodesAreFromTwoStillMakeThreeTiles() {
        // The server's case: 3 shows and 19 episodes, the 12 newest from two of the shows.
        val a = (1..10).map { SampleMedia.episode("a$it", "Alpha Line", 1, it, null, added = SampleMedia.NOW - it) }
        val b = (1..6).map { SampleMedia.episode("b$it", "Beta Bakes", 1, it, null, added = SampleMedia.NOW - 20 - it) }
        val c = (1..3).map { SampleMedia.episode("c$it", "Gamma Garden", 1, it, null, added = SampleMedia.NOW - 86_400L * 60 - it) }
        val recent = (a + b + c).sortedByDescending { it.addedAt }.take(12)
        val library = SampleMedia.shows.copy(itemCount = 19, showCount = 3)
        val shows = listOf(
            HomeShow("Alpha Line", "a1", itemCount = 10, newCount = 10, latest = a[0], lastAddedAt = a[0].addedAt),
            HomeShow("Beta Bakes", "b1", itemCount = 6, newCount = 6, latest = b[0], lastAddedAt = b[0].addedAt),
            HomeShow("Gamma Garden", "c1", itemCount = 3, newCount = 0, latest = c[0], lastAddedAt = c[0].addedAt),
        )
        val tiles = shelfTiles(HomeLibrary(library, recent, shows = shows)).map { it as ShelfTile.Show }
        assertEquals(listOf("Alpha Line", "Beta Bakes", "Gamma Garden"), tiles.map { it.name })
        assertEquals(19L, tiles.sumOf { it.episodeCount })
        assertEquals(listOf("10 new episodes", "6 new episodes", "3 episodes"), tiles.map { format.showTileLine(it) })
        // An older server sends no shows: the shelf groups the newest episodes, as before.
        val older = shelfTiles(HomeLibrary(library, recent, showPosters = mapOf("Alpha Line" to "a1"))).map { it as ShelfTile.Show }
        assertEquals(listOf("Alpha Line", "Beta Bakes"), older.map { it.name })
        assertEquals("a1", older[0].posterId)
        assertEquals("2 new episodes", format.showTileLine(older[1]))
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
