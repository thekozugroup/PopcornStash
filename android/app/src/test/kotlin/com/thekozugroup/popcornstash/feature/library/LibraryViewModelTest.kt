package com.thekozugroup.popcornstash.feature.library

import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.Item
import com.thekozugroup.popcornstash.data.api.ItemPage
import com.thekozugroup.popcornstash.data.api.Layout
import com.thekozugroup.popcornstash.data.api.Library
import com.thekozugroup.popcornstash.data.api.PhotoMonth
import com.thekozugroup.popcornstash.data.api.PhotoTimeline
import com.thekozugroup.popcornstash.data.api.ShowSummary
import com.thekozugroup.popcornstash.data.api.StashEvent
import com.thekozugroup.popcornstash.data.api.baseHttpClient
import com.thekozugroup.popcornstash.feature.library.LibraryViewModel.Content
import com.thekozugroup.popcornstash.testing.MainDispatcherRule
import com.thekozugroup.popcornstash.testing.keep
import com.thekozugroup.popcornstash.testing.SampleMedia
import com.thekozugroup.popcornstash.testing.awaitUntil
import com.thekozugroup.popcornstash.testing.json
import com.thekozugroup.popcornstash.testing.jsonList
import com.thekozugroup.popcornstash.testing.next
import com.thekozugroup.popcornstash.testing.nextOrNull
import com.thekozugroup.popcornstash.testing.respond
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.test.runTest
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import java.util.concurrent.TimeUnit

/** A library page: the library, then its content in its layout, kept current by live events. */
class LibraryViewModelTest {
    @get:Rule
    val main = MainDispatcherRule()

    private val server = MockWebServer()
    private val events = MutableSharedFlow<StashEvent>(extraBufferCapacity = 16)
    private val reconnects = MutableSharedFlow<Unit>(extraBufferCapacity = 4)
    private lateinit var api: ApiClient

    @Before
    fun setUp() {
        server.start()
        api = ApiClient(server.url("/"), baseHttpClient("t"))
    }

    @After
    fun tearDown() = server.close()

    private fun model(library: Library, settle: Long = 0) =
        main.keep { LibraryViewModel(api, events, reconnects, library.id, seed = library, settleMillis = settle) }

    private fun respondLibrary(library: Library) = server.respond(json(Library.serializer(), library))

    private fun respondItems(items: List<Item>, next: String? = null) = server.respond(json(ItemPage.serializer(), ItemPage(items, next)))

    @Test
    fun postersLoadTheNewestItemsOfTheLibrary() = runTest {
        respondLibrary(SampleMedia.movies)
        respondItems(SampleMedia.recentMovies, next = "c2")
        val model = model(SampleMedia.movies)
        val ui = model.ui.awaitUntil { it.content is Content.Items }
        assertEquals("/api/libraries/lib-movies", server.next().target)
        assertEquals("/api/items?library_id=lib-movies&sort=added&order=desc&limit=60", server.next().target)
        val content = ui.content as Content.Items
        assertEquals(ItemSort.Added, content.sort)
        assertEquals(6, content.page.items.size)
        assertTrue(content.page.hasMore)
    }

    @Test
    fun framesSortByTheDayTheyWereFilmed() = runTest {
        respondLibrary(SampleMedia.homeVideos)
        respondItems(listOf(SampleMedia.beachDay))
        val model = model(SampleMedia.homeVideos)
        model.ui.awaitUntil { it.content is Content.Items }
        server.next()
        assertEquals("/api/items?library_id=lib-home-videos&sort=taken&order=desc&limit=60", server.next().target)
        assertEquals(listOf(ItemSort.Taken, ItemSort.Added, ItemSort.Title, ItemSort.Size), ItemSort.optionsFor(Layout.Frames))
    }

    @Test
    fun aSeriesLibraryListsItsShows() = runTest {
        respondLibrary(SampleMedia.shows)
        server.respond(jsonList(ShowSummary.serializer(), SampleMedia.showSummaries))
        val model = model(SampleMedia.shows)
        val ui = model.ui.awaitUntil { it.content is Content.Shows }
        server.next()
        assertEquals("/api/libraries/lib-shows/shows", server.next().target)
        assertEquals(5, (ui.content as Content.Shows).shows.size)
    }

