package com.thekozugroup.popcornstash.feature.downloads

import com.thekozugroup.popcornstash.data.api.TorrentFileKind
import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.ApiException
import com.thekozugroup.popcornstash.data.api.Download
import com.thekozugroup.popcornstash.data.api.DownloadState
import com.thekozugroup.popcornstash.data.api.EventStream
import com.thekozugroup.popcornstash.data.api.Inbox
import com.thekozugroup.popcornstash.data.api.InboxAction
import com.thekozugroup.popcornstash.data.api.ItemDetail
import com.thekozugroup.popcornstash.data.api.Library
import com.thekozugroup.popcornstash.data.api.Role
import com.thekozugroup.popcornstash.data.api.StashEvent
import com.thekozugroup.popcornstash.data.api.User
import com.thekozugroup.popcornstash.data.api.VpnStatus
import com.thekozugroup.popcornstash.data.repo.Loadable
import com.thekozugroup.popcornstash.data.repo.StashData
import com.thekozugroup.popcornstash.share.UploadBatchProgress
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.joinAll
import kotlinx.coroutines.launch

/** What an import row is busy with. */
enum class RowBusy { Accept, Ignore }

/** One file in Ready to import (or under "Left where they are"), as the row shows it. */
data class ImportRow(
    val item: ItemDetail,
    val fileName: String,
    /** Why it waits, in the web's words; null when it only needs accepting. */
    val reason: String?,
    val choices: List<ImportTarget>,
    /** What the picker shows; Accept sends exactly this. Null: "Choose a library". */
    val target: ImportTarget?,
    /** A series needs a show and an episode first: Accept opens Edit ("Add details"). */
    val wantsDetails: Boolean,
    val busy: RowBusy?,
    val ignored: Boolean,
) {
    val ready: Boolean get() = target != null && !wantsDetails
}

/** A finished download and where it went. */
data class FinishedRow(val download: Download, val outcome: FinishedOutcome)

data class AddLinkState(
    val text: String = "",
    /** The field was filled from the clipboard when the sheet opened. */
    val fromClipboard: Boolean = false,
    val fileTypes: List<TorrentFileKind> = TorrentFileKind.Default,
    val error: String? = null,
    val busy: Boolean = false,
    val libraryId: String? = null,
)

data class EditState(
    val itemId: String,
    val form: MoveForm,
    val errors: Map<MoveField, String> = emptyMap(),
    /** A failure that names no field. */
    val error: String? = null,
    val busy: Boolean = false,
)

data class AcceptAllState(val total: Int, val done: Int = 0, val running: Boolean = false, val unchosen: Int = 0)

/** The Downloads tab, as the screen draws it. */
data class DownloadsUiState(
    val isOwner: Boolean = false,
    /** Nothing loaded yet: placeholder rows. */
    val loading: Boolean = true,
    /** Downloads could not be loaded and there is nothing to show. */
    val loadError: String? = null,
    val refreshing: Boolean = false,
    /** The VPN line; null while the server does not answer and its VPN status is unknown. */
    val vpn: VpnLine? = vpnLine(null, unavailable = false, isOwner = false),
    /** Torrents run without a VPN on this server (the add sheet's sentence changes). */
    val noVpnMode: Boolean = false,
    val connectionLost: Boolean = false,
    val uploads: List<UploadBatchProgress> = emptyList(),
    val inProgress: List<Download> = emptyList(),
    val busyDownloads: Set<String> = emptySet(),
    val imports: List<ImportRow> = emptyList(),
    val ignored: List<ImportRow> = emptyList(),
    /** The Inbox could not be loaded (the rest of the page still shows). */
    val importsError: String? = null,
    val finished: List<FinishedRow> = emptyList(),
    val moreFinished: Boolean = false,
    val removing: Download? = null,
    val removeBusy: Boolean = false,
    val add: AddLinkState? = null,
    val edit: EditState? = null,
    val acceptAll: AcceptAllState? = null,
    val destinationLibraries: List<Library> = emptyList(),
) {
    /** Files whose row is ready to accept as it is: what "Accept all" moves. */
    val readyToAccept: List<ImportRow> get() = imports.filter { it.ready }

    /** Nothing at all: no download, no upload, nothing to import. */
    val isEmpty: Boolean
        get() = !loading && loadError == null && inProgress.isEmpty() && uploads.isEmpty() && imports.isEmpty() &&
            ignored.isEmpty() && finished.isEmpty()

    /** The row being edited, while the sheet is open. */
    val editing: ImportRow? get() = edit?.let { e -> (imports + ignored).firstOrNull { it.item.id == e.itemId } }
}

