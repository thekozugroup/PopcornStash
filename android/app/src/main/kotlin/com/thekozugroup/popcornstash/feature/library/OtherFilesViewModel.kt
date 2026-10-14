package com.thekozugroup.popcornstash.feature.library

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.ApiException
import com.thekozugroup.popcornstash.data.api.Item
import com.thekozugroup.popcornstash.data.api.ItemQuery
import com.thekozugroup.popcornstash.data.api.Place
import com.thekozugroup.popcornstash.data.api.StashEvent
import com.thekozugroup.popcornstash.data.repo.Loadable
import kotlinx.coroutines.Job
import kotlinx.coroutines.channels.BufferOverflow
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import java.io.File

/** A private copy of an Other file, ready to hand to another app for reading. */
data class OpenedFile(val file: File, val name: String, val mime: String?)

/**
 * Other files (android/DESIGN.md, A library): music, documents and anything else the person
 * may see (`GET /api/items?place=other_files`). Opening one fetches a private copy into
 * [openedDir] with the session's client (docs/architecture.md section 10, Opening an Other
 * file); the page then hands that copy to another app.
 */
class OtherFilesViewModel(
    private val api: ApiClient,
    events: Flow<StashEvent>,
    reconnects: Flow<Unit>,
    private val files: FileFetcher,
    private val openedDir: File,
    private val settleMillis: Long = 400,
) : ViewModel() {
    data class Ui(
        val files: Loadable<Paged<Item>> = Loadable.Loading,
        val sort: ItemSort = ItemSort.Added,
        /** The file being fetched to open; its row shows a small indicator. */
        val opening: String? = null,
    )

    private val _ui = MutableStateFlow(Ui())
    val ui: StateFlow<Ui> = _ui.asStateFlow()

    private val _opened = MutableSharedFlow<OpenedFile>(extraBufferCapacity = 1, onBufferOverflow = BufferOverflow.DROP_OLDEST)
    val opened: SharedFlow<OpenedFile> = _opened.asSharedFlow()

    private val _messages = MutableSharedFlow<String>(extraBufferCapacity = 4, onBufferOverflow = BufferOverflow.DROP_OLDEST)
    val messages: SharedFlow<String> = _messages.asSharedFlow()

    private var loadJob: Job? = null
    private var moreJob: Job? = null
    private var settleJob: Job? = null
    private val pendingMore = PendingMore()
    private var openJob: Job? = null

    init {
        load(keepLoaded = false)
        viewModelScope.launch {
            events.collect { event ->
                // Other files belong to no library, so their events name none (and removals never do).
                if (event is StashEvent.LibraryChanged && event.libraryId == null) settleThenLoad()
            }
        }
        viewModelScope.launch { reconnects.collect { settleThenLoad() } }
    }

    fun retry() = load(keepLoaded = false)

    fun setSort(sort: ItemSort) {
        if (sort == _ui.value.sort) return
        _ui.update { it.copy(sort = sort, files = Loadable.Loading) }
        load(keepLoaded = false)
    }

    fun loadMore() {
        if (moreJob?.isActive == true) return
        loadJob?.takeIf { it.isActive }?.let { load ->
            pendingMore.after(load) { viewModelScope.launch { loadMore() } }
            return
        }
        pendingMore.clear()
        val page = (_ui.value.files as? Loadable.Ready)?.value ?: return
        val cursor = page.nextCursor ?: return
        update { it.copy(loadingMore = true, moreFailed = null) }
        moreJob = viewModelScope.launch {
            try {
                val next = api.items(query(_ui.value.sort, LibraryViewModel.PAGE, cursor))
                update { it.copy(items = it.items + next.items, nextCursor = next.nextCursor, loadingMore = false) }
            } catch (e: ApiException) {
                update { it.copy(loadingMore = false, moreFailed = e.message) }
            }
        }
    }

    /** Fetches a private copy of [item] and announces it on [opened]. One file at a time. */
    fun open(item: Item) {
        if (openJob?.isActive == true) return
        _ui.update { it.copy(opening = item.id) }
        openJob = viewModelScope.launch {
            val name = MediaFormat.fileName(item.relPath).ifBlank { item.title }
            // Each copy in its own folder, so two files with one name never meet.
            val target = File(File(openedDir, item.id), name)
            try {
                files.download(api.fileUrl(item.id), target)
                _opened.tryEmit(OpenedFile(target, name, mimeFor(name)))
            } catch (e: ApiException) {
                _messages.tryEmit(if (e.isNotFound) "That file is not available any more." else e.message)
                if (e.isNotFound) settleThenLoad()
            } finally {
                _ui.update { it.copy(opening = null) }
            }
        }
    }

    private fun settleThenLoad() {
        settleJob?.cancel()
        settleJob = viewModelScope.launch {
            delay(settleMillis)
            load(keepLoaded = true)
        }
    }

    private fun load(keepLoaded: Boolean) {
        loadJob?.cancel()
        moreJob?.cancel()
        loadJob = viewModelScope.launch {
            val before = _ui.value.files.valueOrNull
            if (before == null) _ui.update { it.copy(files = Loadable.Loading) }
            // As many as were shown, so a live event never drops what the person scrolled through.
            val loaded = if (keepLoaded && before != null) before.items.size else 0
            try {
                val sort = _ui.value.sort
                val files = reloadItems(loaded) { limit, cursor -> api.items(query(sort, limit, cursor)) }
                _ui.update { it.copy(files = Loadable.Ready(files)) }
            } catch (e: ApiException) {
                _ui.update { it.copy(files = if (keepLoaded && before != null) Loadable.Ready(before) else Loadable.Failed(e.message)) }
            }
        }
    }

    private fun query(sort: ItemSort, limit: Int, cursor: String?) =
        ItemQuery(place = Place.OtherFiles, sort = sort.wire, order = sort.order, limit = limit, cursor = cursor)

    private fun update(change: (Paged<Item>) -> Paged<Item>) = _ui.update { ui ->
        val page = (ui.files as? Loadable.Ready)?.value ?: return@update ui
        ui.copy(files = Loadable.Ready(change(page)))
    }

    companion object {
        /** The type another app is told, from the extension; null lets the system guess. */
        fun mimeFor(name: String): String? {
            val type = MediaFormat.fileType(name)?.lowercase() ?: return null
            return android.webkit.MimeTypeMap.getSingleton().getMimeTypeFromExtension(type)
        }
    }
}