    @Test
    fun aPhotosLibraryReadsItsTimelineAndJoinsAMonthSplitAcrossPages() = runTest {
        respondLibrary(SampleMedia.photos)
        val september = SampleMedia.timeline[0]
        server.respond(json(PhotoTimeline.serializer(), PhotoTimeline(listOf(september.copy(items = september.items.take(4))), nextCursor = "p2")))
        val model = model(SampleMedia.photos)
        model.ui.awaitUntil { it.content is Content.Timeline }
        server.next()
        assertEquals("/api/libraries/lib-photos/timeline?limit=200", server.next().target)

        server.respond(
            json(
                PhotoTimeline.serializer(),
                PhotoTimeline(listOf(september.copy(items = september.items.drop(4)), SampleMedia.timeline[1]), nextCursor = null),
            ),
        )
        model.loadMore()
        val ui = model.ui.awaitUntil { (it.content as? Content.Timeline)?.page?.hasMore == false }
        assertEquals("/api/libraries/lib-photos/timeline?cursor=p2&limit=200", server.next().target)
        val timeline = ui.content as Content.Timeline
        assertEquals(listOf("2026-09", "2026-08"), timeline.page.items.map { it.month })
        assertEquals(7, timeline.page.items[0].items.size)
        assertEquals(16, timeline.photos.size)
    }

    @Test
    fun loadMoreAppendsTheNextPageWithTheCursor() = runTest {
        respondLibrary(SampleMedia.movies)
        respondItems(SampleMedia.recentMovies.take(3), next = "c2")
        val model = model(SampleMedia.movies)
        model.ui.awaitUntil { it.content is Content.Items }
        server.next()
        server.next()
        respondItems(SampleMedia.recentMovies.drop(3))
        model.loadMore()
        val ui = model.ui.awaitUntil { (it.content as? Content.Items)?.page?.items?.size == 6 }
        assertEquals("/api/items?library_id=lib-movies&sort=added&order=desc&cursor=c2&limit=60", server.next().target)
        assertFalse((ui.content as Content.Items).page.hasMore)
    }

    @Test
    fun aNextPageAskedForWhileTheFirstLoadsComesOnceItHasLoaded() = runTest {
        respondLibrary(SampleMedia.movies)
        // The first page is slow, so the person reaches the end of the list before it comes.
        server.enqueue(
            MockResponse.Builder().code(200).addHeader("Content-Type", "application/json")
                .body(json(ItemPage.serializer(), ItemPage(SampleMedia.recentMovies.take(3), "c2")))
                .headersDelay(300, TimeUnit.MILLISECONDS)
                .build(),
        )
        respondItems(SampleMedia.recentMovies.drop(3))
        val model = model(SampleMedia.movies)
        model.loadMore()
        val ui = model.ui.awaitUntil { (it.content as? Content.Items)?.page?.items?.size == 6 }
        assertEquals("/api/libraries/lib-movies", server.next().target)
        assertEquals("/api/items?library_id=lib-movies&sort=added&order=desc&limit=60", server.next().target)
        assertEquals("/api/items?library_id=lib-movies&sort=added&order=desc&cursor=c2&limit=60", server.next().target)
        assertFalse((ui.content as Content.Items).page.hasMore)
        assertNull(server.nextOrNull())
    }

    @Test
    fun aFailedNextPageIsKeptBesideWhatWasLoaded() = runTest {
        respondLibrary(SampleMedia.movies)
        respondItems(SampleMedia.recentMovies.take(3), next = "c2")
        val model = model(SampleMedia.movies)
        model.ui.awaitUntil { it.content is Content.Items }
        server.respond("""{"error":{"code":"internal","message":"PopcornStash ran into a problem."}}""", 500)
        model.loadMore()
        val ui = model.ui.awaitUntil { (it.content as? Content.Items)?.page?.moreFailed != null }
        val page = (ui.content as Content.Items).page
        assertEquals(3, page.items.size)
        assertEquals("PopcornStash ran into a problem.", page.moreFailed)
    }

    @Test
    fun choosingASortLoadsTheLibraryAgainInThatOrder() = runTest {
        respondLibrary(SampleMedia.movies)
        respondItems(SampleMedia.recentMovies)
        val model = model(SampleMedia.movies)
        model.ui.awaitUntil { it.content is Content.Items }
        server.next()
        server.next()
        respondItems(SampleMedia.recentMovies.sortedBy { it.title })
        model.setSort(ItemSort.Title)
        val ui = model.ui.awaitUntil { (it.content as? Content.Items)?.sort == ItemSort.Title }
        assertEquals("/api/items?library_id=lib-movies&sort=title&order=asc&limit=60", server.next().target)
        assertEquals("A Quiet Orchard", (ui.content as Content.Items).page.items.first().title)
    }

    @Test
    fun aLibraryThatIsNotThereAnyMoreIsGone() = runTest {
        server.respond("""{"error":{"code":"not_found","message":"That library no longer exists."}}""", 404)
        val model = model(SampleMedia.kids)
        assertTrue(model.ui.awaitUntil { it.gone }.gone)
    }