/** What the person changed on this page, kept beside what the server says. */
private data class Local(
    /** Picker choices the person made, by item id (null: they cleared it). */
    val chosen: Map<String, String?> = emptyMap(),
    /** Files accepted or ignored here, hidden until the Inbox reloads without them. */
    val decided: Set<String> = emptySet(),
    val rowBusy: Map<String, RowBusy> = emptyMap(),
    val busyDownloads: Set<String> = emptySet(),
    val removing: Download? = null,
    val removeBusy: Boolean = false,
    val add: AddLinkState? = null,
    val edit: EditState? = null,
    val acceptAll: AcceptAllState? = null,
    val finishedShown: Int = FINISHED_PAGE,
    /** The VPN status was asked for at least once. */
    val vpnAsked: Boolean = false,
)

private const val FINISHED_PAGE = 20

/**
 * The Downloads tab (android/DESIGN.md, Downloads; docs/architecture.md section 10): what is
 * downloading, live from `download`, `vpn` and `inbox` events through [data]; the files ready
 * to import with the library each goes to; and finished downloads with where they went.
 *
 * Every accept names the place and library the row shows (section 8, Inbox accept), so a
 * file never moves somewhere the person did not see on screen.
 */
class DownloadsController(
    private val api: ApiClient,
    private val data: StashData,
    user: StateFlow<User>,
    connection: StateFlow<EventStream.Status>,
    uploads: StateFlow<List<UploadBatchProgress>>,
    private val onStopUpload: (String) -> Unit,
    private val scope: CoroutineScope,
) {
    private val local = MutableStateFlow(Local())
    private val _messages = Channel<String>(Channel.BUFFERED)

    /** One sentence for a snackbar after an action ("Moved to Kids."). */
    val messages: Flow<String> = _messages.receiveAsFlow()

    private val sources = combine(data.downloads, data.inbox, data.vpn, data.libraries, user) { downloads, inbox, vpn, libraries, person ->
        Sources(downloads, inbox, vpn, libraries, person)
    }

    val state: StateFlow<DownloadsUiState> =
        combine(sources, connection, uploads, local) { s, status, running, l -> build(s, status, running, l) }
            .stateIn(scope, SharingStarted.Eagerly, DownloadsUiState())

    private var lastStatus: EventStream.Status = connection.value
    private var acceptAllJob: Job? = null

    init {
        scope.launch {
            connection.collect { status ->
                // Back after a lost connection: events may have been missed, so read everything again.
                if (status == EventStream.Status.Connected && lastStatus == EventStream.Status.Waiting) refresh()
                lastStatus = status
            }
        }
    }

    /** Loads what the tab shows, in the background (when the tab first shows, after a lost connection). */
    fun refresh() {
        scope.launch { refreshNow() }
    }

    /** Loads everything again and returns when it is done (pull to refresh, "Try again"). */
    suspend fun refreshNow() {
        listOf(data.refreshDownloads(), data.refreshInbox(), data.refreshLibraries(), data.refreshVpn()).joinAll()
        local.update { it.copy(vpnAsked = true) }
    }

    // ---------------------------------------------------------------- downloads ----

    fun pauseOrResume(download: Download) {
        if (download.id in local.value.busyDownloads) return
        local.update { it.copy(busyDownloads = it.busyDownloads + download.id) }
        scope.launch {
            try {
                val next = if (download.canResume()) api.resumeDownload(download.id) else api.pauseDownload(download.id)
                data.apply(StashEvent.DownloadChanged(next))
            } catch (e: ApiException) {
                _messages.trySend(e.message)
            } finally {
                local.update { it.copy(busyDownloads = it.busyDownloads - download.id) }
            }
        }
    }

    /** A failed link or magnet: restart it in place, or add the same link again. */
    fun retry(download: Download) {
        if (download.id in local.value.busyDownloads) return
        local.update { it.copy(busyDownloads = it.busyDownloads + download.id) }
        scope.launch {
            try {
                data.apply(StashEvent.DownloadChanged(api.resumeDownload(download.id)))
            } catch (e: ApiException) {
                if (e.status !in setOf(400, 404, 409)) {
                    _messages.trySend(e.message)
                } else {
                    try {
                        data.apply(StashEvent.DownloadChanged(api.addDownload(download.source, libraryId = download.libraryId)))
                        runCatching { api.removeDownload(download.id, deleteFiles = false) }
                        data.apply(StashEvent.DownloadRemoved(download.id))
                    } catch (second: ApiException) {
                        _messages.trySend(second.message)
                    }
                }
            } finally {
                local.update { it.copy(busyDownloads = it.busyDownloads - download.id) }
            }
        }
    }

    fun askRemove(download: Download) = local.update { it.copy(removing = download) }

    fun dismissRemove() = local.update { if (it.removeBusy) it else it.copy(removing = null) }

    fun confirmRemove() {
        val download = local.value.removing ?: return
        if (local.value.removeBusy) return
        local.update { it.copy(removeBusy = true) }
        scope.launch {
            try {
                api.removeDownload(download.id, deleteFiles = removeDeletesFiles(download))
                data.apply(StashEvent.DownloadRemoved(download.id))
                _messages.trySend("Download removed.")
            } catch (e: ApiException) {
                _messages.trySend(e.message)
            } finally {
                local.update { it.copy(removing = null, removeBusy = false) }
            }
        }
    }

    fun showMoreFinished() = local.update { it.copy(finishedShown = it.finishedShown + FINISHED_PAGE) }

    fun stopUpload(batchId: String) {
        onStopUpload(batchId)
        _messages.trySend("Upload stopped.")
    }

    // ----------------------------------------------------------- add a link ----

    /** Opens the add sheet, filled with [clipboard] when it holds a link. */
    fun openAdd(clipboard: CharSequence?) {
        val link = findLink(clipboard)
        local.update { it.copy(add = AddLinkState(text = link?.url.orEmpty(), fromClipboard = link != null)) }
    }

    fun onAddLibrary(id: String?) = local.update { l -> l.copy(add = l.add?.takeIf { !it.busy }?.copy(libraryId = id, error = null) ?: l.add) }

    fun onAddFileType(kind: TorrentFileKind) = local.update { l ->
        val add = l.add ?: return@update l
        if (add.busy) l else l.copy(add = add.copy(fileTypes = if (kind in add.fileTypes) add.fileTypes - kind else add.fileTypes + kind))
    }

    fun onAddText(text: String) = local.update { l -> l.copy(add = l.add?.copy(text = text, fromClipboard = false, error = null)) }

    fun closeAdd() = local.update { if (it.add?.busy == true) it else it.copy(add = null) }

    fun submitAdd() {
        val add = local.value.add ?: return
        if (add.busy) return
        val link = when (val check = checkLink(add.text)) {
            LinkCheck.Empty -> {
                local.update { it.copy(add = add.copy(error = "Paste a web link or a magnet link.")) }
                return
            }
            is LinkCheck.Invalid -> {
                local.update { it.copy(add = add.copy(error = check.message)) }
                return
            }
            is LinkCheck.Ok -> check.link
        }
        local.update { it.copy(add = add.copy(busy = true, error = null)) }
        scope.launch {
            try {
                val download = api.addDownload(link.url, libraryId = add.libraryId, fileTypes = add.fileTypes.takeIf { link.kind != LinkKind.Http })
                data.apply(StashEvent.DownloadChanged(download))
                local.update { it.copy(add = null) }
                _messages.trySend("Added ${download.name}.")
            } catch (e: ApiException) {
                val message = if (e.code == "vpn_unavailable") {
                    "Torrents are turned off on this server. Web links and uploads still work."
                } else {
                    e.message
                }
                local.update { l -> l.copy(add = l.add?.copy(busy = false, error = message)) }
            }
        }
    }

    // --------------------------------------------------------------- imports ----

    fun choose(itemId: String, target: ImportTarget) = local.update { it.copy(chosen = it.chosen + (itemId to target.key)) }

    fun accept(row: ImportRow) {
        val target = row.target ?: return
        if (row.wantsDetails) {
            openEdit(row)
            return
        }
        if (local.value.rowBusy.isNotEmpty() || local.value.acceptAll?.running == true) return
        local.update { it.copy(rowBusy = it.rowBusy + (row.item.id to RowBusy.Accept)) }
        scope.launch {
            try {
                api.inboxAction(row.item.id, acceptAction(target))
                decided(row.item.id)
                _messages.trySend("Moved to ${target.name}.")
            } catch (e: ApiException) {
                _messages.trySend(e.message)
                // A target that changed meanwhile (a claim moved, a library was hidden): show what is true now.
                if (e.status == 404 || e.status == 409) data.refreshInbox()
            } finally {
                local.update { it.copy(rowBusy = it.rowBusy - row.item.id) }
            }
        }
    }

    fun ignore(row: ImportRow) {
        if (local.value.rowBusy.isNotEmpty() || local.value.acceptAll?.running == true) return
        local.update { it.copy(rowBusy = it.rowBusy + (row.item.id to RowBusy.Ignore)) }
        scope.launch {
            try {
                api.inboxAction(row.item.id, InboxAction(action = "ignore"))
                data.refreshInbox()
                _messages.trySend("Ignored. ${row.fileName} stays where it is.")
            } catch (e: ApiException) {
                _messages.trySend(e.message)
            } finally {
                local.update { it.copy(rowBusy = it.rowBusy - row.item.id) }
            }
        }
    }

    fun openEdit(row: ImportRow) = local.update { it.copy(edit = EditState(row.item.id, initialMoveForm(row.item, row.target))) }

    fun onEditForm(form: MoveForm) = local.update { l ->
        l.copy(edit = l.edit?.takeIf { !it.busy }?.copy(form = form, errors = emptyMap(), error = null) ?: l.edit)
    }

    fun closeEdit() = local.update { if (it.edit?.busy == true) it else it.copy(edit = null) }

    fun submitEdit() {
        val edit = local.value.edit ?: return
        if (edit.busy) return
        val row = state.value.editing ?: return
        val check = validateMove(edit.form, row.choices)
        val action = check.action
        if (action == null) {
            local.update { it.copy(edit = edit.copy(errors = check.errors)) }
            return
        }
        val target = row.choices.first { it.key == edit.form.targetKey }
        local.update { it.copy(edit = edit.copy(busy = true, errors = emptyMap(), error = null), chosen = it.chosen + (row.item.id to target.key)) }
        scope.launch {
            try {
                api.inboxAction(row.item.id, action)
                local.update { it.copy(edit = null) }
                decided(row.item.id)
                _messages.trySend("Moved to ${target.name}.")
            } catch (e: ApiException) {
                val field = fieldOfServerError(e.field)
                local.update { l ->
                    l.copy(
                        edit = l.edit?.copy(
                            busy = false,
                            errors = field?.let { mapOf(it to e.message) } ?: emptyMap(),
                            error = if (field == null) e.message else null,
                        ),
                    )
                }
            }
        }
    }

    fun askAcceptAll() {
        val current = state.value
        if (current.readyToAccept.size < 2) return
        local.update {
            it.copy(acceptAll = AcceptAllState(total = current.readyToAccept.size, unchosen = current.imports.size - current.readyToAccept.size))
        }
    }

    fun dismissAcceptAll() = local.update { if (it.acceptAll?.running == true) it else it.copy(acceptAll = null) }

    /** Accepts every ready row, oldest first, each to the place its row shows. */
    fun confirmAcceptAll() {
        if (acceptAllJob?.isActive == true) return
        val rows = state.value.readyToAccept
        if (rows.isEmpty()) return
        local.update { it.copy(acceptAll = AcceptAllState(total = rows.size, running = true)) }
        acceptAllJob = scope.launch {
            var moved = 0
            var failed = 0
            for (row in rows) {
                val target = row.target ?: continue
                try {
                    api.inboxAction(row.item.id, acceptAction(target))
                    moved++
                    local.update { it.copy(decided = it.decided + row.item.id) }
                } catch (_: ApiException) {
                    failed++
                }
                local.update { l -> l.copy(acceptAll = l.acceptAll?.copy(done = moved + failed)) }
            }
            local.update { it.copy(acceptAll = null) }
            data.refreshInbox()
            _messages.trySend(
                if (failed == 0) {
                    "${countOf(moved.toLong(), "file")} moved to the places shown."
                } else {
                    "${countOf(moved.toLong(), "file")} moved. ${countOf(failed.toLong(), "file")} did not move and " +
                        "${if (failed == 1) "is" else "are"} still here."
                },
            )
        }
    }

    private fun decided(itemId: String) {
        local.update { it.copy(decided = it.decided + itemId) }
        data.refreshInbox()
    }

    // ----------------------------------------------------------------- state ----

    private data class Sources(
        val downloads: Loadable<List<Download>>,
        val inbox: Loadable<Inbox>,
        val vpn: VpnStatus?,
        val libraries: Loadable<List<Library>>,
        val user: User,
    )

    private fun build(s: Sources, status: EventStream.Status, running: List<UploadBatchProgress>, l: Local): DownloadsUiState {
        val isOwner = s.user.role == Role.Owner
        val downloads = s.downloads.valueOrNull
        val libraries = s.libraries.valueOrNull.orEmpty()
        val inbox = s.inbox.valueOrNull
        fun row(item: ItemDetail, ignored: Boolean): ImportRow {
            val choices = targetChoices(item, libraries, isOwner)
            val chosen = item.id in l.chosen
            val target = rowTarget(choices, suggestedTarget(item, libraries), l.chosen[item.id], chosen)
            return ImportRow(
                item = item,
                fileName = item.relPath.substringAfterLast('/').ifBlank { item.title },
                reason = if (ignored) null else waitingReason(item.suggestion, isOwner),
                choices = choices,
                target = target,
                wantsDetails = target != null && needsDetails(target, item),
                busy = l.rowBusy[item.id],
                ignored = ignored,
            )
        }
        // Rows wait for the libraries: before they arrive a video would offer only Other files.
        val librariesReady = s.libraries.valueOrNull != null
        val librariesFailed = (s.libraries as? Loadable.Failed)?.takeIf { it.previous == null }?.message
        val loadError = (s.downloads as? Loadable.Failed)?.takeIf { it.previous == null }?.message
        // The server did not answer: the page failed to load, or the live events are cut off.
        val serverQuiet = loadError != null || status == EventStream.Status.Waiting
        val all = downloads.orEmpty()
        val finished = all.filterNot { it.isActive() }
            .sortedByDescending { it.completedAt ?: it.createdAt }
        return DownloadsUiState(
            destinationLibraries = libraries,
            isOwner = isOwner,
            loading = downloads == null && (s.downloads is Loadable.Idle || s.downloads is Loadable.Loading),
            loadError = loadError,
            refreshing = (s.downloads as? Loadable.Ready)?.refreshing == true || (s.inbox as? Loadable.Ready)?.refreshing == true,
            vpn = pageVpnLine(s.vpn, asked = l.vpnAsked, serverQuiet = serverQuiet, isOwner = isOwner),
            noVpnMode = isNoVpnMode(s.vpn),
            connectionLost = status == EventStream.Status.Waiting,
            uploads = running,
            inProgress = all.filter { it.isActive() }.sortedByDescending { it.createdAt },
            busyDownloads = l.busyDownloads,
            imports = if (!librariesReady) emptyList() else inbox?.items.orEmpty().filterNot { it.id in l.decided }.map { row(it, ignored = false) },
            ignored = if (!librariesReady) emptyList() else inbox?.ignored.orEmpty().filterNot { it.id in l.decided }.map { row(it, ignored = true) },
            importsError = (s.inbox as? Loadable.Failed)?.takeIf { it.previous == null }?.message
                ?: librariesFailed?.takeIf { inbox?.items.orEmpty().isNotEmpty() || inbox?.ignored.orEmpty().isNotEmpty() },
            finished = finished.take(l.finishedShown).map { FinishedRow(it, it.finishedOutcome(libraries, isOwner)) },
            moreFinished = finished.size > l.finishedShown,
            removing = l.removing,
            removeBusy = l.removeBusy,
            add = l.add,
            edit = l.edit,
            acceptAll = l.acceptAll,
        )
    }
}

/**
 * Whether removing [download] also deletes what it downloaded. While it is in progress (or
 * failed), what arrived so far goes; sorted files already live in a library and never do.
 */
fun removeDeletesFiles(download: Download): Boolean = download.isActive() || download.state == DownloadState.Failed

/** Removing is offered except while a finished download waits to be sorted (the server answers 409 then). */
fun canRemove(download: Download): Boolean =
    download.state != DownloadState.Completed && download.state != DownloadState.Sorting

/** The confirm dialog's sentence for removing [download]. */
fun removeExplanation(download: Download): String = when {
    removeDeletesFiles(download) -> "What was downloaded so far is deleted."
    download.state == DownloadState.Sorted -> "It leaves this list. The sorted files stay in your library."
    else -> "It leaves this list."
}
