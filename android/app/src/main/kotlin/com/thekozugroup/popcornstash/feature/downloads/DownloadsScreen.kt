package com.thekozugroup.popcornstash.feature.downloads

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.wrapContentHeight
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Snackbar
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.material3.pulltorefresh.PullToRefreshDefaults
import androidx.compose.material3.pulltorefresh.rememberPullToRefreshState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import com.thekozugroup.popcornstash.data.api.Download
import com.thekozugroup.popcornstash.navigation.TabPage
import com.thekozugroup.popcornstash.share.UploadBatchProgress
import com.thekozugroup.popcornstash.ui.components.ConnectionLostRow
import com.thekozugroup.popcornstash.ui.components.EmptyState
import com.thekozugroup.popcornstash.ui.components.LoadFailed
import com.thekozugroup.popcornstash.ui.components.PageTopBar
import com.thekozugroup.popcornstash.ui.components.PrimaryButton
import com.thekozugroup.popcornstash.ui.components.QuietButton
import com.thekozugroup.popcornstash.ui.components.TopBarAction
import com.thekozugroup.popcornstash.ui.icons.PopcornIcons
import com.thekozugroup.popcornstash.ui.theme.Popcorn
import kotlinx.coroutines.launch

/** What the Downloads page's controls do; [DownloadsTab] connects them to the controller. */
class DownloadsActions(
    val onRefresh: suspend () -> Unit = {},
    val onOpenVpnSettings: (() -> Unit)? = null,
    val onPauseOrResume: (Download) -> Unit = {},
    val onAskRemove: (Download) -> Unit = {},
    val onConfirmRemove: () -> Unit = {},
    val onDismissRemove: () -> Unit = {},
    val onRetryDownload: (Download) -> Unit = {},
    val onShowMoreFinished: () -> Unit = {},
    val onStopUpload: (String) -> Unit = {},
    val onOpenAdd: () -> Unit = {},
    val onAddText: (String) -> Unit = {},
    val onSubmitAdd: () -> Unit = {},
    val onCloseAdd: () -> Unit = {},
    val onChoose: (String, ImportTarget) -> Unit = { _, _ -> },
    val onAccept: (ImportRow) -> Unit = {},
    val onEdit: (ImportRow) -> Unit = {},
    val onIgnore: (ImportRow) -> Unit = {},
    val onAskAcceptAll: () -> Unit = {},
    val onConfirmAcceptAll: () -> Unit = {},
    val onDismissAcceptAll: () -> Unit = {},
    val onEditForm: (MoveForm) -> Unit = {},
    val onSubmitEdit: () -> Unit = {},
    val onCloseEdit: () -> Unit = {},
    /** Opens a library (or Other files, with a null id) from a finished row; null: the line only says where. */
    val onOpenPlace: ((OutcomePlace) -> Unit)? = null,
)

/** One line of the page's list, so a finished row can scroll the list to Ready to import. */
private sealed class Entry(val key: String) {
    data object Connection : Entry("connection")
    data object Vpn : Entry("vpn")
    data object Placeholder : Entry("placeholder")
    data class Failed(val message: String) : Entry("failed")
    data object Empty : Entry("empty")
    data class InProgressHeader(val count: Int) : Entry("in progress")
    data class Upload(val upload: UploadBatchProgress, val ruled: Boolean) : Entry("upload ${upload.id}")
    data class Transfer(val download: Download, val ruled: Boolean) : Entry("download ${download.id}")
    data class ImportsHeader(val count: Int, val acceptAll: Boolean) : Entry("imports")
    data class ImportsFailed(val message: String) : Entry("imports failed")
    data class Import(val row: ImportRow, val first: Boolean, val ruled: Boolean) : Entry("import ${row.item.id}")
    data class IgnoredToggle(val count: Int) : Entry("ignored")
    data class Ignored(val row: ImportRow) : Entry("ignored ${row.item.id}")
    data object FinishedHeader : Entry("finished")
    data class Finished(val row: FinishedRow, val ruled: Boolean) : Entry("finished ${row.download.id}")
    data object More : Entry("more finished")
}

