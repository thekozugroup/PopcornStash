package com.thekozugroup.popcornstash.feature.settings

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.relocation.BringIntoViewRequester
import androidx.compose.foundation.relocation.bringIntoViewRequester
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import com.thekozugroup.popcornstash.ui.components.PopcornMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExposedDropdownMenuAnchorType
import androidx.compose.material3.ExposedDropdownMenuBox
import androidx.compose.material3.MenuDefaults
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import com.thekozugroup.popcornstash.ui.components.ConfirmDialog
import com.thekozugroup.popcornstash.ui.components.IconPickerTokens
import androidx.compose.ui.unit.dp
import com.thekozugroup.popcornstash.data.api.Category
import com.thekozugroup.popcornstash.data.api.ContentType
import com.thekozugroup.popcornstash.data.api.FolderState
import com.thekozugroup.popcornstash.data.api.Structure
import com.thekozugroup.popcornstash.data.api.Visibility
import com.thekozugroup.popcornstash.ui.components.IconPicker
import com.thekozugroup.popcornstash.ui.components.PageTopBar
import com.thekozugroup.popcornstash.ui.components.PrimaryButton
import com.thekozugroup.popcornstash.ui.components.SecondaryButton
import com.thekozugroup.popcornstash.ui.icons.LibraryIcons
import com.thekozugroup.popcornstash.ui.icons.LucideIcon
import com.thekozugroup.popcornstash.ui.icons.PopcornIcons
import com.thekozugroup.popcornstash.ui.theme.Popcorn

/** What the library editor can be asked to do; the controller does it, the screenshots do nothing. */
interface LibraryEditorActions {
    fun load()
    fun setName(value: String)
    fun setIcon(icon: String)
    fun setType(contentType: ContentType, structure: Structure?)
    fun toggleClaim(kind: Category)
    fun setFolderMode(mode: FolderMode)
    fun setNewFolder(value: String)
    fun chooseFolder(name: String)
    fun setVisibility(visibility: Visibility)
    fun toggleMember(id: String)
    fun save()
    fun confirmClaims()
    fun cancelClaims()
    fun askDelete()
    fun cancelDelete()
    fun confirmDelete(removeFolder: Boolean = false)
}

