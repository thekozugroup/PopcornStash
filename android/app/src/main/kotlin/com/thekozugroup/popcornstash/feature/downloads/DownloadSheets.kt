package com.thekozugroup.popcornstash.feature.downloads

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.thekozugroup.popcornstash.data.api.Category
import com.thekozugroup.popcornstash.data.api.Download
import com.thekozugroup.popcornstash.data.api.Structure
import com.thekozugroup.popcornstash.ui.components.ChoiceSegmentMark
import com.thekozugroup.popcornstash.ui.components.ConfirmDialog
import com.thekozugroup.popcornstash.ui.components.choiceSegmentColors
import com.thekozugroup.popcornstash.ui.components.PrimaryButton
import com.thekozugroup.popcornstash.ui.components.QuietButton
import com.thekozugroup.popcornstash.ui.icons.LucideIcon
import com.thekozugroup.popcornstash.ui.icons.PopcornIcons
import com.thekozugroup.popcornstash.ui.theme.Popcorn

/** A Popcorn bottom sheet: `surface`, 16dp top corners, the content clear of the keyboard and the navigation bar. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DownloadsSheet(onDismiss: () -> Unit, tag: String, content: @Composable ColumnScope.() -> Unit) {
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
        containerColor = Popcorn.colors.surface,
        contentColor = Popcorn.colors.text,
        shape = Popcorn.shapes.sheet,
        scrimColor = Popcorn.colors.stage.copy(alpha = 0.4f),
    ) {
        SheetBody(tag, content)
    }
}

/** The sheet's content column, also drawn on its own by the screenshot tests. */
@Composable
fun SheetBody(tag: String, content: @Composable ColumnScope.() -> Unit) {
    Column(
        Modifier
            .fillMaxWidth()
            .windowInsetsPadding(WindowInsets.navigationBars)
            .imePadding()
            .verticalScroll(rememberScrollState())
            .padding(start = Popcorn.spacing.gutter, end = Popcorn.spacing.gutter, top = Popcorn.spacing.xs, bottom = Popcorn.spacing.m)
            .testTag(tag),
        content = content,
    )
}

@Composable
private fun SheetTitle(text: String) {
    Text(
        text,
        style = Popcorn.type.title,
        color = Popcorn.colors.text,
        modifier = Modifier
            .padding(bottom = Popcorn.spacing.m)
            .semantics { heading() },
    )
}

@Composable
fun popcornFieldColors() = OutlinedTextFieldDefaults.colors(
    focusedBorderColor = Popcorn.colors.accent,
    unfocusedBorderColor = Popcorn.colors.fieldOutline,
    focusedLabelColor = Popcorn.colors.text,
    unfocusedLabelColor = Popcorn.colors.textMuted,
    cursorColor = Popcorn.colors.text,
    focusedTextColor = Popcorn.colors.text,
    unfocusedTextColor = Popcorn.colors.text,
    focusedPlaceholderColor = Popcorn.colors.textMuted,
    unfocusedPlaceholderColor = Popcorn.colors.textMuted,
    focusedSupportingTextColor = Popcorn.colors.textMuted,
    unfocusedSupportingTextColor = Popcorn.colors.textMuted,
    errorBorderColor = Popcorn.colors.error,
    errorLabelColor = Popcorn.colors.error,
    errorSupportingTextColor = Popcorn.colors.error,
    errorCursorColor = Popcorn.colors.error,
    disabledTextColor = Popcorn.colors.textMuted,
    disabledBorderColor = Popcorn.colors.divider,
    disabledLabelColor = Popcorn.colors.textMuted,
)

// ------------------------------------------------------------- add a link ----

/**
 * "Add a link": one "Link or magnet" field (URL keyboard, filled from the clipboard when it
 * holds a link) and "Add" (android/DESIGN.md, Downloads). Under the field, what the link is
 * ("Magnet link. Downloads as a torrent, only through the VPN."), or why it cannot be added.
 */
