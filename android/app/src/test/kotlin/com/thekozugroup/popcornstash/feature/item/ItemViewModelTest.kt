package com.thekozugroup.popcornstash.feature.item

import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.ItemDetail
import com.thekozugroup.popcornstash.data.api.StashEvent
import com.thekozugroup.popcornstash.data.api.baseHttpClient
import com.thekozugroup.popcornstash.data.repo.Loadable
import com.thekozugroup.popcornstash.testing.MainDispatcherRule
import com.thekozugroup.popcornstash.testing.keep
import com.thekozugroup.popcornstash.testing.SampleMedia
import com.thekozugroup.popcornstash.testing.awaitUntil
import com.thekozugroup.popcornstash.testing.bodyText
import com.thekozugroup.popcornstash.testing.json
import com.thekozugroup.popcornstash.testing.next
import com.thekozugroup.popcornstash.testing.nextOrNull
import com.thekozugroup.popcornstash.testing.respond
import com.thekozugroup.popcornstash.testing.withinRealSeconds
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.emptyFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.runTest
import mockwebserver3.MockWebServer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test

/** An item's page: the item, the next episode, the favourite, and live changes. */
class ItemViewModelTest {
    @get:Rule
    val main = MainDispatcherRule()

    private val server = MockWebServer()
    private val events = MutableSharedFlow<StashEvent>(extraBufferCapacity = 16)
    private lateinit var api: ApiClient

    @Before
    fun setUp() {
        server.start()
        api = ApiClient(server.url("/"), baseHttpClient("t"))
    }

    @After
    fun tearDown() = server.close()

    private fun respondItem(item: ItemDetail) = server.respond(json(ItemDetail.serializer(), item))

    @Test
    fun anEpisodeLoadsWithTheNextEpisodeInItsLibrary() = runTest {
        respondItem(SampleMedia.episodeDetail)
        respondItem(SampleMedia.episodeDetail.copy(id = "e-l103", episode = 3, episodeTitle = "The lantern keepers", nextEpisodeId = null))
        val model = main.keep { ItemViewModel(api, events, emptyFlow(), "e-l102") }
        val ui = model.ui.awaitUntil { it.next != null }
        assertEquals("/api/items/e-l102", server.next().target)
        assertEquals("/api/items/e-l103", server.next().target)
        assertEquals("Fog over the pier", ui.item.valueOrNull?.episodeTitle)
        assertEquals("The lantern keepers", ui.next?.episodeTitle)
    }

    @Test
    fun aMovieHasNoNext() = runTest {
        respondItem(SampleMedia.paperBoatsDetail)
        val model = main.keep { ItemViewModel(api, events, emptyFlow(), "m-paper") }
        val ui = model.ui.awaitUntil { it.item is Loadable.Ready }
        assertNull(ui.next)
    }

    @Test
    fun theFavouriteChangesAtOnceAndIsSaved() = runTest {
        respondItem(SampleMedia.paperBoatsDetail)
        val model = main.keep { ItemViewModel(api, events, emptyFlow(), "m-paper") }
        model.ui.awaitUntil { it.item is Loadable.Ready }
        server.next()
        respondItem(SampleMedia.paperBoatsDetail.copy(favorite = true))
        model.toggleFavourite()
        assertTrue(model.ui.value.item.valueOrNull!!.favorite)
        model.ui.awaitUntil { !it.savingFavourite }
        val patch = server.next()
        assertEquals("PATCH", patch.method)
        assertEquals("/api/items/m-paper", patch.target)
        assertEquals("""{"favorite":true}""", patch.bodyText)
        assertTrue(model.ui.value.item.valueOrNull!!.favorite)
    }

    @Test
    fun aFavouriteTheServerRefusesGoesBackAndSaysWhy() = runTest {
        respondItem(SampleMedia.paperBoatsDetail)
        val model = main.keep { ItemViewModel(api, events, emptyFlow(), "m-paper") }
        model.ui.awaitUntil { it.item is Loadable.Ready }
        server.respond("""{"error":{"code":"internal","message":"PopcornStash ran into a problem."}}""", 500)
        val message = backgroundMessage(model)
        model.toggleFavourite()
        val ui = model.ui.awaitUntil { !it.savingFavourite }
        assertFalse(ui.item.valueOrNull!!.favorite)
        assertEquals("PopcornStash ran into a problem.", message.await())
    }

    @Test
    fun anItemThePersonCanNoLongerSeeIsGone() = runTest {
        respondItem(SampleMedia.paperBoatsDetail)
        val model = main.keep { ItemViewModel(api, events, emptyFlow(), "m-paper") }
        model.ui.awaitUntil { it.item is Loadable.Ready }
        server.respond("""{"error":{"code":"not_found","message":"That item no longer exists."}}""", 404)
        events.emit(StashEvent.LibraryChanged("removed", "m-paper", null))
        assertTrue(model.ui.awaitUntil { it.gone }.gone)
    }

    @Test
    fun eventsAboutOtherItemsChangeNothing() = runTest {
        respondItem(SampleMedia.paperBoatsDetail)
        val model = main.keep { ItemViewModel(api, events, emptyFlow(), "m-paper") }
        model.ui.awaitUntil { it.item is Loadable.Ready }
        server.next()
        events.emit(StashEvent.LibraryChanged("updated", "someone-else", "lib-movies"))
        assertNull(server.nextOrNull())
    }

    private fun TestScope.backgroundMessage(model: ItemViewModel) =
        backgroundScope.async(Dispatchers.Unconfined) { withinRealSeconds { model.messages.first() } }
}
