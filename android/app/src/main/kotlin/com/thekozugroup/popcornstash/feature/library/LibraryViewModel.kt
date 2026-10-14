package com.thekozugroup.popcornstash.feature.library

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.ApiException
import com.thekozugroup.popcornstash.data.api.Item
import com.thekozugroup.popcornstash.data.api.ItemPage
import com.thekozugroup.popcornstash.data.api.ItemQuery
import com.thekozugroup.popcornstash.data.api.Layout
import com.thekozugroup.popcornstash.data.api.Library
import com.thekozugroup.popcornstash.data.api.PhotoMonth
import com.thekozugroup.popcornstash.data.api.ShowSummary
import com.thekozugroup.popcornstash.data.api.StashEvent
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** How a one-offs library or Other files can be sorted (the web's choices; `sort` and `order` in `GET /api/items`). */
enum class ItemSort(val label: String, val wire: String, val order: String) {
    Taken("Date filmed", "taken", "desc"),
    Added("Recently added", "added", "desc"),
    Title("Title", "title", "asc"),
    Name("Name", "title", "asc"),
    Size("Largest first", "size", "desc"),
    ;

    companion object {
        /** Frames (camera videos) sort by the day they were filmed first; posters by when they came in. */
        fun optionsFor(layout: Layout): List<ItemSort> =
            if (layout == Layout.Frames) listOf(Taken, Added, Title, Size) else listOf(Added, Title, Size)

        val otherFiles = listOf(Added, Name, Size)
    }
}

/**
 * A list read again from its start after a live event: as many items as were [loaded] (at
 * least [LibraryViewModel.PAGE]), with the cursor for what follows. Past the server's largest
 * page ([LibraryViewModel.MAX_PAGE]) it follows the cursor, so nothing the person had scrolled
 * through is dropped. A first load ([loaded] 0) is one page.
 */
suspend fun reloadItems(loaded: Int, page: suspend (limit: Int, cursor: String?) -> ItemPage): Paged<Item> {
    var next = page(loaded.coerceIn(LibraryViewModel.PAGE, LibraryViewModel.MAX_PAGE), null)
    var items = next.items
    while (items.size < loaded && next.nextCursor != null) {
        next = page((loaded - items.size).coerceIn(1, LibraryViewModel.MAX_PAGE), next.nextCursor)
        items = items + next.items
    }
    return Paged(items, next.nextCursor)
}

/** A list read page by page with the server's cursor (docs/architecture.md section 8, Lists). */
data class Paged<T>(
    val items: List<T>,
    val nextCursor: String? = null,
    val loadingMore: Boolean = false,
    /** Why the next page did not come; the end of the list offers "Try again". */
    val moreFailed: String? = null,
) {
    val hasMore: Boolean get() = nextCursor != null
}

/**
 * A library page (android/DESIGN.md, A library): the library itself, then its content in the
 * library's layout: items for posters and frames (`GET /api/items?library_id=`), shows for a
 * series library, the month timeline for photos. Live `library` events about this library
 * and `libraries` events load it again; a library that is deleted or no longer shared
 * (404) is [Ui.gone], and the page closes.
 */
class LibraryViewModel(
    private val api: ApiClient,
    events: Flow<StashEvent>,
    reconnects: Flow<Unit>,
    private val libraryId: String,
    seed: Library? = null,
    private val settleMillis: Long = 400,
) : ViewModel() {
    sealed interface Content {
        data object Loading : Content

        data class Failed(val message: String) : Content

        data class Items(val sort: ItemSort, val page: Paged<Item>) : Content

        data class Shows(val shows: List<ShowSummary>) : Content

        data class Timeline(val page: Paged<PhotoMonth>) : Content {
            /** Every photo loaded so far, in the order the page shows them (the viewer pages through these). */
            val photos: List<Item> get() = page.items.flatMap { it.items }
        }
    }

    data class Ui(val library: Library?, val content: Content = Content.Loading, val gone: Boolean = false)

    private val _ui = MutableStateFlow(Ui(seed))
    val ui: StateFlow<Ui> = _ui.asStateFlow()

    /** The sort the person chose; null keeps the library's own default. */
    private var chosenSort: ItemSort? = null
    private var loadJob: Job? = null
    private var moreJob: Job? = null
    private var settleJob: Job? = null
    private val pendingMore = PendingMore()

    init {
        reload(keepLoaded = false)
        viewModelScope.launch { events.collect(::onEvent) }
        viewModelScope.launch { reconnects.collect { settleThenReload() } }
    }

    fun retry() = reload(keepLoaded = false)

    fun setSort(sort: ItemSort) {
        if (chosenSort == sort) return
        chosenSort = sort
        val library = _ui.value.library ?: return
        moreJob?.cancel()
        loadJob?.cancel()
        _ui.update { it.copy(content = Content.Loading) }
        loadJob = viewModelScope.launch { loadContent(library, keepLoaded = false) }
    }

    /** Loads the next page when the person nears the end of what is loaded. */
    fun loadMore() {
        if (moreJob?.isActive == true) return
        loadJob?.takeIf { it.isActive }?.let { load ->
            pendingMore.after(load) { viewModelScope.launch { loadMore() } }
            return
        }
        pendingMore.clear()
        val library = _ui.value.library ?: return
        when (val content = _ui.value.content) {
            is Content.Items -> {
                val cursor = content.page.nextCursor ?: return
                setContent(content.copy(page = content.page.copy(loadingMore = true, moreFailed = null)))
                moreJob = viewModelScope.launch {
                    try {
                        val next = api.items(query(library, content.sort, PAGE, cursor))
                        updateItems { it.copy(items = it.items + next.items, nextCursor = next.nextCursor, loadingMore = false) }
                    } catch (e: ApiException) {
                        if (e.isNotFound) return@launch markGone()
                        updateItems { it.copy(loadingMore = false, moreFailed = e.message) }
                    }
                }
            }
            is Content.Timeline -> {
                val cursor = content.page.nextCursor ?: return
                setContent(content.copy(page = content.page.copy(loadingMore = true, moreFailed = null)))
                moreJob = viewModelScope.launch {
                    try {
                        val next = api.timeline(libraryId, cursor, PHOTO_PAGE)
                        updateTimeline {
                            it.copy(items = mergeMonths(it.items, next.months), nextCursor = next.nextCursor, loadingMore = false)
                        }
                    } catch (e: ApiException) {
                        if (e.isNotFound) return@launch markGone()
                        updateTimeline { it.copy(loadingMore = false, moreFailed = e.message) }
                    }
                }
            }
            else -> Unit
        }
    }

    /** The sort in use for an items layout. */
    fun sortFor(library: Library): ItemSort {
        val options = ItemSort.optionsFor(library.layout)
        return chosenSort?.takeIf { it in options } ?: options.first()
    }

    private fun onEvent(event: StashEvent) {
        when (event) {
            is StashEvent.LibraryChanged -> {
                // A removal never names the library; it matters when the item is on this page.
                if (event.libraryId == libraryId || (event.libraryId == null && showsItem(event.itemId))) settleThenReload()
            }
            StashEvent.LibrariesChanged -> settleThenReload()
            else -> Unit
        }
    }

    private fun showsItem(id: String): Boolean = when (val content = _ui.value.content) {
        is Content.Items -> content.page.items.any { it.id == id }
        is Content.Timeline -> content.page.items.any { month -> month.items.any { it.id == id } }
        // A show's episodes are not listed here; any removal may change its counts.
        is Content.Shows -> true
        else -> false
    }

    /** A burst of events becomes one reload, of as much as was loaded (several pages past the server's largest). */
    private fun settleThenReload() {
        settleJob?.cancel()
        settleJob = viewModelScope.launch {
            delay(settleMillis)
            reload(keepLoaded = true)
        }
    }

    private fun reload(keepLoaded: Boolean) {
        loadJob?.cancel()
        moreJob?.cancel()
        loadJob = viewModelScope.launch {
            val library = try {
                api.library(libraryId)
            } catch (e: ApiException) {
                if (e.isNotFound) return@launch markGone()
                if (!hasContent()) _ui.update { it.copy(content = Content.Failed(e.message)) }
                return@launch
            }
            val before = _ui.value.library
            _ui.update { it.copy(library = library) }
            // A library whose layout changed (its type changed while empty) starts its content again.
            loadContent(library, keepLoaded = keepLoaded && before?.layout == library.layout)
        }
    }

    private suspend fun loadContent(library: Library, keepLoaded: Boolean) {
        val current = _ui.value.content
        try {
            val content = when (library.layout) {
                Layout.Series -> Content.Shows(api.shows(libraryId))
                Layout.Timeline -> {
                    val loaded = if (keepLoaded) (current as? Content.Timeline)?.photos?.size ?: 0 else 0
                    var timeline = api.timeline(libraryId, null, loaded.coerceIn(PHOTO_PAGE, MAX_PHOTO_PAGE))
                    var months = timeline.months
                    // Past the server's largest page, follow the cursor until as many photos are in as were shown.
                    while (months.sumOf { it.items.size } < loaded && timeline.nextCursor != null) {
                        val more = (loaded - months.sumOf { it.items.size }).coerceIn(1, MAX_PHOTO_PAGE)
                        timeline = api.timeline(libraryId, timeline.nextCursor, more)
                        months = mergeMonths(months, timeline.months)
                    }
                    Content.Timeline(Paged(months, timeline.nextCursor))
                }
                else -> {
                    val sort = sortFor(library)
                    val loaded = if (keepLoaded) (current as? Content.Items)?.takeIf { it.sort == sort }?.page?.items?.size ?: 0 else 0
                    Content.Items(sort, reloadItems(loaded) { limit, cursor -> api.items(query(library, sort, limit, cursor)) })
                }
            }
            setContent(content)
        } catch (e: ApiException) {
            if (e.isNotFound) return markGone()
            if (!keepLoaded || !hasContent()) setContent(Content.Failed(e.message))
        }
    }

    private fun query(library: Library, sort: ItemSort, limit: Int, cursor: String?) =
        ItemQuery(libraryId = library.id, sort = sort.wire, order = sort.order, limit = limit, cursor = cursor)

    private fun hasContent(): Boolean = when (_ui.value.content) {
        is Content.Items, is Content.Shows, is Content.Timeline -> true
        else -> false
    }

    private fun markGone() {
        settleJob?.cancel()
        _ui.update { it.copy(gone = true) }
    }

    private fun setContent(content: Content) = _ui.update { it.copy(content = content) }

    private fun updateItems(change: (Paged<Item>) -> Paged<Item>) = _ui.update { ui ->
        val content = ui.content as? Content.Items ?: return@update ui
        ui.copy(content = content.copy(page = change(content.page)))
    }

    private fun updateTimeline(change: (Paged<PhotoMonth>) -> Paged<PhotoMonth>) = _ui.update { ui ->
        val content = ui.content as? Content.Timeline ?: return@update ui
        ui.copy(content = content.copy(page = change(content.page)))
    }

    companion object {
        const val PAGE = 60
        /** The server's largest page of items (`GET /api/items`, `limit` at most 200). */
        const val MAX_PAGE = 200
        const val PHOTO_PAGE = 200
        /** The server's largest page of photos (`GET /api/libraries/:id/timeline`, `limit` at most 500). */
        const val MAX_PHOTO_PAGE = 500

        /** A busy month can continue on the next page: its photos join the month already shown. */
        fun mergeMonths(loaded: List<PhotoMonth>, next: List<PhotoMonth>): List<PhotoMonth> {
            if (loaded.isEmpty()) return next
            val last = loaded.last()
            val first = next.firstOrNull() ?: return loaded
            return if (first.month == last.month) {
                loaded.dropLast(1) + last.copy(items = last.items + first.items) + next.drop(1)
            } else {
                loaded + next
            }
        }
    }
}