@Composable
fun AddLinkContent(state: AddLinkState, viaVpn: Boolean, onText: (String) -> Unit, onAdd: () -> Unit, onCancel: () -> Unit, libraries: List<com.thekozugroup.popcornstash.data.api.Library> = emptyList(), onLibrary: (String?) -> Unit = {}) {
    SheetTitle("Add a link")
    val check = checkLink(state.text)
    val helper = when {
        state.error != null -> state.error
        check is LinkCheck.Ok -> {
            val what = describeLink(check.link, viaVpn)
            when {
                state.fromClipboard -> "From your clipboard. $what"
                check.extracted -> "Uses the first link in what you pasted. $what"
                else -> what
            }
        }
        else -> "A web link (http or https) or a magnet link."
    }
    OutlinedTextField(
        value = state.text,
        onValueChange = onText,
        label = { Text("Link or magnet") },
        placeholder = { Text("https://… or magnet:?xt=…") },
        singleLine = true,
        enabled = !state.busy,
        isError = state.error != null,
        supportingText = { Text(helper, modifier = Modifier.semantics { if (state.error != null) liveRegion = LiveRegionMode.Polite }) },
        keyboardOptions = KeyboardOptions(
            keyboardType = KeyboardType.Uri,
            imeAction = ImeAction.Go,
            autoCorrectEnabled = false,
            capitalization = KeyboardCapitalization.None,
        ),
        keyboardActions = KeyboardActions(onGo = { onAdd() }),
        shape = Popcorn.shapes.control,
        colors = popcornFieldColors(),
        modifier = Modifier.fillMaxWidth().testTag("link field"),
    )
    Spacer(Modifier.height(Popcorn.spacing.l))
    com.thekozugroup.popcornstash.ui.components.DownloadDestinationPicker(libraries, state.libraryId, onLibrary, enabled = !state.busy)
    Spacer(Modifier.height(Popcorn.spacing.s))
    PrimaryButton("Add", onClick = onAdd, busy = state.busy, modifier = Modifier.fillMaxWidth())
    Spacer(Modifier.height(Popcorn.spacing.xxs))
    QuietButton("Cancel", onClick = onCancel, enabled = !state.busy, modifier = Modifier.fillMaxWidth())
}

// ------------------------------------------------------------------ edit ----

/**
 * Edit where a waiting file goes: the same library picker, the details that library needs
 * (a title and year for a movie; show, season and episode for a series), and the live
 * preview "Will move to Kids", the one place that says the destination in words.
 */