/**
 * The library editor, pushed from Settings (android/DESIGN.md, Library editor): the web editor's
 * groups in the same order and words, as one scrolling form with the primary action at its end
 * and "Delete library" apart below it.
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun LibraryEditorPage(
    state: LibraryEditorUi,
    actions: LibraryEditorActions,
    onBack: () -> Unit,
    bottomClearance: Dp,
    onConversion: () -> Unit = {},
) {
    val scroll = rememberScrollState()
    // Read the scroll position only when it crosses the top, not on every scrolled pixel.
    val scrolled by remember { derivedStateOf { scroll.value > 0 } }
    val title = if (state.isNew) "New library" else state.library?.name ?: state.knownName ?: "Library"
    Column(Modifier.fillMaxSize().background(Popcorn.colors.page).testTag("library editor")) {
        PageTopBar(title = title, onBack = onBack, scrolled = scrolled)
        when (val load = state.load) {
            EditorLoad.Loading -> EditorPlaceholder()
            is EditorLoad.Failed -> EditorFailed(load, onRetry = actions::load, onBack = onBack)
            EditorLoad.Ready -> {
                val draft = state.draft ?: return@Column
                val requesters = remember { EditorField.entries.associateWith { BringIntoViewRequester() } }
                // A save the server refused: bring the field it names into view.
                LaunchedEffect(state.errors) {
                    val first = EditorField.entries.firstOrNull { it in state.errors } ?: return@LaunchedEffect
                    requesters.getValue(first).bringIntoView()
                }
                Column(
                    Modifier
                        .fillMaxSize()
                        .imePadding()
                        .verticalScroll(scroll)
                        .padding(horizontal = Popcorn.spacing.gutter)
                        .padding(bottom = bottomClearance),
                ) {
                    NameAndIcon(state, draft, actions, Modifier.bringIntoViewRequester(requesters.getValue(EditorField.Name)))
                    WhatItHolds(state, draft, actions, Modifier.bringIntoViewRequester(requesters.getValue(EditorField.Type)))
                    NewFiles(state, draft, actions, Modifier.bringIntoViewRequester(requesters.getValue(EditorField.Claims)))
                    Folder(state, draft, actions, Modifier.bringIntoViewRequester(requesters.getValue(EditorField.Folder)))
                    WhoSeesIt(state, draft, actions, Modifier.bringIntoViewRequester(requesters.getValue(EditorField.Visibility)))
                    if (!state.isNew && draft.contentType == ContentType.Video) {
                        FormSection("Video conversion", "Save space with verified AV1 or H.265 replacements.") {
                            SecondaryButton("Manage video conversion", onClick = onConversion, enabled = !state.busy)
                        }
                    }
                    SaveFooter(state, actions)
                    DeleteLibrary(state, actions)
                }
                Dialogs(state, draft, actions)
            }
        }
    }
}

@Composable
private fun NameAndIcon(state: LibraryEditorUi, draft: LibraryDraft, actions: LibraryEditorActions, modifier: Modifier) {
    FormSection("Name and icon", "How it looks on Home and in the web sidebar.", modifier, first = true) {
        val nameError = state.errors[EditorField.Name]
        OutlinedTextField(
            value = draft.name,
            onValueChange = actions::setName,
            label = { Text("Name") },
            singleLine = true,
            enabled = !state.busy,
            isError = nameError != null,
            supportingText = { Text(nameError ?: "Up to $LIBRARY_NAME_MAX characters.") },
            keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences, imeAction = ImeAction.Done),
            shape = Popcorn.shapes.control,
            colors = stashFieldColors(),
            modifier = Modifier.fillMaxWidth().testTag("library name"),
        )
        Spacer(Modifier.height(Popcorn.spacing.m))
        FieldLabel("Icon")
        Spacer(Modifier.height(Popcorn.spacing.xs))
        IconPicker(icons = state.icons, selected = draft.icon, onSelect = actions::setIcon, enabled = !state.busy)
        FieldError(state.errors[EditorField.Icon])
        Spacer(Modifier.height(Popcorn.spacing.l))
        Text("On Home", style = Popcorn.type.metadata, color = Popcorn.colors.textMuted)
        Spacer(Modifier.height(Popcorn.spacing.xs))
        HomeRowPreview(draft.icon, draft.name.trim().ifEmpty { "New library" })
    }
}

/** The library's header row on Home as it will look, on a quiet plate so it reads as a preview. */
@Composable
private fun HomeRowPreview(icon: String, name: String) {
    val colors = Popcorn.colors
    Row(
        Modifier
            .fillMaxWidth()
            .height(56.dp)
            .background(colors.subtle, Popcorn.shapes.control)
            .padding(horizontal = Popcorn.spacing.m)
            .clearAndSetSemantics { contentDescription = "Preview on Home: $name" },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        LucideIcon(LibraryIcons.glyph(icon), contentDescription = null, tint = colors.textMuted)
        Spacer(Modifier.width(Popcorn.spacing.s))
        Text(name, style = Popcorn.type.section, color = colors.text, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
        LucideIcon(PopcornIcons.Open, contentDescription = null, tint = colors.textMuted, size = 20.dp)
    }
}

@Composable
private fun WhatItHolds(state: LibraryEditorUi, draft: LibraryDraft, actions: LibraryEditorActions, modifier: Modifier) {
    val locked = state.locked
    val enabled = !locked && !state.busy
    FormSection(
        "What it holds",
        if (locked) {
            "The type can change only while the library is empty."
        } else {
            "Videos are shows with seasons and episodes, or films and clips that stand on their own."
        },
        modifier,
    ) {
        if (locked) {
            // What it holds is a fact now, not a choice: said in the page's text colour, like the folder.
            Column(Modifier.semantics(mergeDescendants = true) { }.testTag("holds fact")) {
                FieldLabel("Holds")
                Text(holdsLine(draft.contentType, draft.structure), style = Popcorn.type.body, color = Popcorn.colors.text)
            }
        } else {
            Column(Modifier.selectableGroup()) {
                RadioRow(
                    "Videos",
                    selected = draft.contentType == ContentType.Video,
                    onClick = { actions.setType(ContentType.Video, draft.structure ?: Structure.OneOffs) },
                    enabled = enabled,
                )
                RadioRow(
                    "Photos",
                    description = "Photos and the short clips phones take with them",
                    selected = draft.contentType == ContentType.Photos,
                    onClick = { actions.setType(ContentType.Photos, null) },
                    enabled = enabled,
                )
            }
            if (draft.contentType == ContentType.Video) {
                Spacer(Modifier.height(Popcorn.spacing.s))
                FieldLabel("Arranged as")
                Column(Modifier.selectableGroup()) {
                    RadioRow(
                        "Series",
                        description = "Shows with seasons and episodes",
                        selected = draft.structure == Structure.Episodes,
                        onClick = { actions.setType(ContentType.Video, Structure.Episodes) },
                        enabled = enabled,
                    )
                    RadioRow(
                        "One-offs",
                        description = "Films and clips that stand on their own",
                        selected = draft.structure != Structure.Episodes,
                        onClick = { actions.setType(ContentType.Video, Structure.OneOffs) },
                        enabled = enabled,
                    )
                }
            }
        }
        FieldError(state.errors[EditorField.Type])
    }
}

@Composable
private fun NewFiles(state: LibraryEditorUi, draft: LibraryDraft, actions: LibraryEditorActions, modifier: Modifier) {
    FormSection("New files", "Each kind of new file goes to one library at most. A kind no library takes waits in Inbox.", modifier) {
        FieldLabel("Sort these here")
        fittingClaims(draft.contentType, draft.structure).forEach { kind ->
            val checked = kind in draft.claims
            CheckRow(
                claimLabel(kind),
                checked = checked,
                onToggle = { actions.toggleClaim(kind) },
                description = claimDescription(state.holderOf(kind), state.library?.id, checked),
                enabled = !state.busy,
                modifier = Modifier.testTag("claim ${kind.wire}"),
            )
        }
        FieldError(state.errors[EditorField.Claims])
    }
}

@Composable
private fun Folder(state: LibraryEditorUi, draft: LibraryDraft, actions: LibraryEditorActions, modifier: Modifier) {
    FormSection(
        "Folder",
        if (state.isNew) {
            "Every library is one folder in your PopcornStash folder. Files already in a folder are never moved."
        } else {
            "A library keeps its folder for good."
        },
        modifier,
    ) {
        if (state.isNew) {
            if (state.foldersError != null) {
                FieldNote("The folders could not be listed. ${state.foldersError}", color = Popcorn.colors.warning)
                Spacer(Modifier.height(Popcorn.spacing.xs))
            }
            Column(Modifier.selectableGroup()) {
                RadioRow(
                    "Make a new folder",
                    selected = draft.folderMode == FolderMode.New,
                    onClick = { actions.setFolderMode(FolderMode.New) },
                    enabled = !state.busy,
                )
                RadioRow(
                    "Use a folder that is already there",
                    description = if (state.folders.isEmpty()) "Every top-level folder is already a library." else null,
                    selected = draft.folderMode == FolderMode.Existing,
                    onClick = { actions.setFolderMode(FolderMode.Existing) },
                    enabled = !state.busy && state.folders.isNotEmpty(),
                )
            }
            Spacer(Modifier.height(Popcorn.spacing.s))
            val folderError = state.errors[EditorField.Folder]
            if (draft.folderMode == FolderMode.New) {
                OutlinedTextField(
                    value = draft.newFolder,
                    onValueChange = actions::setNewFolder,
                    label = { Text("Folder name") },
                    singleLine = true,
                    enabled = !state.busy,
                    isError = folderError != null,
                    supportingText = folderError?.let { message -> { Text(message) } },
                    textStyle = Popcorn.type.body.copy(fontFamily = FontFamily.Monospace),
                    keyboardOptions = KeyboardOptions(autoCorrectEnabled = false, imeAction = ImeAction.Done),
                    shape = Popcorn.shapes.control,
                    colors = stashFieldColors(),
                    modifier = Modifier.fillMaxWidth().testTag("folder name"),
                )
            } else {
                FolderPicker(state, draft, actions, folderError)
                draft.existingFolder?.let { name ->
                    FieldNote(existingFolderSentence(name, state.chosenFolder, draft.contentType, draft.structure, draft.claims))
                }
            }
        } else {
            FolderFact(state)
        }
    }
}

/** The folders that are not a library yet, in a menu: each name as on the disk with what is in it. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun FolderPicker(state: LibraryEditorUi, draft: LibraryDraft, actions: LibraryEditorActions, error: String?) {
    var expanded by rememberSaveable { mutableStateOf(false) }
    val colors = Popcorn.colors
    ExposedDropdownMenuBox(expanded = expanded && !state.busy, onExpandedChange = { expanded = it && !state.busy }) {
        OutlinedTextField(
            value = draft.existingFolder?.let { "$it/" } ?: "",
            onValueChange = {},
            readOnly = true,
            enabled = !state.busy,
            label = { Text("Folder") },
            placeholder = { Text("Choose a folder") },
            isError = error != null,
            supportingText = error?.let { message -> { Text(message) } },
            trailingIcon = { LucideIcon(PopcornIcons.Expand, contentDescription = null, size = 20.dp) },
            textStyle = Popcorn.type.body.copy(fontFamily = FontFamily.Monospace),
            // The whole name shows: a long one wraps at its spaces instead of being cut off.
            singleLine = false,
            maxLines = 3,
            shape = Popcorn.shapes.control,
            colors = stashFieldColors(),
            modifier = Modifier
                .fillMaxWidth()
                .testTag("folder picker")
                .menuAnchor(ExposedDropdownMenuAnchorType.PrimaryNotEditable, enabled = !state.busy),
        )
        ExposedDropdownMenu(
            expanded = expanded && !state.busy,
            onDismissRequest = { expanded = false },
            shape = Popcorn.shapes.panel,
            containerColor = colors.surface,
            border = BorderStroke(1.dp, colors.divider),
            shadowElevation = 4.dp,
        ) {
            state.folders.forEach { folder ->
                PopcornMenuItem(
                    text = {
                        Column(Modifier.padding(vertical = Popcorn.spacing.xxs)) {
                            Text("${folder.name}/", style = FolderNameStyle, color = colors.text)
                            Text(folderCounts(folder).joinToString(", "), style = Popcorn.type.metadata, color = colors.textMuted)
                        }
                    },
                    trailingIcon = if (folder.name == draft.existingFolder) {
                        { LucideIcon(PopcornIcons.Chosen, contentDescription = "Chosen", tint = colors.text, size = 20.dp) }
                    } else {
                        null
                    },
                    onClick = {
                        expanded = false
                        actions.chooseFolder(folder.name)
                    },
                    colors = MenuDefaults.itemColors(textColor = colors.text),
                )
            }
        }
    }
}

/** An existing library's folder, which never changes, and how it is doing. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun FolderFact(state: LibraryEditorUi) {
    val library = state.library ?: return
    FlowRow(
        horizontalArrangement = Arrangement.spacedBy(Popcorn.spacing.s),
        verticalArrangement = Arrangement.spacedBy(Popcorn.spacing.xxs),
        itemVerticalAlignment = Alignment.CenterVertically,
    ) {
        Text("${library.folder ?: ""}/", style = FolderNameStyle.copy(fontSize = Popcorn.type.body.fontSize), color = Popcorn.colors.text)
        folderProblem(library.folderState)?.let { StatusWords(it) }
    }
    FieldNote(
        when (library.folderState) {
            FolderState.Missing ->
                "The folder is not there right now (a disk or share that did not mount?). New files of its kinds wait in Inbox until it is back."
            FolderState.ReadOnly -> "PopcornStash cannot write to the folder, so new files of its kinds wait in Inbox."
            else -> "Renaming the library keeps this folder."
        },
    )
}

@Composable
private fun WhoSeesIt(state: LibraryEditorUi, draft: LibraryDraft, actions: LibraryEditorActions, modifier: Modifier) {
    FormSection("Who sees it", "Owners see every library.", modifier) {
        Column(Modifier.selectableGroup()) {
            RadioRow(
                "Everyone",
                selected = draft.visibility == Visibility.Everyone,
                onClick = { actions.setVisibility(Visibility.Everyone) },
                enabled = !state.busy,
            )
            RadioRow(
                "Only some people",
                selected = draft.visibility == Visibility.Selected,
                onClick = { actions.setVisibility(Visibility.Selected) },
                enabled = !state.busy,
            )
        }
        if (draft.visibility == Visibility.Selected) {
            // The people sit under "Only some people", in line with its words.
            Column(Modifier.padding(start = 36.dp)) {
                if (state.members.isEmpty()) {
                    FieldNote("Nobody else has joined yet, so only owners will see it. Invite people on the web, in Settings, People.")
                } else {
                    state.members.forEach { member ->
                        CheckRow(
                            member.displayName,
                            checked = member.id in draft.memberIds,
                            onToggle = { actions.toggleMember(member.id) },
                            enabled = !state.busy,
                        )
                    }
                }
            }
        }
        val folder = state.chosenFolder
        if (folder != null) {
            shownWords(folder, draft.contentType, draft.structure, draft.claims)?.let { shown ->
                val names = state.members.filter { it.id in draft.memberIds }.map { it.displayName }
                FieldNote(whoWillSeeSentence(shown, draft.visibility, names))
            }
        }
        FieldError(state.errors[EditorField.Visibility])
    }
}

@Composable
private fun SaveFooter(state: LibraryEditorUi, actions: LibraryEditorActions) {
    Spacer(Modifier.height(Popcorn.spacing.xl))
    if (state.formError != null) {
        Column(Modifier.padding(bottom = Popcorn.spacing.m).semantics(mergeDescendants = true) { liveRegion = LiveRegionMode.Polite }) {
            Text("The library was not saved.", style = Popcorn.type.item, color = Popcorn.colors.error)
            Text(state.formError, style = Popcorn.type.metadata, color = Popcorn.colors.error)
        }
    }
    PrimaryButton(
        if (state.isNew) "Make library" else "Save changes",
        onClick = actions::save,
        busy = state.saving,
        enabled = !state.deleting && !state.atLimit,
        modifier = Modifier.fillMaxWidth().testTag("save library"),
    )
    if (state.atLimit) FieldNote("A stash can have $LIBRARY_LIMIT libraries.")
}

@Composable
private fun DeleteLibrary(state: LibraryEditorUi, actions: LibraryEditorActions) {
    val library = state.library ?: return
    Spacer(Modifier.height(Popcorn.spacing.l))
    FormSection("Delete library", deleteSentence(library)) {
        DangerButton(
            "Delete library",
            onClick = actions::askDelete,
            busy = state.deleting,
            enabled = !state.saving,
            modifier = Modifier.testTag("delete library"),
        )
        FieldError(state.deleteError)
    }
}

@Composable
private fun Dialogs(state: LibraryEditorUi, draft: LibraryDraft, actions: LibraryEditorActions) {
    state.confirmingClaims?.let { move ->
        val name = draft.name.trim().ifEmpty { "this library" }
        val kinds = joinWords(move.kinds.map(::claimWords))
        ConfirmDialog(
            title = "Sort $kinds into $name?",
            text = "New $kinds will go to $name instead of ${joinWords(move.from)}. Files already sorted stay where they are.",
            confirm = if (state.isNew) "Move and make library" else "Move and save",
            onConfirm = actions::confirmClaims,
            onDismiss = actions::cancelClaims,
        )
    }
    val library = state.library
    if (state.confirmingDelete && library != null) {
        LibraryRemovalDialog(library, onConfirm = actions::confirmDelete, onDismiss = actions::cancelDelete)
    }
}

/** The form's shape while it loads: a field, the icon grid and a group of choices. */
@Composable
private fun EditorPlaceholder() {
    Column(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = Popcorn.spacing.gutter)
            .clearAndSetSemantics { contentDescription = "Loading" },
    ) {
        Spacer(Modifier.height(Popcorn.spacing.l))
        PlaceholderBlock(width = 112.dp, height = 16.dp)
        Spacer(Modifier.height(Popcorn.spacing.xs))
        PlaceholderBlock(width = 260.dp, height = 12.dp)
        Spacer(Modifier.height(Popcorn.spacing.m))
        PlaceholderBlock(width = null, height = 56.dp, corner = 8.dp)
        Spacer(Modifier.height(Popcorn.spacing.l))
        BoxWithConstraints {
            val (columns, cell) = IconPickerTokens.layout(maxWidth)
            Column(verticalArrangement = Arrangement.spacedBy(IconPickerTokens.gap)) {
                repeat(2) {
                    Row(horizontalArrangement = Arrangement.spacedBy(IconPickerTokens.gap)) {
                        repeat(columns) { PlaceholderBlock(width = cell, height = cell, corner = 8.dp) }
                    }
                }
            }
        }
        Spacer(Modifier.height(Popcorn.spacing.l))
        PlaceholderBlock(width = 140.dp, height = 16.dp)
        Spacer(Modifier.height(Popcorn.spacing.m))
        repeat(2) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                PlaceholderBlock(width = 20.dp, height = 20.dp, corner = 10.dp)
                Spacer(Modifier.width(Popcorn.spacing.s))
                PlaceholderBlock(width = 96.dp, height = 14.dp)
            }
            Spacer(Modifier.height(Popcorn.spacing.l))
        }
    }
}

@Composable
private fun EditorFailed(failed: EditorLoad.Failed, onRetry: () -> Unit, onBack: () -> Unit) {
    Column(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = Popcorn.spacing.gutter, vertical = Popcorn.spacing.l)
            .semantics { liveRegion = LiveRegionMode.Polite },
    ) {
        Text(
            if (failed.gone) failed.message else "This library could not be loaded. ${failed.message}",
            style = Popcorn.type.body,
            color = Popcorn.colors.textMuted,
        )
        Spacer(Modifier.height(Popcorn.spacing.m))
        if (failed.gone) {
            SecondaryButton("Back to Settings", onClick = onBack)
        } else {
            SecondaryButton("Try again", onClick = onRetry)
        }
    }
}
