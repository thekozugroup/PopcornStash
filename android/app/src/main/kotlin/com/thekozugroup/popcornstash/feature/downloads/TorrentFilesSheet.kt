package com.thekozugroup.popcornstash.feature.downloads

import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.SheetValue
import androidx.compose.material3.Text
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import com.thekozugroup.popcornstash.ui.components.choiceSegmentColors
import com.thekozugroup.popcornstash.ui.components.ChoiceSegmentMark
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.foundation.layout.imePadding
import com.thekozugroup.popcornstash.data.api.TorrentFileKind
import com.thekozugroup.popcornstash.data.api.TorrentFile
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.repeatOnLifecycle
import kotlinx.coroutines.delay
import androidx.compose.ui.Modifier
import androidx.compose.ui.Alignment
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.dismiss
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.thekozugroup.popcornstash.feature.settings.CheckRow
import com.thekozugroup.popcornstash.feature.settings.FieldError
import com.thekozugroup.popcornstash.ui.components.PrimaryButton
import com.thekozugroup.popcornstash.ui.components.QuietButton
import com.thekozugroup.popcornstash.ui.components.ConfirmDialog
import com.thekozugroup.popcornstash.ui.theme.Popcorn

@Composable
internal fun TorrentFilesDialog(controller: TorrentFilesController) {
    val state by controller.state.collectAsStateWithLifecycle()
    state?.let { manifest ->
        TorrentFilesSheet(
            manifest, onToggle = controller::toggle, onSave = controller::submit,
            onCancel = controller::close, onRetry = controller::refresh, onDraft = controller::draft,
            onType = controller::type, onFilter = controller::filter,
            onFinish = controller::finishCompleted, onSkip = controller::skipFile,
        )
    }
}