@Composable
fun EditImportContent(
    row: ImportRow,
    edit: EditState,
    isOwner: Boolean,
    onForm: (MoveForm) -> Unit,
    onSubmit: () -> Unit,
    onCancel: () -> Unit,
) {
    val form = edit.form
    val target = row.choices.firstOrNull { it.key == form.targetKey }
    val category = target?.let { categoryAt(it, form) }
    SheetTitle("Where does it go?")
    Row(verticalAlignment = Alignment.CenterVertically) {
        MediaThumb(row.item.thumb, glyphForKind(row.item.kind), size = 40.dp)
        Spacer(Modifier.width(Popcorn.spacing.s))
        Text(
            breakableName(row.fileName),
            style = Popcorn.type.item,
            color = Popcorn.colors.text,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
        )
    }
    Spacer(Modifier.height(Popcorn.spacing.m))
    FieldLabel("Library")
    LibraryPicker(
        choices = row.choices,
        selected = target,
        onChoose = { onForm(form.copy(targetKey = it.key)) },
        enabled = !edit.busy,
        fileName = row.fileName,
        error = edit.errors[MoveField.Target] != null,
    )
    edit.errors[MoveField.Target]?.let { FieldError(it) }

    if (target?.library?.structure == Structure.OneOffs) {
        Spacer(Modifier.height(Popcorn.spacing.m))
        FieldLabel("What is this file?")
        KindChoice(selected = if (category == Category.HomeVideo) Category.HomeVideo else Category.Movie, enabled = !edit.busy) {
            onForm(form.copy(category = it))
        }
    }

    when (category) {
        Category.Movie -> {
            Spacer(Modifier.height(Popcorn.spacing.m))
            Row(horizontalArrangement = Arrangement.spacedBy(Popcorn.spacing.s)) {
                Field("Title", form.title, edit.errors[MoveField.Title], !edit.busy, modifier = Modifier.weight(2f), tag = "title field") {
                    onForm(form.copy(title = it))
                }
                Field("Year", form.year, edit.errors[MoveField.Year], !edit.busy, numeric = true, placeholder = "1995", modifier = Modifier.weight(1f), tag = "year field") {
                    onForm(form.copy(year = it))
                }
            }
        }
        Category.Episode -> {
            Spacer(Modifier.height(Popcorn.spacing.m))
            Field("Show", form.show, edit.errors[MoveField.Show], !edit.busy, tag = "show field") { onForm(form.copy(show = it)) }
            Row(horizontalArrangement = Arrangement.spacedBy(Popcorn.spacing.s)) {
                Field("Season", form.season, edit.errors[MoveField.Season], !edit.busy, numeric = true, placeholder = "1", modifier = Modifier.weight(1f)) {
                    onForm(form.copy(season = it))
                }
                Field("Episode", form.episode, edit.errors[MoveField.Episode], !edit.busy, numeric = true, modifier = Modifier.weight(1f), tag = "episode field") {
                    onForm(form.copy(episode = it))
                }
            }
        }
        else -> Unit
    }

    Spacer(Modifier.height(Popcorn.spacing.m))
    Preview(target?.let { previewMove(it, form, row.item, showFolder = isOwner) }, edit.errors)
    if (edit.error != null) {
        Spacer(Modifier.height(Popcorn.spacing.s))
        Text(
            edit.error,
            style = Popcorn.type.body,
            color = Popcorn.colors.error,
            modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite },
        )
    }
    Spacer(Modifier.height(Popcorn.spacing.l))
    PrimaryButton("Move it there", onClick = onSubmit, busy = edit.busy, modifier = Modifier.fillMaxWidth())
    Spacer(Modifier.height(Popcorn.spacing.xxs))
    QuietButton("Cancel", onClick = onCancel, enabled = !edit.busy, modifier = Modifier.fillMaxWidth())
}

@Composable
private fun FieldLabel(text: String) {
    Text(text, style = Popcorn.type.metadata, color = Popcorn.colors.textMuted, modifier = Modifier.padding(bottom = Popcorn.spacing.xxs))
}

@Composable
private fun FieldError(text: String) {
    Text(
        text,
        style = Popcorn.type.metadata,
        color = Popcorn.colors.error,
        modifier = Modifier
            .padding(top = Popcorn.spacing.xxs)
            .semantics { liveRegion = LiveRegionMode.Polite },
    )
}

@Composable
private fun Field(
    label: String,
    value: String,
    error: String?,
    enabled: Boolean,
    modifier: Modifier = Modifier,
    numeric: Boolean = false,
    placeholder: String? = null,
    tag: String? = null,
    onChange: (String) -> Unit,
) {
    OutlinedTextField(
        value = value,
        onValueChange = onChange,
        label = { Text(label) },
        placeholder = placeholder?.let { { Text(it) } },
        singleLine = true,
        enabled = enabled,
        isError = error != null,
        supportingText = error?.let { { Text(it) } },
        keyboardOptions = KeyboardOptions(
            keyboardType = if (numeric) KeyboardType.Number else KeyboardType.Text,
            capitalization = if (numeric) KeyboardCapitalization.None else KeyboardCapitalization.Words,
            imeAction = ImeAction.Next,
        ),
        shape = Popcorn.shapes.control,
        colors = popcornFieldColors(),
        modifier = modifier.fillMaxWidth().then(if (tag != null) Modifier.testTag(tag) else Modifier),
    )
}

