package com.thekozugroup.popcornstash.feature.library

import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.Season
import com.thekozugroup.popcornstash.data.api.ShowDetail
import com.thekozugroup.popcornstash.data.api.StashEvent
import com.thekozugroup.popcornstash.data.api.baseHttpClient
import com.thekozugroup.popcornstash.data.repo.Loadable
import com.thekozugroup.popcornstash.testing.MainDispatcherRule
import com.thekozugroup.popcornstash.testing.keep
import com.thekozugroup.popcornstash.testing.SampleMedia
import com.thekozugroup.popcornstash.testing.awaitUntil
import com.thekozugroup.popcornstash.testing.json
import com.thekozugroup.popcornstash.testing.next
import com.thekozugroup.popcornstash.testing.respond
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.emptyFlow
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

/** A show page: its seasons and episodes in this library, the season shown and what plays next. */
class ShowViewModelTest {
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

    private fun model(name: String = "The Lantern Keepers") =
        main.keep { ShowViewModel(api, events, emptyFlow(), "lib-shows", name, settleMillis = 0) }

    @Test
    fun loadsTheShowAndOpensOnTheSeasonOfWhatPlaysNext() = runTest {
        server.respond(json(ShowDetail.serializer(), SampleMedia.lanternShow))
        val model = model()
        val ui = model.ui.awaitUntil { it.show is Loadable.Ready }
        assertEquals("/api/libraries/lib-shows/shows/The%20Lantern%20Keepers", server.next().target)
        assertEquals(1, ui.season)
        assertEquals("e-l102", ui.upNext?.episode?.id)
        assertTrue(ui.upNext!!.resume)
        model.selectSeason(2)
        assertEquals(2, model.ui.value.season)
    }

    @Test
    fun aShowThatLeftTheLibraryIsGone() = runTest {
        server.respond("""{"error":{"code":"not_found","message":"That show is not in this library."}}""", 404)
        assertTrue(model("Gone").ui.awaitUntil { it.gone }.gone)
    }

    @Test
    fun aFailedReloadKeepsWhatWasShown() = runTest {
        server.respond(json(ShowDetail.serializer(), SampleMedia.lanternShow))
        val model = model()
        model.ui.awaitUntil { it.show is Loadable.Ready }
        server.respond("""{"error":{"code":"internal","message":"PopcornStash ran into a problem."}}""", 500)
        events.emit(StashEvent.LibraryChanged("updated", "e-l103", "lib-shows"))
        val ui = model.ui.awaitUntil { it.show is Loadable.Failed }
        assertEquals(SampleMedia.lanternShow, ui.show.valueOrNull)
        assertFalse(ui.gone)
    }

    @Test
    fun aSeasonThatIsGoneAfterAReloadFallsBack() = runTest {
        server.respond(json(ShowDetail.serializer(), SampleMedia.lanternShow))
        val model = model()
        model.ui.awaitUntil { it.show is Loadable.Ready }
        model.selectSeason(2)
        server.respond(json(ShowDetail.serializer(), SampleMedia.lanternShow.copy(seasons = listOf(Season(1, SampleMedia.lantern)))))
        model.refresh()
        val ui = model.ui.awaitUntil { it.show is Loadable.Ready && it.show.valueOrNull?.seasons?.size == 1 }
        assertEquals(1, ui.season)
    }

    // ------------------------------------------------------------------ up next ----

    private fun show(vararg seasons: Season) = ShowDetail("Show", null, seasons.toList())

    @Test
    fun nothingWatchedPlaysTheFirstEpisode() {
        val fresh = SampleMedia.lantern.map { it.copy(progress = null) }
        val next = ShowViewModel.upNext(show(Season(1, fresh)))!!
        assertEquals("e-l101", next.episode.id)
        assertFalse(next.resume)
    }

    @Test
    fun aStartedEpisodeResumes() {
        val next = ShowViewModel.upNext(SampleMedia.lanternShow)!!
        assertEquals("e-l102", next.episode.id)
        assertTrue(next.resume)
    }

    @Test
    fun afterAFinishedEpisodeComesTheNextOneAcrossSeasons() {
        val season1 = SampleMedia.lantern.map { it.copy(progress = com.thekozugroup.popcornstash.data.api.Progress(1440.0, 1440.0, finished = true)) }
        val next = ShowViewModel.upNext(show(Season(1, season1), Season(2, SampleMedia.lanternSeason2)))!!
        assertEquals("e-l201", next.episode.id)
        assertFalse(next.resume)
    }

    @Test
    fun specialsComeIntoItOnlyWhenThereIsNothingElse() {
        val specials = Season(0, listOf(SampleMedia.episode("sp1", "Show", 0, 1, "Holiday special")))
        assertEquals("e-l101", ShowViewModel.upNext(show(specials, Season(1, SampleMedia.lantern.map { it.copy(progress = null) })))!!.episode.id)
        assertEquals("sp1", ShowViewModel.upNext(show(specials))!!.episode.id)
        assertNull(ShowViewModel.upNext(show()))
    }
}