    @Test
    fun aFailedFirstLoadSaysWhyAndTryAgainWorks() = runTest {
        server.respond("""{"error":{"code":"internal","message":"PopcornStash ran into a problem."}}""", 500)
        val model = model(SampleMedia.movies)
        val failed = model.ui.awaitUntil { it.content is Content.Failed }
        assertEquals("PopcornStash ran into a problem.", (failed.content as Content.Failed).message)
        respondLibrary(SampleMedia.movies)
        respondItems(SampleMedia.recentMovies)
        model.retry()
        model.ui.awaitUntil { it.content is Content.Items }
    }

    @Test
    fun anEventAboutThisLibraryLoadsAsMuchAsWasShownAgain() = runTest {
        respondLibrary(SampleMedia.movies)
        respondItems(List(70) { SampleMedia.paperBoats.copy(id = "m$it") }.take(60), next = "c2")
        val model = model(SampleMedia.movies)
        model.ui.awaitUntil { it.content is Content.Items }
        server.next()
        server.next()
        respondItems(List(70) { SampleMedia.paperBoats.copy(id = "m$it") }.take(10), next = "c3")
        model.loadMore()
        model.ui.awaitUntil { (it.content as? Content.Items)?.page?.items?.size == 70 }
        server.next()

        respondLibrary(SampleMedia.movies.copy(itemCount = 71))
        respondItems(List(71) { SampleMedia.paperBoats.copy(id = "m$it") }, next = "c4")
        events.emit(StashEvent.LibraryChanged("added", "m70", SampleMedia.movies.id))
        val ui = model.ui.awaitUntil { it.library?.itemCount == 71L && (it.content as? Content.Items)?.page?.items?.size == 71 }
        assertEquals("/api/libraries/lib-movies", server.next().target)
        assertEquals("/api/items?library_id=lib-movies&sort=added&order=desc&limit=70", server.next().target)
        assertEquals(71, (ui.content as Content.Items).page.items.size)
    }

    @Test
    fun anEventPastTheServersLargestPageLoadsEveryItemShownAgain() = runTest {
        val all = List(260) { SampleMedia.paperBoats.copy(id = "m$it") }
        respondLibrary(SampleMedia.movies)
        respondItems(all.take(60), next = "c2")
        val model = model(SampleMedia.movies)
        model.ui.awaitUntil { it.content is Content.Items }
        server.next()
        server.next()
        respondItems(all.subList(60, 250), next = "c3")
        model.loadMore()
        model.ui.awaitUntil { (it.content as? Content.Items)?.page?.items?.size == 250 }
        server.next()

        // 250 shown: the reload asks for the server's largest page (200), then the 50 after it.
        respondLibrary(SampleMedia.movies)
        respondItems(all.take(200), next = "r2")
        respondItems(all.subList(200, 250), next = "r3")
        events.emit(StashEvent.LibraryChanged("updated", "m3", SampleMedia.movies.id))
        val ui = model.ui.awaitUntil { (it.content as? Content.Items)?.page?.nextCursor == "r3" }
        assertEquals("/api/libraries/lib-movies", server.next().target)
        assertEquals("/api/items?library_id=lib-movies&sort=added&order=desc&limit=200", server.next().target)
        assertEquals("/api/items?library_id=lib-movies&sort=added&order=desc&cursor=r2&limit=50", server.next().target)
        assertEquals(all.take(250).map { it.id }, (ui.content as Content.Items).page.items.map { it.id })
    }

