package com.thekozugroup.popcornstash.feature.library

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.ApiException
import com.thekozugroup.popcornstash.data.api.Item
import com.thekozugroup.popcornstash.data.api.ShowDetail
import com.thekozugroup.popcornstash.data.api.StashEvent
import com.thekozugroup.popcornstash.data.repo.Loadable
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** The episode a show's primary button plays: the one started and not finished, else the next one. */
data class UpNext(val episode: Item, val resume: Boolean)

/**
 * A show page (android/DESIGN.md, Series): this library's seasons and episodes of the show
 * (`GET /api/libraries/:id/shows/:name`), the season shown, and what plays next. Live events
 * about the library load it again; a show that left the library, or a library the person no
 * longer sees, is [Ui.gone].
 */
class ShowViewModel(
    private val api: ApiClient,
    events: Flow<StashEvent>,
    reconnects: Flow<Unit>,
    private val libraryId: String,
    private val showName: String,
    private val settleMillis: Long = 400,
) : ViewModel() {
    data class Ui(
        val show: Loadable<ShowDetail> = Loadable.Loading,
        /** The season on screen; null until the show loads. */
        val season: Int? = null,
        val gone: Boolean = false,
    ) {
        val upNext: UpNext? get() = show.valueOrNull?.let(::upNext)
    }

    private val _ui = MutableStateFlow(Ui())
    val ui: StateFlow<Ui> = _ui.asStateFlow()

    private var loadJob: Job? = null
    private var settleJob: Job? = null

    init {
        load()
        viewModelScope.launch {
            events.collect { event ->
                when (event) {
                    is StashEvent.LibraryChanged ->
                        if (event.libraryId == libraryId || (event.libraryId == null && hasEpisode(event.itemId))) settleThenLoad()
                    StashEvent.LibrariesChanged -> settleThenLoad()
                    else -> Unit
                }
            }
        }
        viewModelScope.launch { reconnects.collect { settleThenLoad() } }
    }

    fun retry() = load()

    /** After playing (the progress changed), quietly read the show again. */
    fun refresh() = load()

    fun selectSeason(season: Int) = _ui.update { it.copy(season = season) }

    private fun hasEpisode(id: String) =
        _ui.value.show.valueOrNull?.seasons?.any { season -> season.episodes.any { it.id == id } } == true

    private fun settleThenLoad() {
        settleJob?.cancel()
        settleJob = viewModelScope.launch {
            delay(settleMillis)
            load()
        }
    }

    private fun load() {
        loadJob?.cancel()
        loadJob = viewModelScope.launch {
            val before = _ui.value.show.valueOrNull
            _ui.update { it.copy(show = if (before != null) Loadable.Ready(before, refreshing = true) else Loadable.Loading) }
            try {
                val show = api.show(libraryId, showName)
                _ui.update { ui ->
                    val seasons = show.seasons.map { it.season }
                    val keep = ui.season?.takeIf { it in seasons }
                    ui.copy(show = Loadable.Ready(show), season = keep ?: upNext(show)?.episode?.season ?: seasons.firstOrNull())
                }
            } catch (e: ApiException) {
                if (e.isNotFound) {
                    _ui.update { it.copy(gone = true) }
                } else {
                    _ui.update { it.copy(show = Loadable.Failed(e.message, before)) }
                }
            }
        }
    }

    companion object {
        /**
         * What plays next: the last episode (in order) the person touched, if they did not finish
         * it; the one after it if they did; otherwise the first episode. Specials count only when
         * the show has nothing else.
         */
        fun upNext(show: ShowDetail): UpNext? {
            val regular = show.seasons.filter { it.season != 0 }.flatMap { it.episodes }
            val episodes = regular.ifEmpty { show.seasons.flatMap { it.episodes } }
            if (episodes.isEmpty()) return null
            val touched = episodes.indexOfLast { episode ->
                val progress = episode.progress ?: return@indexOfLast false
                progress.finished || progress.positionS > 0
            }
            if (touched < 0) return UpNext(episodes.first(), resume = false)
            val episode = episodes[touched]
            if (episode.progress?.finished == false) return UpNext(episode, resume = true)
            return UpNext(episodes.getOrNull(touched + 1) ?: episodes.first(), resume = false)
        }
    }
}
