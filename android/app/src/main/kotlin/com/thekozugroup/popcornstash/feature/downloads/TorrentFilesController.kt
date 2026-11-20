package com.thekozugroup.popcornstash.feature.downloads

import com.thekozugroup.popcornstash.data.api.ApiException
import com.thekozugroup.popcornstash.data.api.Download
import com.thekozugroup.popcornstash.data.api.DownloadKind
import com.thekozugroup.popcornstash.data.api.FinishCompletedRequest
import com.thekozugroup.popcornstash.data.api.TorrentFile
import com.thekozugroup.popcornstash.data.api.TorrentFileKind
import com.thekozugroup.popcornstash.data.api.TorrentFiles
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull

private const val PROGRESS_VALID_MS = 9_000L

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
    val filtersPending: Boolean = false,
    val reviewRequired: Boolean = false,
    val draft: TorrentRuleDraft = TorrentRuleDraft(),
    val types: List<TorrentFileKind> = listOf(TorrentFileKind.Video, TorrentFileKind.Subtitle),
    val refreshing: Boolean = false,
    val needsRefresh: Boolean = false,
) {
    val changed: Boolean by lazy { selected != files.filter { it.selected }.map { it.index }.toSet() }
    val canSave: Boolean get() = ready && !loading && !busy && !needsRefresh && !filtersPending && filterError == null && selected.isNotEmpty() && (changed || reviewRequired)
    val hasFileProgress: Boolean by lazy { files.any { it.complete != null } }
    private val canManageFiles: Boolean get() = ready && !reviewRequired && !busy && !needsRefresh &&
        !changed && !filtersPending && filterError == null
    val finishPlan: FinishFilesPlan? by lazy {
        if (!canManageFiles) return@lazy null
        val wanted = files.filter { it.selected }
        if (wanted.any { !it.isVerifiedComplete && !it.isVerifiedIncomplete }) return@lazy null
        val completed = wanted.filter { it.isVerifiedComplete }.map { it.index }.toSet()
        if (completed.isEmpty()) null else FinishFilesPlan(selected, completed)
    }

    private val incompleteSelection by lazy { files.filter { it.selected && it.isVerifiedIncomplete }.map { it.index }.toSet() }
    fun canSkip(index: Int): Boolean = canManageFiles && selected.size > 1 && index in incompleteSelection
}

