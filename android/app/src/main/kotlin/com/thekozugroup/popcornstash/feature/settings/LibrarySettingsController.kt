package com.thekozugroup.popcornstash.feature.settings

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.ApiException
import com.thekozugroup.popcornstash.data.api.Library
import com.thekozugroup.popcornstash.data.api.UnassignedFolder
import com.thekozugroup.popcornstash.data.api.User
import com.thekozugroup.popcornstash.data.repo.Loadable
import com.thekozugroup.popcornstash.data.repo.StashData
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.mapNotNull
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** What the owner's Libraries section shows (android/DESIGN.md, Settings). */
data class LibrarySettingsUi(
    /** Every library, from the session's data, kept current by `libraries` events. */
    val libraries: Loadable<List<Library>> = Loadable.Idle,
    /** The order shown while a move is being saved, so the row moves at once. */
    val pendingOrder: List<Library>? = null,
    /** Everyone with an account, for "Only Ana and Tomas"; null until it has loaded. */
    val people: List<User>? = null,
    /** Top-level folders that are not a library yet. */
    val folders: List<UnassignedFolder> = emptyList(),
    val moving: Boolean = false,
    /** The library being deleted, while it is. */
    val deletingId: String? = null,
) {
    /** The libraries in the order the rows show. */
    val shown: List<Library>? get() = pendingOrder ?: libraries.valueOrNull

    val canMakeMore: Boolean get() = (shown?.size ?: 0) < LIBRARY_LIMIT
}

/**
 * The owner's library list in Settings: the libraries in order with their reorder and delete,
 * the people for who sees each one, and the folders that are not a library yet. Nothing here
 * moves a file: libraries are views of folders (docs/architecture.md section 3.1).
 */
class LibrarySettingsController(
    private val api: ApiClient,
    private val data: StashData,
    private val scope: CoroutineScope,
) {
    private val _state = MutableStateFlow(LibrarySettingsUi(libraries = data.libraries.value))
    val state: StateFlow<LibrarySettingsUi> = _state.asStateFlow()

    private val _notices = Channel<String>(Channel.BUFFERED)

    /** Sentences for a snackbar: a library deleted, a move that did not save. Each is shown once. */
    val notices: Flow<String> = _notices.receiveAsFlow()

    init {
        scope.launch { data.libraries.collect { libraries -> _state.update { it.copy(libraries = libraries) } } }
        // A library made from a folder, or deleted (here, on the web or on another phone),
        // changes which folders are not a library. The first list is the one refresh() loads.
        scope.launch {
            data.libraries
                .mapNotNull { (it as? Loadable.Ready)?.takeIf { ready -> !ready.refreshing }?.value }
                .map { list -> list.map { it.id to it.folder }.toSet() }
                .distinctUntilChanged()
                .drop(1)
                .collect { loadFolders() }
        }
    }

    /** Loads everything again: when the page shows and when the person asks. */
    fun refresh() {
        data.refreshLibraries()
        loadFolders()
        scope.launch {
            try {
                val people = api.users()
                _state.update { it.copy(people = people) }
            } catch (_: ApiException) {
                // Rows say "Only some people" until the names load.
            }
        }
    }

    private fun loadFolders() {
        scope.launch {
            try {
                val folders = api.unassignedFolders()
                _state.update { it.copy(folders = folders) }
            } catch (_: ApiException) {
                // The section stays as it was; the web lists them too.
            }
        }
    }

    /** Moves a library one place up ([step] -1) or down (+1) in the order Home and the web sidebar use. */
    fun move(libraryId: String, step: Int) {
        val current = _state.value
        val list = current.shown ?: return
        if (current.moving) return
        val from = list.indexOfFirst { it.id == libraryId }
        val to = from + step
        if (from < 0 || to !in list.indices) return
        val reordered = list.toMutableList().apply { add(to, removeAt(from)) }
        _state.update { it.copy(pendingOrder = reordered, moving = true) }
        scope.launch {
            try {
                api.reorderLibraries(reordered.map { it.id })
                data.refreshLibraries().join()
            } catch (e: ApiException) {
                _notices.trySend("The order was not saved. ${e.message}")
            } finally {
                _state.update { it.copy(pendingOrder = null, moving = false) }
            }
        }
    }

    /** Takes a library out of the app; its folder and files stay where they are. */
    fun delete(library: Library, removeFolder: Boolean = false) {
        if (_state.value.deletingId != null) return
        _state.update { it.copy(deletingId = library.id) }
        scope.launch {
            try {
                api.deleteLibrary(library.id, library.folder.takeIf { removeFolder })
                data.refreshLibraries().join()
                _notices.trySend(deletedNotice(library, removeFolder))
            } catch (e: ApiException) {
                _notices.trySend("${library.name} was not deleted. ${e.message}")
            } finally {
                _state.update { it.copy(deletingId = null) }
            }
        }
    }
}

/** Keeps the owner's library list for the Settings tab's first page. */
class LibrarySettingsViewModel(api: ApiClient, data: StashData) : ViewModel() {
    val controller = LibrarySettingsController(api, data, viewModelScope)
}
