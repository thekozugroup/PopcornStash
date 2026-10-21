package com.thekozugroup.popcornstash.feature.downloads

import com.thekozugroup.popcornstash.data.api.ApiException
import com.thekozugroup.popcornstash.data.api.Download
import com.thekozugroup.popcornstash.data.api.DownloadKind
import com.thekozugroup.popcornstash.data.api.TorrentFile
import com.thekozugroup.popcornstash.data.api.TorrentFiles
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.delay

data class TorrentFilesState(
    val download: Download,
    val loading: Boolean = true,
    val ready: Boolean = false,
    val files: List<TorrentFile> = emptyList(),
    val selected: Set<Int> = emptySet(),
    val busy: Boolean = false,
    val error: String? = null,
    val filterError: String? = null,
    val filtersEdited: Boolean = false,
    val reviewRequired: Boolean = false,
    val draft: TorrentRuleDraft = TorrentRuleDraft(),
    val types: List<com.thekozugroup.popcornstash.data.api.TorrentFileKind> = listOf(com.thekozugroup.popcornstash.data.api.TorrentFileKind.Video, com.thekozugroup.popcornstash.data.api.TorrentFileKind.Subtitle),
) {
    val changed: Boolean get() = selected != files.filter { it.selected }.map { it.index }.toSet()
    val canSave: Boolean get() = ready && !loading && !busy && filterError == null && selected.isNotEmpty() && (changed || reviewRequired)
}

fun Download.canChooseTorrentFiles(): Boolean = kind == DownloadKind.Torrent && isActive()

/**
 * File choices are local until Save. Opening never applies new-torrent defaults to an
 * existing torrent. The caller supplies the session API's GET and PUT operations.
 */
class TorrentFilesController(
    private val load: suspend (String) -> TorrentFiles,
    private val save: suspend (String, List<Int>) -> Unit,
    private val scope: CoroutineScope,
    private val onSaved: () -> Unit = {},
    private val start: suspend (String, List<Int>) -> Unit = save,
) {
    private val mutableState = MutableStateFlow<TorrentFilesState?>(null)
    val state: StateFlow<TorrentFilesState?> = mutableState.asStateFlow()
    private var loadJob: Job? = null
    private var generation = 0L

    fun open(download: Download) {
        if (!download.canChooseTorrentFiles() || mutableState.value?.busy == true) return
        generation++
        loadJob?.cancel()
        loadJob = null
        mutableState.value = TorrentFilesState(download)
        refresh()
    }

    fun refresh() {
        val current = mutableState.value ?: return
        if (current.busy) return
        generation++
        loadJob?.cancel()
        val requestGeneration = generation
        mutableState.value = current.copy(loading = true, error = null)
        loadJob = scope.launch {
            try {
                do {
                val manifest = load(current.download.id)
                if (generation == requestGeneration) {
                    mutableState.update { latest ->
                        latest?.copy(
                            loading = false,
                            ready = manifest.ready,
                            files = manifest.files,
                            reviewRequired = manifest.reviewRequired,
                            draft = if (latest.filtersEdited) latest.draft else manifest.rules?.toDraft() ?: TorrentRuleDraft(),
                            types = if (latest.filtersEdited) latest.types else manifest.rules?.fileTypes ?: latest.types,
                            selected = if (latest.changed || latest.filtersEdited) {
                                latest.selected.intersect(manifest.files.map { it.index }.toSet())
                            } else {
                                manifest.files.filter { it.selected }.map { it.index }.toSet()
                            },
                            error = null,
                        )
                    }
                }
                if (!manifest.ready) delay(3000)
                } while (!manifest.ready && generation == requestGeneration)
            } catch (e: ApiException) {
                if (generation == requestGeneration) mutableState.update { it?.copy(loading = false, error = e.message) }
            }
        }
    }

    fun toggle(index: Int) {
        mutableState.update { current ->
            if (current == null || current.loading || !current.ready || current.busy || current.files.none { it.index == index }) {
                current
            } else {
                current.copy(selected = if (index in current.selected) current.selected - index else current.selected + index, error = null)
            }
        }
    }

    fun draft(value: TorrentRuleDraft) {
        mutableState.update { current -> current?.editFilters(value, current.types) }
    }

    fun type(value: com.thekozugroup.popcornstash.data.api.TorrentFileKind) {
        mutableState.update { current -> current?.editFilters(current.draft, if (value in current.types) current.types - value else current.types + value) }
    }

    private fun TorrentFilesState.editFilters(value: TorrentRuleDraft, nextTypes: List<com.thekozugroup.popcornstash.data.api.TorrentFileKind>): TorrentFilesState {
        if (!ready || loading || busy) return this
        val next = copy(draft = value.copy(enabled = true), types = nextTypes, filtersEdited = true, error = null)
        if (nextTypes.isEmpty()) return next.copy(selected = emptySet(), filterError = "Choose at least one file type.")
        return try {
            val rules = requireNotNull(next.draft.rules(nextTypes))
            next.copy(selected = files.filter { rules.matches(it) }.map { it.index }.toSet(), filterError = null)
        } catch (e: IllegalArgumentException) { next.copy(filterError = e.message) }
    }

    fun close() {
        if (mutableState.value?.busy == true) return
        generation++
        loadJob?.cancel()
        mutableState.value = null
    }

    fun submit() {
        val current = mutableState.value ?: return
        if (!current.canSave) return
        mutableState.value = current.copy(busy = true, error = null)
        scope.launch {
            try {
                if (current.reviewRequired) start(current.download.id, current.selected.sorted())
                else save(current.download.id, current.selected.sorted())
                mutableState.value = null
                onSaved()
            } catch (e: ApiException) {
                mutableState.update { it?.copy(busy = false, error = e.message) }
            }
        }
    }
}