private fun entries(state: DownloadsUiState, ignoredOpen: Boolean): List<Entry> = buildList {
    if (state.connectionLost) add(Entry.Connection)
    add(Entry.Vpn)
    when {
        state.loading -> add(Entry.Placeholder)
        state.loadError != null -> add(Entry.Failed(state.loadError))
        state.isEmpty -> add(Entry.Empty)
        else -> {
            val inProgress = state.uploads.size + state.inProgress.size
            if (inProgress > 0) {
                add(Entry.InProgressHeader(inProgress))
                state.uploads.forEachIndexed { index, upload -> add(Entry.Upload(upload, ruled = index > 0)) }
                state.inProgress.forEachIndexed { index, download ->
                    add(Entry.Transfer(download, ruled = index > 0 || state.uploads.isNotEmpty()))
                }
            }
            if (state.imports.isNotEmpty() || state.ignored.isNotEmpty() || state.importsError != null) {
                add(Entry.ImportsHeader(state.imports.size, acceptAll = state.readyToAccept.size >= 2))
                state.importsError?.let { add(Entry.ImportsFailed(it)) }
                state.imports.forEachIndexed { index, row -> add(Entry.Import(row, first = index == 0, ruled = index > 0)) }
                if (state.ignored.isNotEmpty()) {
                    add(Entry.IgnoredToggle(state.ignored.size))
                    if (ignoredOpen) state.ignored.forEach { add(Entry.Ignored(it)) }
                }
            }
            if (state.finished.isNotEmpty()) {
                add(Entry.FinishedHeader)
                state.finished.forEachIndexed { index, row -> add(Entry.Finished(row, ruled = index > 0)) }
                if (state.moreFinished) add(Entry.More)
            }
        }
    }
}