data class FinishFilesPlan(val selected: Set<Int>, val completed: Set<Int>) {
    /** Additional verified files can safely join the same selection. Losing a reviewed file requires confirmation again. */
    fun stillAppliesTo(latest: FinishFilesPlan?): Boolean =
        latest != null && selected == latest.selected && latest.completed.containsAll(completed)
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
    private val finish: (suspend (String, FinishCompletedRequest) -> Unit)? = null,
    private val skip: (suspend (String, Int) -> Unit)? = null,
) {
    private val mutableState = MutableStateFlow<TorrentFilesState?>(null)
    val state: StateFlow<TorrentFilesState?> = mutableState.asStateFlow()
    private var loadJob: Job? = null
    private var expiryJob: Job? = null
    private var generation = 0L

    fun open(download: Download) {
        if (!download.canChooseTorrentFiles() || mutableState.value?.busy == true) return
        generation++
        loadJob?.cancel()
        expiryJob?.cancel()
        loadJob = null
        mutableState.value = TorrentFilesState(download)
        refresh()
    }

    fun refresh() = reload(message = null)

    private fun reload(message: String?) {
        val current = mutableState.value ?: return
        if (current.busy || current.refreshing) return
        generation++
        loadJob?.cancel()
        val requestGeneration = generation
        mutableState.value = current.copy(loading = !current.ready && (current.loading || current.needsRefresh), refreshing = true, error = message)
        loadJob = scope.launch {
            try {
                val manifest = load(current.download.id)
                if (generation == requestGeneration) {
                    val latest = mutableState.value ?: return@launch
                    publish(latest.withManifest(manifest).copy(error = message))
                }
            } catch (e: ApiException) {
                if (generation == requestGeneration) reportFailure(e, needsRefresh = true, priorMessage = message)
            }
        }
    }

    private fun TorrentFilesState.withManifest(manifest: TorrentFiles): TorrentFilesState = copy(
        loading = false, refreshing = false, needsRefresh = false, busy = false, error = null,
        ready = manifest.ready, files = manifest.files, reviewRequired = manifest.reviewRequired,
        draft = if (filtersEdited) draft else manifest.rules?.toDraft() ?: TorrentRuleDraft(),
        types = if (filtersEdited) types else manifest.rules?.fileTypes ?: types,
        selected = mergeSelection(manifest),
    )

    private fun publish(next: TorrentFilesState) {
        expiryJob?.cancel()
        mutableState.value = next
        if (next.ready && !next.reviewRequired && next.hasFileProgress) expiryJob = scope.launch {
            delay(PROGRESS_VALID_MS)
            mutableState.update { it?.copy(needsRefresh = true) }
        }
    }

    private fun reportFailure(error: ApiException, needsRefresh: Boolean, priorMessage: String? = null) {
        mutableState.update { current ->
            if (current == null) return@update null
            if (error.status in setOf(401, 403, 404)) {
                TorrentFilesState(current.download, loading = false, needsRefresh = true, error = error.message)
            } else current.copy(
                loading = false, refreshing = false, busy = false, needsRefresh = needsRefresh || current.needsRefresh,
                error = if (priorMessage == null) error.message else "$priorMessage ${error.message}",
            )
        }
    }

    private fun TorrentFilesState.mergeSelection(manifest: TorrentFiles): Set<Int> {
        val previous = files.filter { it.selected }.map { it.index }.toSet()
        return manifest.files.filter { file ->
            // Preserve local checkbox edits. Untouched files follow other clients' saved choices.
            if ((file.index in previous) != (file.index in selected)) file.index in selected else file.selected
        }.map { it.index }.toSet()
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

    fun type(value: TorrentFileKind) {
        mutableState.update { current -> current?.editFilters(current.draft, if (value in current.types) current.types - value else current.types + value) }
    }

    private fun TorrentFilesState.editFilters(value: TorrentRuleDraft, nextTypes: List<TorrentFileKind>): TorrentFilesState {
        if (!ready || loading || busy) return this
        return copy(draft = value.copy(enabled = true), types = nextTypes, filtersEdited = true, filtersPending = true, filterError = null, error = null)
    }

    fun filter() {
        mutableState.update { current ->
            if (current == null || !current.ready || current.loading || current.busy) return@update current
            if (current.types.isEmpty()) return@update current.copy(selected = emptySet(), filtersPending = false, filterError = "Choose at least one file type.")
            try {
                val rules = requireNotNull(current.draft.copy(enabled = true).rules(current.types))
                current.copy(selected = current.files.filter { rules.matches(it) }.map { it.index }.toSet(), filtersPending = false, filterError = null)
            } catch (e: IllegalArgumentException) { current.copy(filterError = e.message) }
        }
    }

    fun close() {
        if (mutableState.value?.busy == true) return
        generation++
        loadJob?.cancel()
        expiryJob?.cancel()
        mutableState.value = null
    }

    fun submit() {
        val current = mutableState.value ?: return
        if (!current.canSave) return
        mutate(current, keepOpen = current.reviewRequired) {
            if (current.reviewRequired) start(current.download.id, current.selected.sorted())
            else save(current.download.id, current.selected.sorted())
        }
    }

    fun finishCompleted(confirmed: FinishFilesPlan) {
        val finish = finish ?: return
        val current = mutableState.value ?: return
        if (!confirmed.stillAppliesTo(current.finishPlan)) return
        mutate(current, recheck = { confirmed.stillAppliesTo(it.finishPlan) }) {
            finish(current.download.id, FinishCompletedRequest(
                expectedSelected = confirmed.selected.sorted(),
                expectedComplete = confirmed.completed.sorted(),
            ))
        }
    }

    fun skipFile(index: Int) {
        val skip = skip ?: return
        val current = mutableState.value ?: return
        if (!current.canSkip(index)) return
        mutate(current, keepOpen = true, recheck = { it.canSkip(index) }) { skip(current.download.id, index) }
    }

    private fun mutate(
        current: TorrentFilesState,
        keepOpen: Boolean = false,
        recheck: ((TorrentFilesState) -> Boolean)? = null,
        action: suspend () -> Unit,
    ) {
        generation++
        loadJob?.cancel()
        if (recheck != null) expiryJob?.cancel()
        mutableState.value = current.copy(busy = true, refreshing = false, error = null)
        scope.launch {
            try {
                if (recheck != null) {
                    val manifest = withTimeoutOrNull(PROGRESS_VALID_MS) { load(current.download.id) } ?: throw ApiException.network()
                    val latest = current.withManifest(manifest)
                    if (!recheck(latest)) {
                        publish(latest.copy(error = "The file status changed. Review the latest progress and try again."))
                        return@launch
                    }
                }
                action()
                if (keepOpen) {
                    mutableState.value = if (current.reviewRequired) {
                        // Start committed the draft. The next read owns selection and live progress.
                        TorrentFilesState(
                            current.download, loading = false, ready = true, needsRefresh = true,
                            files = current.files.map { it.copy(selected = it.index in current.selected) },
                            selected = current.selected,
                        )
                    } else current.copy(busy = false, refreshing = false, needsRefresh = true)
                    refresh()
                } else {
                    expiryJob?.cancel()
                    mutableState.value = null
                }
                onSaved()
            } catch (e: ApiException) {
                reportFailure(e, needsRefresh = recheck != null)
                if (recheck != null && e.status == 409) reload(message = e.message)
            }
        }
    }
}