/** "A movie" or "A camera video", inside a One-offs library. */
@Composable
private fun KindChoice(selected: Category, enabled: Boolean, onSelect: (Category) -> Unit) {
    val options = listOf(Category.Movie to "A movie", Category.HomeVideo to "A camera video")
    SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
        options.forEachIndexed { index, (category, label) ->
            SegmentedButton(
                selected = selected == category,
                onClick = { onSelect(category) },
                enabled = enabled,
                shape = SegmentedButtonDefaults.itemShape(index, options.size, RoundedCornerShape(8.dp)),
                colors = choiceSegmentColors(Popcorn.colors.surface),
                icon = { ChoiceSegmentMark(selected == category) },
                label = { Text(label, style = Popcorn.type.item) },
            )
        }
    }
}

/**
 * The live preview, above a thin rule: "Will move to Kids", the folder for owners, and the new
 * name or what is missing (unless the field that is missing already says so under it).
 */
@Composable
private fun Preview(preview: MovePreview?, errors: Map<MoveField, String>) {
    Column(
        Modifier
            .fillMaxWidth()
            .semantics(mergeDescendants = true) { liveRegion = LiveRegionMode.Polite }
            .testTag("move preview"),
    ) {
        RowRule(start = 0.dp)
        Spacer(Modifier.height(Popcorn.spacing.s))
        Row(verticalAlignment = Alignment.Top) {
            LucideIcon(PopcornIcons.Folder, contentDescription = null, tint = Popcorn.colors.textMuted, size = 18.dp, modifier = Modifier.padding(top = 1.dp))
            Spacer(Modifier.width(Popcorn.spacing.xs))
            Column {
                Text(preview?.title ?: "Choose where it goes", style = Popcorn.type.item, color = Popcorn.colors.text)
                preview?.folder?.let { Text(it, style = Popcorn.type.metadata, color = Popcorn.colors.textMuted) }
                val last = if (preview?.hint != null) preview.hintBeside(errors) else preview?.fileName?.let { "Renamed to $it" }
                last?.let { Text(it, style = Popcorn.type.metadata, color = Popcorn.colors.textMuted) }
            }
        }
    }
}

// --------------------------------------------------------------- dialogs ----

/** "Remove this download?" with what happens to its files; Remove is destructive. */
@Composable
fun RemoveDownloadDialog(download: Download, busy: Boolean, onConfirm: () -> Unit, onDismiss: () -> Unit) {
    ConfirmDialog(
        title = "Remove this download?",
        confirm = if (busy) "Removing" else "Remove",
        onConfirm = onConfirm,
        onDismiss = onDismiss,
        destructive = true,
        enabled = !busy,
    ) {
        Column {
            Text(breakableName(download.name), style = Popcorn.type.item, color = Popcorn.colors.text)
            Spacer(Modifier.height(Popcorn.spacing.xs))
            Text(removeExplanation(download))
        }
    }
}

/** "Accept all 4 suggestions?", then "Moving 2 of 4". */
@Composable
fun AcceptAllDialog(state: AcceptAllState, onConfirm: () -> Unit, onDismiss: () -> Unit) {
    val body = if (state.running) {
        "Moving ${(state.done + 1).coerceAtMost(state.total)} of ${state.total}."
    } else {
        listOfNotNull(
            "Each file moves to the library shown in its row.",
            if (state.unchosen > 0) "Choose a library for ${countOf(state.unchosen.toLong(), "file")} afterwards." else null,
        ).joinToString(" ")
    }
    ConfirmDialog(
        title = "Accept all ${state.total} suggestions?",
        confirm = "Accept all",
        onConfirm = onConfirm,
        onDismiss = onDismiss,
        enabled = !state.running,
    ) {
        Text(body, modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite })
    }
}
