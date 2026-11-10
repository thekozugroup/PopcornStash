package com.thekozugroup.popcornstash.feature.library

import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.StashEvent
import com.thekozugroup.popcornstash.data.api.VideoLibraryFeed
import com.thekozugroup.popcornstash.data.api.baseHttpClient
import com.thekozugroup.popcornstash.testing.MainDispatcherRule
import com.thekozugroup.popcornstash.testing.VideoFeedFixtures
import com.thekozugroup.popcornstash.testing.awaitUntil
import com.thekozugroup.popcornstash.testing.json
import com.thekozugroup.popcornstash.testing.keep
import com.thekozugroup.popcornstash.testing.next
import com.thekozugroup.popcornstash.testing.nextOrNull
import com.thekozugroup.popcornstash.testing.respond
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.test.runTest
import mockwebserver3.MockWebServer
import org.junit.After
import org.junit.Assert.*
import org.junit.Before
import org.junit.Rule
import org.junit.Test

class LibraryFeedViewModelTest {
    @get:Rule val main = MainDispatcherRule()
    private val server = MockWebServer()
    private val events = MutableSharedFlow<StashEvent>(extraBufferCapacity = 16)
    private val reconnects = MutableSharedFlow<Unit>(extraBufferCapacity = 4)
    private val first = VideoFeedFixtures.movies
    private val second = VideoLibraryFeed(collections = VideoFeedFixtures.collections.drop(12))
    private lateinit var api: ApiClient

    @Before fun setup() { server.start(); api = ApiClient(server.url("/"), baseHttpClient("test")) }
    @After fun cleanup() = server.close()
    private fun respond(feed: VideoLibraryFeed) = server.respond(json(VideoLibraryFeed.serializer(), feed))
    private fun model() = main.keep { LibraryFeedViewModel(api, events, reconnects, "lib-movies", settleMillis = 0) }
    private suspend fun ready(model: LibraryFeedViewModel) = model.ui.awaitUntil { !it.loading && it.feed != null }

    @Test fun firstPageIncludesMainRowsAndTwelveCollections() = runTest {
        respond(first)
        val ui = ready(model())
        assertEquals(first, ui.feed)
        assertEquals("/api/libraries/lib-movies/feed", server.next().target)
        assertEquals(12, ui.feed!!.collections.size)
    }

    @Test fun pagingAppendsEveryCollectionAndRetainsFirstPageRows() = runTest {
        respond(first)
        val model = model(); ready(model); server.next()
        respond(second); model.loadMore()
        val feed = model.ui.awaitUntil { it.feed?.collections?.size == 14 }.feed!!
        assertEquals(first.continueWatching, feed.continueWatching)
        assertEquals(first.suggested, feed.suggested)
        assertEquals(first.recent, feed.recent)
        assertNull(feed.nextCursor)
        assertEquals("/api/libraries/lib-movies/feed?cursor=12", server.next().target)
    }

    @Test fun failedPagingKeepsRowsAndRetriesTheSameCursor() = runTest {
        respond(first)
        val model = model(); ready(model); server.next()
        server.respond("{}", 503); model.loadMore()
        val failed = model.ui.awaitUntil { it.moreFailed != null }
        assertEquals(first, failed.feed)
        assertFalse(failed.loadingMore)
        server.next()
        respond(second); model.loadMore()
        model.ui.awaitUntil { it.feed?.nextCursor == null && it.feed != null }
        assertEquals("/api/libraries/lib-movies/feed?cursor=12", server.next().target)
    }

    @Test fun refreshReloadsAllPreviouslyLoadedCollections() = runTest {
        respond(first)
        val model = model(); ready(model); server.next()
        respond(second); model.loadMore()
        model.ui.awaitUntil { it.feed?.collections?.size == 14 }; server.next()
        val changed = first.copy(suggested = first.recent.takeLast(1))
        respond(changed); respond(second)
        model.refresh()
        val ui = model.ui.awaitUntil { !it.loading && it.feed?.suggested == changed.suggested }
        assertEquals(14, ui.feed!!.collections.size)
        assertEquals("/api/libraries/lib-movies/feed", server.next().target)
        assertEquals("/api/libraries/lib-movies/feed?cursor=12", server.next().target)
    }

    @Test fun relevantLiveChangesAndReconnectRefreshTheFeed() = runTest {
        respond(first)
        val model = model(); ready(model); server.next()
        respond(first.copy(recent = emptyList()))
        events.emit(StashEvent.LibraryChanged("updated", "changed", "lib-movies"))
        model.ui.awaitUntil { it.feed?.recent?.isEmpty() == true }; server.next()
        respond(first.copy(suggested = emptyList()))
        reconnects.emit(Unit)
        model.ui.awaitUntil { it.feed?.suggested?.isEmpty() == true }; server.next()
        respond(first.copy(continueWatching = emptyList()))
        events.emit(StashEvent.LibraryChanged("removed", "removed", null))
        model.ui.awaitUntil { it.feed?.continueWatching?.isEmpty() == true }; server.next()
    }

    @Test fun unrelatedLibraryChangesDoNotFetchThisFeed() = runTest {
        respond(first)
        val model = model(); ready(model); server.next()
        events.emit(StashEvent.LibraryChanged("updated", "other", "lib-other"))
        assertNull(server.nextOrNull())
    }

    @Test fun accessRevokedDuringPagingClearsAllPrivateRows() = runTest {
        respond(first)
        val model = model(); ready(model); server.next()
        server.respond("{}", 404); model.loadMore()
        val ui = model.ui.awaitUntil { it.gone }
        assertNull(ui.feed)
        assertFalse(ui.loading)
        assertFalse(ui.loadingMore)
    }

    @Test fun firstLoadFailureCanBeRetried() = runTest {
        server.respond("{}", 503)
        val model = model(); model.ui.awaitUntil { it.error != null }; server.next()
        respond(first); model.refresh()
        assertEquals(first, ready(model).feed)
    }
}
