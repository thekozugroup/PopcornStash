package com.thekozugroup.popcornstash.feature.library

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.ApiException
import com.thekozugroup.popcornstash.data.api.StashEvent
import com.thekozugroup.popcornstash.data.api.VideoLibraryFeed
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** Keeps every loaded collection current after events, reconnects and returns from playback. */
class LibraryFeedViewModel(
    private val api: ApiClient,
    events: Flow<StashEvent>,
    reconnects: Flow<Unit>,
    private val libraryId: String,
    private val settleMillis: Long = 400,
) : ViewModel() {
    data class Ui(
        val feed: VideoLibraryFeed? = null,
        val loading: Boolean = true,
        val error: String? = null,
        val loadingMore: Boolean = false,
        val moreFailed: String? = null,
        val gone: Boolean = false,
    )

    private val _ui = MutableStateFlow(Ui())
    val ui = _ui.asStateFlow()
    private var loadJob: Job? = null
    private var moreJob: Job? = null
    private var settleJob: Job? = null
    private val pendingMore = PendingMore()

    init {
        refresh()
        viewModelScope.launch {
            events.collect { event ->
                if (event == StashEvent.LibrariesChanged ||
                    event is StashEvent.LibraryChanged && (event.libraryId == libraryId || event.libraryId == null)
                ) scheduleRefresh()
            }
        }
        viewModelScope.launch { reconnects.collect { scheduleRefresh() } }
    }

    private fun scheduleRefresh() {
        settleJob?.cancel()
        settleJob = viewModelScope.launch { delay(settleMillis); refresh() }
    }

    fun refresh() {
        loadJob?.cancel()
        moreJob?.cancel()
        _ui.update { it.copy(loading = true, error = null, loadingMore = false, moreFailed = null) }
        val loaded = _ui.value.feed?.collections?.size ?: 0
        loadJob = viewModelScope.launch {
            try {
                val first = api.libraryFeed(libraryId)
                var collections = first.collections
                var cursor = first.nextCursor
                val seen = mutableSetOf<String>()
                // Re-read previously loaded pages so arrivals cannot leave stale or missing rows.
                while (collections.size < loaded && cursor != null && seen.add(cursor)) {
                    val next = api.libraryFeed(libraryId, cursor)
                    collections = (collections + next.collections).distinctBy { it.folder.key }
                    cursor = next.nextCursor
                }
                _ui.value = Ui(feed = first.copy(collections = collections, nextCursor = cursor), loading = false)
            } catch (e: ApiException) {
                if (e.isNotFound) markGone() else _ui.update { it.copy(loading = false, error = e.message) }
            }
        }
    }

    fun loadMore() {
        if (moreJob?.isActive == true || _ui.value.gone) return
        loadJob?.takeIf { it.isActive }?.let { load ->
            pendingMore.after(load) { viewModelScope.launch { loadMore() } }
            return
        }
        pendingMore.clear()
        val cursor = _ui.value.feed?.nextCursor ?: return
        _ui.update { it.copy(loadingMore = true, moreFailed = null) }
        moreJob = viewModelScope.launch {
            try {
                val next = api.libraryFeed(libraryId, cursor)
                _ui.update { current ->
                    current.copy(
                        feed = current.feed?.let { feed ->
                            feed.copy(collections = (feed.collections + next.collections).distinctBy { it.folder.key }, nextCursor = next.nextCursor)
                        },
                        loadingMore = false,
                    )
                }
            } catch (e: ApiException) {
                if (e.isNotFound) markGone() else _ui.update { it.copy(loadingMore = false, moreFailed = e.message) }
            }
        }
    }

    private fun markGone() {
        pendingMore.clear()
        _ui.value = Ui(loading = false, gone = true)
    }
}
