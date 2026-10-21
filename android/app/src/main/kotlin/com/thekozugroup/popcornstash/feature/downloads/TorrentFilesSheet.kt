package com.thekozugroup.popcornstash.feature.downloads

import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
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
import androidx.compose.material3.Text
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import com.thekozugroup.popcornstash.ui.components.choiceSegmentColors
import com.thekozugroup.popcornstash.ui.components.ChoiceSegmentMark
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.foundation.layout.imePadding
import com.thekozugroup.popcornstash.data.api.TorrentFileKind
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.thekozugroup.popcornstash.feature.settings.CheckRow
import com.thekozugroup.popcornstash.feature.settings.FieldError
import com.thekozugroup.popcornstash.ui.components.PrimaryButton
import com.thekozugroup.popcornstash.ui.components.QuietButton
import com.thekozugroup.popcornstash.ui.theme.Popcorn

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
) {
    ModalBottomSheet(
        onDismissRequest = onCancel,
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
        containerColor = Popcorn.colors.surface,
        contentColor = Popcorn.colors.text,
        shape = Popcorn.shapes.sheet,
        scrimColor = Popcorn.colors.stage.copy(alpha = 0.4f),
    ) {
        TorrentFilesContent(state, onToggle, onSave, onCancel, onRetry, onDraft = onDraft, onType = onType, onFilter = onFilter)
    }
}

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
) {
    val largeText = LocalDensity.current.fontScale > 1.3f
    val containerHeight = LocalWindowInfo.current.containerSize.height
    val maxHeight = with(LocalDensity.current) { (containerHeight * 0.85f).toDp() }
    var search by remember(state.download.id) { mutableStateOf("") }
    Column(
        modifier.fillMaxWidth()
            .heightIn(max = maxHeight)
            .windowInsetsPadding(WindowInsets.navigationBars).imePadding()
            .padding(horizontal = Popcorn.spacing.gutter, vertical = Popcorn.spacing.m)
            .testTag("torrent files"),
    ) {
        Text(if (state.reviewRequired) "Review torrent" else "Choose files", style = Popcorn.type.title, color = Popcorn.colors.text, modifier = Modifier.semantics { heading() })
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
                val bytes = state.files.filter { it.index in state.selected }.sumOf { it.size }
                Text("${state.selected.size} of ${state.files.size} selected · ${formatBytes(bytes)}", style = Popcorn.type.metadata, color = Popcorn.colors.textMuted)
                Spacer(Modifier.height(Popcorn.spacing.s))
                LazyColumn(Modifier.weight(1f, fill = false).testTag("torrent file list")) {
                    item(key = "filters") {
                        Column(verticalArrangement = Arrangement.spacedBy(Popcorn.spacing.s)) {
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
                    items(state.files.filter { it.path.contains(search, ignoreCase = true) }, key = { it.index }) { file ->
                        CheckRow(
                            label = breakableName(file.path),
                            checked = file.index in state.selected,
                            onToggle = { onToggle(file.index) },
                            description = "${file.kind.label} · ${formatBytes(file.size)}",
                            enabled = !state.busy,
                            modifier = Modifier.testTag("torrent file ${file.index}"),
                        )
                    }
                }
                if (state.selected.isEmpty()) FieldError("Select at least one file.")
            }
        }
        FieldError(state.error)
        if (state.error != null && state.files.isEmpty() && !state.loading) QuietButton("Try again", onClick = onRetry)
        Spacer(Modifier.height(Popcorn.spacing.m))
        PrimaryButton(if (state.reviewRequired) "Start download" else "Save", onClick = onSave, enabled = state.canSave, busy = state.busy, modifier = Modifier.fillMaxWidth())
        QuietButton("Cancel", onClick = onCancel, enabled = !state.busy, modifier = Modifier.fillMaxWidth())
    }
}