/** A bounded, lazy list keeps large torrents usable and the confirmation action visible. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TorrentFilesSheet(
    state: TorrentFilesState,
    onToggle: (Int) -> Unit,
    onSave: () -> Unit,
    onCancel: () -> Unit,
    onRetry: () -> Unit,
    onDraft: (TorrentRuleDraft) -> Unit = {},
    onType: (TorrentFileKind) -> Unit = {},
    onFilter: () -> Unit = {},
    onFinish: (FinishFilesPlan) -> Unit = {},
    onSkip: (Int) -> Unit = {},
) {
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    val refresh by rememberUpdatedState(onRetry)
    val busy by rememberUpdatedState(state.busy)
    val poll = !state.busy && state.error == null && (!state.ready || (!state.reviewRequired && state.hasFileProgress))
    LaunchedEffect(state.download.id, lifecycle, poll) {
        if (poll) lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
            while (true) {
                delay(5_000)
                refresh()
            }
        }
    }
    ModalBottomSheet(
        onDismissRequest = { if (!busy) onCancel() },
        // Give the file list the entire vertical gesture, including the fling at its edges.
        sheetGesturesEnabled = false,
        dragHandle = null,
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true, confirmValueChange = { it != SheetValue.Hidden || !busy }),
        containerColor = Popcorn.colors.surface,
        contentColor = Popcorn.colors.text,
        shape = Popcorn.shapes.sheet,
        scrimColor = Popcorn.colors.stage.copy(alpha = 0.4f),
    ) {
        TorrentFilesContent(
            state, onToggle, onSave, onCancel, onRetry,
            modifier = Modifier.semantics {
                if (!busy) dismiss { onCancel(); true }
            },
            onDraft = onDraft, onType = onType, onFilter = onFilter, onFinish = onFinish, onSkip = onSkip,
        )
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun TorrentFilesContent(
    state: TorrentFilesState,
    onToggle: (Int) -> Unit,
    onSave: () -> Unit,
    onCancel: () -> Unit,
    onRetry: () -> Unit,
    modifier: Modifier = Modifier,
    onDraft: (TorrentRuleDraft) -> Unit = {},
    onType: (TorrentFileKind) -> Unit = {},
    onFilter: () -> Unit = {},
    onFinish: (FinishFilesPlan) -> Unit = {},
    onSkip: (Int) -> Unit = {},
) {
    val largeText = LocalDensity.current.fontScale > 1.3f
    val containerHeight = LocalWindowInfo.current.containerSize.height
    val maxHeight = with(LocalDensity.current) { (containerHeight * 0.85f).toDp() }
    var search by rememberSaveable(state.download.id, state.reviewRequired) { mutableStateOf("") }
    var editing by rememberSaveable(state.download.id, state.reviewRequired) { mutableStateOf(false) }
    val managing = state.hasFileProgress && !state.reviewRequired && !editing && !state.changed && !state.filtersEdited
    var confirmFiles by remember(state.download.id) { mutableStateOf<FinishFilesPlan?>(null) }
    var skipIndex by remember(state.download.id) { mutableStateOf<Int?>(null) }
    LaunchedEffect(state.finishPlan) {
        if (confirmFiles?.stillAppliesTo(state.finishPlan) == false) confirmFiles = null
    }
    confirmFiles?.takeIf { it.stillAppliesTo(state.finishPlan) }?.let { plan ->
        ConfirmDialog(
            title = "Finish completed files?",
            confirm = "Finish completed",
            onConfirm = { confirmFiles = null; onFinish(plan) },
            onDismiss = { confirmFiles = null },
            destructive = true,
        ) {
            val total = plan.selected.size
            val count = if (total == 1) "1 selected file" else "$total selected files"
            val complete = plan.completed.size
            Text("$complete of $count verified. Finishing stops downloading and sharing. The server will sort the selected files it verifies as complete. Skipped and unfinished files will be discarded.")
        }
    }
    skipIndex?.let { index ->
        val file = state.files.firstOrNull { it.index == index }
        if (file != null) ConfirmDialog(
            title = "Skip this file?",
            confirm = "Skip file",
            onConfirm = { skipIndex = null; onSkip(index) },
            onDismiss = { skipIndex = null },
            confirmEnabled = state.canSkip(index),
            destructive = true,
        ) {
            Column(verticalArrangement = Arrangement.spacedBy(Popcorn.spacing.s)) {
                Text(breakableName(file.path), maxLines = 2, overflow = TextOverflow.Ellipsis)
                Text(if (state.canSkip(index)) "Stop requesting this file and exclude it from sorting. Other file choices stay in effect."
                     else "The selection changed. Cancel and review the latest files.")
            }
        }
    }
    Column(
        modifier.fillMaxWidth()
            .heightIn(max = maxHeight)
            .windowInsetsPadding(WindowInsets.navigationBars).imePadding()
            .padding(horizontal = Popcorn.spacing.gutter, vertical = Popcorn.spacing.m)
            .testTag("torrent files"),
    ) {
        Text(if (state.reviewRequired) "Review torrent" else if (managing) "Torrent files" else "Choose files", style = Popcorn.type.title, color = Popcorn.colors.text, modifier = Modifier.semantics { heading() })
        Spacer(Modifier.height(Popcorn.spacing.xs))
        Text(state.download.name, style = Popcorn.type.item, color = Popcorn.colors.text, maxLines = 2, overflow = TextOverflow.Ellipsis)
        Spacer(Modifier.height(Popcorn.spacing.s))
        if (state.reviewRequired) Text("File contents stay stopped until you start.", style = Popcorn.type.metadata, color = Popcorn.colors.textMuted)
        when {
            state.loading -> Column(verticalArrangement = Arrangement.spacedBy(Popcorn.spacing.s)) {
                Text("Loading files…", style = Popcorn.type.metadata, color = Popcorn.colors.textMuted)
                repeat(3) { Box(Modifier.fillMaxWidth().height(48.dp).background(Popcorn.colors.subtle, Popcorn.shapes.control)) }
            }
            !state.ready && state.error != null -> Unit
            !state.ready -> {
                Text("Fetching torrent details. Files will appear when a peer shares them.", style = Popcorn.type.body, color = Popcorn.colors.textMuted)
                QuietButton("Check again", onClick = onRetry)
            }
            state.files.isEmpty() -> {
                Text("No files are available to choose.", style = Popcorn.type.body, color = Popcorn.colors.textMuted)
                QuietButton("Check again", onClick = onRetry)
            }
            else -> {
                val bytes = remember(state.files, state.selected) {
                    state.files.sumOf { if (it.index in state.selected) it.size else 0L }
                }
                val visible = remember(state.files, search) {
                    state.files.filter { it.path.contains(search, ignoreCase = true) }
                }
                Text("${state.selected.size} of ${state.files.size} selected · ${formatBytes(bytes)}", style = Popcorn.type.metadata, color = Popcorn.colors.textMuted)
                Spacer(Modifier.height(Popcorn.spacing.s))
                LazyColumn(Modifier.weight(1f, fill = false).testTag("torrent file list")) {
                    item(key = "filters") {
                        if (!state.reviewRequired) {
                            FlowRow(horizontalArrangement = Arrangement.spacedBy(Popcorn.spacing.s)) {
                                if (managing) QuietButton("Edit selection", onClick = { editing = true }, enabled = !state.busy)
                                QuietButton(if (state.refreshing) "Refreshing…" else "Refresh", onClick = onRetry, enabled = !state.busy && !state.refreshing)
                                if (managing) state.finishPlan?.let { plan ->
                                    QuietButton("Finish completed", onClick = { confirmFiles = plan })
                                }
                            }
                            if (!managing) Text("Uncheck files to skip them, then save. Only selected files will be imported.", style = Popcorn.type.metadata, color = Popcorn.colors.textMuted)
                        }
                        if (!managing) Column(verticalArrangement = Arrangement.spacedBy(Popcorn.spacing.s)) {
                            Text("File types", style = Popcorn.type.metadata, color = Popcorn.colors.textMuted)
                            TorrentFileKind.entries.chunked(if (largeText) 1 else 2).forEach { pair ->
                                Row(horizontalArrangement = Arrangement.spacedBy(Popcorn.spacing.s)) {
                                    pair.forEach { kind -> CheckRow(kind.label, kind in state.types, onToggle = { onType(kind) }, enabled = !state.busy, modifier = Modifier.weight(1f)) }
                                }
                            }
                            SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
                                listOf("MB", "GB").forEachIndexed { index, label ->
                                    val selected = state.draft.gigabytes == (index == 1)
                                    SegmentedButton(selected = selected, onClick = { onDraft(state.draft.copy(gigabytes = index == 1)) }, enabled = !state.busy,
                                        shape = SegmentedButtonDefaults.itemShape(index, 2, RoundedCornerShape(8.dp)),
                                        colors = choiceSegmentColors(Popcorn.colors.surface), icon = { ChoiceSegmentMark(selected) }, label = { Text(label) })
                                }
                            }
                            val unit = if (state.draft.gigabytes) "GB" else "MB"
                            if (largeText) {
                                RuleField("Minimum ($unit)", state.draft.minimum, { onDraft(state.draft.copy(minimum = it)) }, !state.busy, decimal = true)
                                RuleField("Maximum ($unit)", state.draft.maximum, { onDraft(state.draft.copy(maximum = it)) }, !state.busy, decimal = true)
                            } else Row(horizontalArrangement = Arrangement.spacedBy(Popcorn.spacing.s)) {
                                Box(Modifier.weight(1f)) { RuleField("Minimum ($unit)", state.draft.minimum, { onDraft(state.draft.copy(minimum = it)) }, !state.busy, decimal = true) }
                                Box(Modifier.weight(1f)) { RuleField("Maximum ($unit)", state.draft.maximum, { onDraft(state.draft.copy(maximum = it)) }, !state.busy, decimal = true) }
                            }
                            RuleField("Name contains", state.draft.include, { onDraft(state.draft.copy(include = it)) }, !state.busy)
                            RuleField("Name excludes", state.draft.exclude, { onDraft(state.draft.copy(exclude = it)) }, !state.busy)
                            Text("Choose filters, then press Filter. Adjust individual files below. Separate alternative names with commas; blank sizes have no limit.", style = Popcorn.type.metadata, color = Popcorn.colors.textMuted)
                            QuietButton("Filter", onClick = onFilter, enabled = !state.busy)
                            if (state.filtersPending) Text("Filters changed · selection unchanged", style = Popcorn.type.metadata, color = Popcorn.colors.textMuted)
                            FieldError(state.filterError)
                        }
                        RuleField("Find files in this torrent", search, { search = it }, !state.busy)
                    }
                    if (visible.isEmpty()) item { Text("No matching files.", style = Popcorn.type.body, color = Popcorn.colors.textMuted) }
                    items(visible, key = { it.index }, contentType = { managing }) { file ->
                        if (managing) {
                            TorrentFileProgressRow(file, canSkip = state.canSkip(file.index), onSkip = { skipIndex = file.index })
                        } else CheckRow(
                            label = breakableName(file.path),
                            checked = file.index in state.selected,
                            onToggle = { onToggle(file.index) },
                            description = if (state.reviewRequired) "${file.kind.label} · ${formatBytes(file.size)}"
                                else file.transferDescription(file.index in state.selected),
                            enabled = !state.busy,
                            modifier = Modifier.testTag("torrent file ${file.index}"),
                        )
                    }
                }
                if (state.selected.isEmpty()) FieldError("Select at least one file.")
            }
        }
        FieldError(state.error)
        if ((state.error != null || state.needsRefresh) && !state.loading && !state.refreshing) QuietButton("Refresh files", onClick = onRetry, enabled = !state.busy)
        Spacer(Modifier.height(Popcorn.spacing.m))
        if (!managing) PrimaryButton(if (state.reviewRequired) "Start download" else "Save", onClick = onSave, enabled = state.canSave, busy = state.busy, modifier = Modifier.fillMaxWidth())
        QuietButton(if (managing) "Close" else "Cancel", onClick = onCancel, enabled = !state.busy, modifier = Modifier.fillMaxWidth())
    }
}

@Composable
private fun TorrentFileProgressRow(file: TorrentFile, canSkip: Boolean, onSkip: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().heightIn(min = 56.dp).padding(vertical = Popcorn.spacing.xs),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Popcorn.spacing.s),
    ) {
        Column(Modifier.weight(1f).semantics(mergeDescendants = true) {}) {
            Text(breakableName(file.path), maxLines = 2, overflow = TextOverflow.Ellipsis, style = Popcorn.type.item, color = Popcorn.colors.text)
            Text(file.transferDescription(file.selected), style = Popcorn.type.metadata, color = Popcorn.colors.textMuted)
        }
        if (file.selected && file.isVerifiedIncomplete) QuietButton(
            "Skip", onClick = onSkip, enabled = canSkip,
            modifier = Modifier.semantics { contentDescription = "Skip ${file.path}" },
        )
    }
}

internal fun TorrentFile.transferDescription(wanted: Boolean): String {
    val status = when {
        !wanted && selected -> "Will skip"
        !wanted -> "Skipped"
        !selected -> "Will download"
        isVerifiedComplete -> "Complete"
        verifiedBytes == null || verifiedBytes !in 0..size -> "Progress unavailable"
        verifiedBytes == size -> "Verifying"
        verifiedBytes == 0L -> "Waiting"
        else -> "Incomplete"
    }
    val done = verifiedBytes?.takeIf { it in 0..size }
    val progress = when {
        done == null -> formatBytes(size)
        isVerifiedComplete -> "100% · ${formatBytes(size)}"
        done == size -> formatBytes(size)
        else -> "${formatPercent(done.toDouble() / size)} · ${formatBytesOf(done, size)}"
    }
    return "$status · $progress"
}