/**
 * The Downloads tab's first page (android/DESIGN.md, Downloads): the VPN line, In progress,
 * Ready to import and Finished, with "Add a link" in the top bar. Pull down to load again.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DownloadsPage(
    page: TabPage,
    state: DownloadsUiState,
    actions: DownloadsActions,
    nowSeconds: Long,
    modifier: Modifier = Modifier,
    snackbar: SnackbarHostState = remember { SnackbarHostState() },
    listState: LazyListState = rememberLazyListState(),
) {
    val scope = rememberCoroutineScope()
    var handledReselections by rememberSaveable { mutableIntStateOf(page.reselections) }
    LaunchedEffect(page.reselections) {
        if (page.reselections != handledReselections) {
            handledReselections = page.reselections
            listState.animateScrollToItem(0)
        }
    }
    var ignoredOpen by rememberSaveable { mutableStateOf(false) }
    var pulling by remember { mutableStateOf(false) }
    val list = entries(state, ignoredOpen)
    val locked = state.acceptAll?.running == true || state.imports.any { it.busy != null } || state.ignored.any { it.busy != null }

    Box(modifier.fillMaxSize()) {
        Column(Modifier.fillMaxSize()) {
            PageTopBar(title = "Downloads", scrolled = listState.canScrollBackward) {
                TopBarAction(PopcornIcons.Add, "Add a link", actions.onOpenAdd)
            }
            val pullState = rememberPullToRefreshState()
            PullToRefreshBox(
                isRefreshing = pulling,
                onRefresh = {
                    scope.launch {
                        pulling = true
                        try {
                            actions.onRefresh()
                        } finally {
                            pulling = false
                        }
                    }
                },
                state = pullState,
                modifier = Modifier.fillMaxSize(),
                indicator = {
                    PullToRefreshDefaults.Indicator(
                        state = pullState,
                        isRefreshing = pulling,
                        containerColor = Popcorn.colors.surface,
                        color = Popcorn.colors.accent,
                        modifier = Modifier.align(Alignment.TopCenter),
                    )
                },
            ) {
                LazyColumn(
                    state = listState,
                    contentPadding = PaddingValues(bottom = page.bottomClearance),
                    modifier = Modifier.fillMaxSize().testTag("list ${page.tab.name}"),
                ) {
                    items(list, key = { it.key }, contentType = { it::class }) { entry ->
                        EntryView(
                            entry = entry,
                            state = state,
                            actions = actions,
                            nowSeconds = nowSeconds,
                            locked = locked,
                            ignoredOpen = ignoredOpen,
                            onToggleIgnored = { ignoredOpen = !ignoredOpen },
                            onRetryLoad = { scope.launch { actions.onRefresh() } },
                        ) {
                            val index = list.indexOfFirst { it is Entry.ImportsHeader }
                            if (index >= 0) scope.launch { listState.animateScrollToItem(index) }
                        }
                    }
                }
            }
        }
        SnackbarHost(
            snackbar,
            modifier = Modifier
                .align(Alignment.BottomCenter)
                .padding(bottom = page.bottomClearance - Popcorn.spacing.m),
        ) { data ->
            Snackbar(
                data,
                containerColor = Popcorn.colors.text,
                contentColor = Popcorn.colors.page,
                shape = Popcorn.shapes.control,
            )
        }
    }

    state.add?.let { add ->
        DownloadsSheet(onDismiss = actions.onCloseAdd, tag = "add link sheet") {
            AddLinkContent(add, viaVpn = !state.noVpnMode, onText = actions.onAddText, onAdd = actions.onSubmitAdd, onCancel = actions.onCloseAdd)
        }
    }
    val editing = state.editing
    if (state.edit != null && editing != null) {
        DownloadsSheet(onDismiss = actions.onCloseEdit, tag = "edit sheet") {
            EditImportContent(editing, state.edit, state.isOwner, actions.onEditForm, actions.onSubmitEdit, actions.onCloseEdit)
        }
    }
    state.removing?.let { download ->
        RemoveDownloadDialog(download, state.removeBusy, actions.onConfirmRemove, actions.onDismissRemove)
    }
    state.acceptAll?.let { AcceptAllDialog(it, actions.onConfirmAcceptAll, actions.onDismissAcceptAll) }
}

@Composable
private fun EntryView(
    entry: Entry,
    state: DownloadsUiState,
    actions: DownloadsActions,
    nowSeconds: Long,
    locked: Boolean,
    ignoredOpen: Boolean,
    onToggleIgnored: () -> Unit,
    onRetryLoad: () -> Unit,
    onShowWaiting: () -> Unit,
) {
    when (entry) {
        Entry.Connection -> ConnectionLostRow()
        Entry.Vpn -> VpnLineRow(state.vpn, actions.onOpenVpnSettings)
        Entry.Placeholder -> DownloadsPlaceholder()
        is Entry.Failed -> LoadFailed(entry.message, onRetry = onRetryLoad, title = "Downloads could not be loaded")
        Entry.Empty -> EmptyState(
            glyph = PopcornIcons.Downloads,
            title = "Nothing downloading",
            body = "Links and magnets you add, and files you share to PopcornStash, show up here.",
            modifier = Modifier
                .fillMaxWidth()
                .padding(top = 96.dp)
                .wrapContentHeight(),
            action = { PrimaryButton("Add a link", onClick = actions.onOpenAdd, glyph = PopcornIcons.Add) },
        )
        is Entry.InProgressHeader -> DownloadsSectionHeader("In progress", entry.count)
        is Entry.Upload -> {
            if (entry.ruled) RowRule()
            UploadRow(entry.upload, onStop = { actions.onStopUpload(entry.upload.id) })
        }
        is Entry.Transfer -> {
            if (entry.ruled) RowRule()
            DownloadRow(
                entry.download,
                busy = entry.download.id in state.busyDownloads,
                onPauseOrResume = { actions.onPauseOrResume(entry.download) },
                onRemove = { actions.onAskRemove(entry.download) },
            )
        }
        is Entry.ImportsHeader -> DownloadsSectionHeader("Ready to import", entry.count.takeIf { it > 0 }) {
            if (entry.acceptAll) QuietButton("Accept all", onClick = actions.onAskAcceptAll, enabled = !locked)
        }
        is Entry.ImportsFailed -> LoadFailed(entry.message, onRetry = onRetryLoad, title = "Files to import could not be loaded")
        is Entry.Import -> {
            if (entry.ruled) RowRule(start = Popcorn.spacing.gutter + 64.dp + Popcorn.spacing.m)
            ImportRowView(
                row = entry.row,
                first = entry.first,
                locked = locked,
                onChoose = { actions.onChoose(entry.row.item.id, it) },
                onAccept = { actions.onAccept(entry.row) },
                onEdit = { actions.onEdit(entry.row) },
                onIgnore = { actions.onIgnore(entry.row) },
            )
        }
        is Entry.IgnoredToggle -> {
            RowRule(start = 0.dp)
            IgnoredDisclosure(entry.count, open = ignoredOpen, onToggle = onToggleIgnored)
        }
        is Entry.Ignored -> {
            RowRule(start = Popcorn.spacing.gutter + 64.dp + Popcorn.spacing.m)
            ImportRowView(
                row = entry.row,
                first = false,
                locked = locked,
                onChoose = { actions.onChoose(entry.row.item.id, it) },
                onAccept = { actions.onAccept(entry.row) },
                onEdit = { actions.onEdit(entry.row) },
                onIgnore = {},
            )
        }
        Entry.FinishedHeader -> DownloadsSectionHeader("Finished", null)
        is Entry.Finished -> {
            if (entry.ruled) RowRule()
            FinishedRowView(
                row = entry.row,
                nowSeconds = nowSeconds,
                busy = entry.row.download.id in state.busyDownloads,
                onOpenPlace = actions.onOpenPlace,
                onShowWaiting = onShowWaiting,
                onRetry = { actions.onRetryDownload(entry.row.download) },
                onRemove = { actions.onAskRemove(entry.row.download) },
            )
        }
        Entry.More -> Box(Modifier.fillMaxWidth().padding(top = Popcorn.spacing.xs), contentAlignment = Alignment.Center) {
            QuietButton("Show more", onClick = actions.onShowMoreFinished)
        }
    }
}