    @Test
    fun aTimelinePastTheServersLargestPageLoadsEveryPhotoShownAgain() = runTest {
        val photo = SampleMedia.timeline[0].items[0]
        fun month(name: String, from: Int, count: Int) = PhotoMonth(name, count.toLong(), List(count) { photo.copy(id = "p${from + it}") })
        respondLibrary(SampleMedia.photos)
        server.respond(json(PhotoTimeline.serializer(), PhotoTimeline(listOf(month("2026-09", 0, 200)), nextCursor = "p2")))
        val model = model(SampleMedia.photos)
        model.ui.awaitUntil { it.content is Content.Timeline }
        server.next()
        server.next()
        server.respond(json(PhotoTimeline.serializer(), PhotoTimeline(listOf(month("2026-09", 200, 100), month("2026-08", 300, 400)), nextCursor = "p3")))
        model.loadMore()
        model.ui.awaitUntil { (it.content as? Content.Timeline)?.photos?.size == 700 }
        server.next()

        // 700 photos shown: 500 (the server's largest page), then the 200 after them, September joined across pages.
        respondLibrary(SampleMedia.photos)
        server.respond(json(PhotoTimeline.serializer(), PhotoTimeline(listOf(month("2026-09", 0, 300), month("2026-08", 300, 200)), nextCursor = "q2")))
        server.respond(json(PhotoTimeline.serializer(), PhotoTimeline(listOf(month("2026-08", 500, 200)), nextCursor = "q3")))
        events.emit(StashEvent.LibraryChanged("updated", "p3", SampleMedia.photos.id))
        val ui = model.ui.awaitUntil { (it.content as? Content.Timeline)?.page?.nextCursor == "q3" }
        assertEquals("/api/libraries/lib-photos", server.next().target)
        assertEquals("/api/libraries/lib-photos/timeline?limit=500", server.next().target)
        assertEquals("/api/libraries/lib-photos/timeline?cursor=q2&limit=200", server.next().target)
        val timeline = ui.content as Content.Timeline
        assertEquals(listOf("2026-09", "2026-08"), timeline.page.items.map { it.month })
        assertEquals(700, timeline.photos.size)
    }

    @Test
    fun aRemovalMattersOnlyForAnItemOnThePage() = runTest {
        respondLibrary(SampleMedia.movies)
        respondItems(SampleMedia.recentMovies)
        val model = model(SampleMedia.movies)
        model.ui.awaitUntil { it.content is Content.Items }
        server.next()
        server.next()

        events.emit(StashEvent.LibraryChanged("removed", "somewhere-else", null))
        events.emit(StashEvent.LibraryChanged("added", "x", "another-library"))
        assertNull("nothing is asked for an item that is not here", server.nextOrNull())

        respondLibrary(SampleMedia.movies)
        respondItems(SampleMedia.recentMovies.drop(1))
        events.emit(StashEvent.LibraryChanged("removed", SampleMedia.northbound.id, null))
        model.ui.awaitUntil { (it.content as? Content.Items)?.page?.items?.size == 5 }
    }

    @Test
    fun aBurstOfEventsBecomesOneReload() = runTest {
        respondLibrary(SampleMedia.movies)
        respondItems(SampleMedia.recentMovies)
        val model = model(SampleMedia.movies, settle = 400)
        model.ui.awaitUntil { it.content is Content.Items }
        server.next()
        server.next()
        respondLibrary(SampleMedia.movies.copy(itemCount = 51))
        respondItems(SampleMedia.recentMovies)
        repeat(3) { events.emit(StashEvent.LibraryChanged("added", "n$it", SampleMedia.movies.id)) }
        model.ui.awaitUntil { it.library?.itemCount == 51L }
        server.next()
        server.next()
        assertNull("one reload for the burst", server.nextOrNull(500))
    }

    @Test
    fun aLibrariesEventThatHidesTheLibraryMakesItGone() = runTest {
        respondLibrary(SampleMedia.movies)
        respondItems(SampleMedia.recentMovies)
        val model = model(SampleMedia.movies)
        model.ui.awaitUntil { it.content is Content.Items }
        server.respond("""{"error":{"code":"not_found","message":"That library no longer exists."}}""", 404)
        events.emit(StashEvent.LibrariesChanged)
        assertTrue(model.ui.awaitUntil { it.gone }.gone)
    }

    @Test
    fun comingBackAfterAGapLoadsAgain() = runTest {
        respondLibrary(SampleMedia.movies)
        respondItems(SampleMedia.recentMovies)
        val model = model(SampleMedia.movies)
        model.ui.awaitUntil { it.content is Content.Items }
        respondLibrary(SampleMedia.movies.copy(itemCount = 49))
        respondItems(SampleMedia.recentMovies)
        reconnects.emit(Unit)
        model.ui.awaitUntil { it.library?.itemCount == 49L }
    }

    @Test
    fun monthsJoinOnlyWhenTheySplitAcrossPages() {
        val a = PhotoMonth("2026-09", 3, SampleMedia.timeline[0].items.take(2))
        val b = PhotoMonth("2026-09", 3, SampleMedia.timeline[0].items.drop(2).take(1))
        val c = PhotoMonth("2026-08", 1, SampleMedia.timeline[1].items.take(1))
        val merged = LibraryViewModel.mergeMonths(listOf(a), listOf(b, c))
        assertEquals(listOf("2026-09", "2026-08"), merged.map { it.month })
        assertEquals(3, merged[0].items.size)
        assertEquals(listOf(a, c), LibraryViewModel.mergeMonths(listOf(a), listOf(c)))
    }
}
