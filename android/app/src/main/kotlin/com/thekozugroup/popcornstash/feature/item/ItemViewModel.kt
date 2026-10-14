package com.thekozugroup.popcornstash.feature.item

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.ApiException
import com.thekozugroup.popcornstash.data.api.Item
import com.thekozugroup.popcornstash.data.api.ItemDetail
import com.thekozugroup.popcornstash.data.api.ItemPatch
import com.thekozugroup.popcornstash.data.api.StashEvent
import com.thekozugroup.popcornstash.data.repo.Loadable
import kotlinx.coroutines.Job
import kotlinx.coroutines.channels.BufferOverflow
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/**
 * An item's page (android/DESIGN.md, Item detail): `GET /api/items/:id`, the next episode in
 * the same library when there is one, and the favourite (`PATCH /api/items/:id`). A live
 * event about the item reads it again; an item the person can no longer see (404) is
 * [Ui.gone].
 */
class ItemViewModel(
    private val api: ApiClient,
    events: Flow<StashEvent>,
    reconnects: Flow<Unit>,
    private val itemId: String,
) : ViewModel() {
    data class Ui(
        val item: Loadable<ItemDetail> = Loadable.Loading,
        /** The next episode in the same library, once read. */
        val next: Item? = null,
        val savingFavourite: Boolean = false,
        val gone: Boolean = false,
    )

    private val _ui = MutableStateFlow(Ui())
    val ui: StateFlow<Ui> = _ui.asStateFlow()

    private val _messages = MutableSharedFlow<String>(extraBufferCapacity = 4, onBufferOverflow = BufferOverflow.DROP_OLDEST)

    /** Sentences for a snackbar ("Could not save the favourite."). */
    val messages: SharedFlow<String> = _messages.asSharedFlow()

    private var loadJob: Job? = null

    init {
        load()
        viewModelScope.launch {
            events.collect { event ->
                if (event is StashEvent.LibraryChanged && event.itemId == itemId) load()
                if (event is StashEvent.LibrariesChanged) load()
            }
        }
        viewModelScope.launch { reconnects.collect { load() } }
    }

    fun retry() = load()

    /** After playing (the progress changed), read the item again. */
    fun refresh() = load()

    /** Adds to or removes from the household's favourites at once; puts it back if the server says no. */
    fun toggleFavourite() {
        val item = _ui.value.item.valueOrNull ?: return
        if (_ui.value.savingFavourite) return
        val wanted = !item.favorite
        _ui.update { it.copy(item = Loadable.Ready(item.copy(favorite = wanted)), savingFavourite = true) }
        viewModelScope.launch {
            try {
                val saved = api.updateItem(itemId, ItemPatch(favorite = wanted))
                _ui.update { it.copy(item = Loadable.Ready(saved), savingFavourite = false) }
            } catch (e: ApiException) {
                if (e.isNotFound) {
                    _ui.update { it.copy(gone = true, savingFavourite = false) }
                    return@launch
                }
                _ui.update { ui ->
                    val now = ui.item.valueOrNull ?: item
                    ui.copy(item = Loadable.Ready(now.copy(favorite = !wanted)), savingFavourite = false)
                }
                _messages.tryEmit(e.message)
            }
        }
    }

    private fun load() {
        loadJob?.cancel()
        loadJob = viewModelScope.launch {
            val before = _ui.value.item.valueOrNull
            _ui.update { it.copy(item = if (before != null) Loadable.Ready(before, refreshing = true) else Loadable.Loading) }
            val item = try {
                api.item(itemId)
            } catch (e: ApiException) {
                if (e.isNotFound) {
                    _ui.update { it.copy(gone = true) }
                } else {
                    _ui.update { it.copy(item = Loadable.Failed(e.message, before)) }
                }
                return@launch
            }
            _ui.update { it.copy(item = Loadable.Ready(item), next = it.next?.takeIf { next -> next.id == item.nextEpisodeId }) }
            val nextId = item.nextEpisodeId ?: return@launch _ui.update { it.copy(next = null) }
            val next = try {
                api.item(nextId).item
            } catch (_: ApiException) {
                null
            }
            _ui.update { it.copy(next = next) }
        }
    }
}
