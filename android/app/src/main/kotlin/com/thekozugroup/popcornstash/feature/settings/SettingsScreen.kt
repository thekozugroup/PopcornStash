package com.thekozugroup.popcornstash.feature.settings

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.IconButton
import androidx.compose.material3.MenuDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.thekozugroup.popcornstash.data.api.Library
import com.thekozugroup.popcornstash.data.api.Role as PersonRole
import com.thekozugroup.popcornstash.data.api.UnassignedFolder
import com.thekozugroup.popcornstash.data.api.User
import com.thekozugroup.popcornstash.data.repo.Loadable
import com.thekozugroup.popcornstash.feature.downloads.breakableUrl
import com.thekozugroup.popcornstash.navigation.TabPage
import com.thekozugroup.popcornstash.ui.components.ConfirmDialog
import com.thekozugroup.popcornstash.ui.components.ListRow
import com.thekozugroup.popcornstash.ui.components.RowDivider
import com.thekozugroup.popcornstash.ui.components.SecondaryButton
import com.thekozugroup.popcornstash.ui.components.SectionHeading
import com.thekozugroup.popcornstash.ui.components.TabRootScaffold
import com.thekozugroup.popcornstash.ui.icons.LibraryIcons
import com.thekozugroup.popcornstash.ui.icons.LucideGlyph
import com.thekozugroup.popcornstash.ui.icons.LucideIcon
import com.thekozugroup.popcornstash.ui.icons.PopcornIcons
import com.thekozugroup.popcornstash.ui.theme.Popcorn

/** A web settings page the app opens in a Custom Tab (docs/architecture.md section 10, Settings). */
data class WebSettingsPage(val label: String, val path: String)

/**
 * The web settings pages a person can open from the app. Owners manage libraries here in the app;
 * the rest of the web's Libraries page (sorting, a scan, the trash) and the other admin areas open
 * on the web. Members get their own devices and passkeys.
 */
fun webSettingsPages(role: PersonRole): List<WebSettingsPage> = if (role == PersonRole.Owner) {
    listOf(
        WebSettingsPage("Sorting and trash", "settings/libraries"),
        WebSettingsPage("VPN", "settings/vpn"),
        WebSettingsPage("People", "settings/people"),
        WebSettingsPage("Devices", "settings/devices"),
        WebSettingsPage("Security", "settings/security"),
        WebSettingsPage("System", "settings/system"),
    )
} else {
    listOf(WebSettingsPage("Devices", "settings/devices"), WebSettingsPage("Security", "settings/security"))
}

/** What the owner's Libraries section does when tapped. */
class LibrarySettingsActions(
    val onRetry: () -> Unit = {},
    val onOpen: (Library) -> Unit = {},
    /** Opens the editor for a new library; [folder] when it is made from a folder that is already there. */
    val onNew: (folder: String?) -> Unit = {},
    val onMove: (Library, step: Int) -> Unit = { _, _ -> },
    val onDelete: (Library) -> Unit = {},
)

/**
 * Settings, the right tab's first page (android/DESIGN.md, Settings): the signed-in person, this
 * server, for owners the libraries and the folders that are not a library yet, the web settings
 * pages, this app and its updates ([appUpdates]), and sign out. [libraries] is null for members.
 * [uploadsRunning] share sheet uploads stop when the person signs out, and the confirmation says so.
 */
@Composable
fun SettingsRootPage(
    page: TabPage,
    user: User,
    serverDisplay: String,
    serverVersion: String?,
    libraries: LibrarySettingsUi?,
    onOpenWeb: (WebSettingsPage) -> Unit,
    onSignOut: () -> Unit,
    actions: LibrarySettingsActions = LibrarySettingsActions(),
    listState: LazyListState = rememberLazyListState(),
    uploadsRunning: Int = 0,
    appUpdates: AppUpdateSectionState? = null,
    updateActions: AppUpdateActions = AppUpdateActions(),
) {
    var confirmSignOut by rememberSaveable { mutableStateOf(false) }
    var confirmDeleteId by rememberSaveable { mutableStateOf<String?>(null) }
    TabRootScaffold(page = page, title = "Settings", listState = listState) {
        item(key = "you") {
            SectionHeading("You")
            ListRow(
                title = user.displayName.ifBlank { "Signed in" },
                supporting = if (user.role == PersonRole.Owner) "Owner" else "Member",
                leading = { Avatar(user.displayName) },
            )
        }
        item(key = "server") {
            SectionHeading("This server")
            ListRow(
                // A long address wraps after a dot or a hyphen, never inside a word.
                title = breakableUrl(serverDisplay),
                supporting = serverVersion?.let { "Version $it" },
                leading = { LucideIcon(PopcornIcons.Server, contentDescription = null, tint = Popcorn.colors.textMuted) },
            )
        }
        if (libraries != null) {
            librariesSection(libraries, actions, onDelete = { confirmDeleteId = it.id })
            foldersSection(libraries.folders, actions)
        }
        item(key = "web heading") { SectionHeading("On the web") }
        val pages = webSettingsPages(user.role)
        pages.forEachIndexed { index, web ->
            item(key = "web ${web.path}") {
                ListRow(
                    title = web.label,
                    onClick = { onOpenWeb(web) },
                    onClickLabel = "Open ${web.label} on the web",
                    trailing = { LucideIcon(PopcornIcons.OpenOnWeb, contentDescription = null, tint = Popcorn.colors.textMuted, size = 20.dp) },
                )
                if (index < pages.lastIndex) RowDivider(inset = false)
            }
        }
        if (appUpdates != null) appUpdateSection(appUpdates, updateActions)
        item(key = "sign out") {
            Spacer(Modifier.height(Popcorn.spacing.xl))
            RowDivider(inset = false)
            ListRow(
                title = "Sign out",
                titleColor = Popcorn.colors.textMuted,
                onClick = { confirmSignOut = true },
                leading = { LucideIcon(PopcornIcons.SignOut, contentDescription = null, tint = Popcorn.colors.textMuted) },
            )
        }
    }
    if (confirmSignOut) {
        ConfirmDialog(
            title = "Sign out on this phone?",
            text = signOutSentence(uploadsRunning),
            confirm = "Sign out",
            onConfirm = {
                confirmSignOut = false
                onSignOut()
            },
            onDismiss = { confirmSignOut = false },
        )
    }
    val deleting = confirmDeleteId?.let { id -> libraries?.shown?.firstOrNull { it.id == id } }
    if (deleting != null) {
        ConfirmDialog(
            title = "Delete ${deleting.name}?",
            text = deleteSentence(deleting),
            confirm = "Delete library",
            destructive = true,
            onConfirm = {
                confirmDeleteId = null
                actions.onDelete(deleting)
            },
            onDismiss = { confirmDeleteId = null },
        )
    }
}

/** The leading glyph of a library or folder row: 20dp on a 40dp `subtle` square with 8dp corners. */
@Composable
fun GlyphSquare(glyph: LucideGlyph, modifier: Modifier = Modifier) {
    Box(
        modifier
            .size(40.dp)
            .background(Popcorn.colors.subtle, Popcorn.shapes.control)
            .clearAndSetSemantics { },
        contentAlignment = Alignment.Center,
    ) {
        LucideIcon(glyph, contentDescription = null, tint = Popcorn.colors.text, size = 20.dp)
    }
}

/** The rule between library and folder rows, starting where their names start. */
@Composable
private fun SquareRowDivider() {
    HorizontalDivider(
        thickness = 1.dp,
        color = Popcorn.colors.divider,
        modifier = Modifier.padding(start = Popcorn.spacing.gutter + 40.dp + Popcorn.spacing.m),
    )
}

@Composable
private fun SectionSentence(text: String) {
    Text(
        text,
        style = Popcorn.type.metadata,
        color = Popcorn.colors.textMuted,
        modifier = Modifier
            .fillMaxWidth()
            .padding(start = Popcorn.spacing.gutter, end = Popcorn.spacing.gutter, bottom = Popcorn.spacing.xs),
    )
}

private fun LazyListScope.librariesSection(state: LibrarySettingsUi, actions: LibrarySettingsActions, onDelete: (Library) -> Unit) {
    item(key = "libraries heading") {
        SectionHeading("Libraries")
        SectionSentence("What Home and the web sidebar show, in this order.")
    }
    val shown = state.shown
    val libraries = state.libraries
    when {
        shown == null && libraries is Loadable.Failed -> item(key = "libraries failed") {
            LibrariesFailed(libraries.message, actions.onRetry)
        }
        shown == null -> items(4, key = { "placeholder $it" }) { LibraryRowPlaceholder() }
        shown.isEmpty() -> item(key = "libraries empty") {
            Text(
                "No libraries yet. Make one to give the family something to browse.",
                style = Popcorn.type.body,
                color = Popcorn.colors.textMuted,
                modifier = Modifier.padding(horizontal = Popcorn.spacing.gutter, vertical = Popcorn.spacing.s),
            )
        }
        else -> itemsIndexed(shown, key = { _, library -> "library ${library.id}" }) { index, library ->
            LibrarySettingsRow(
                library = library,
                state = state,
                index = index,
                total = shown.size,
                onOpen = { actions.onOpen(library) },
                onMove = { step -> actions.onMove(library, step) },
                onDelete = { onDelete(library) },
            )
            if (index < shown.lastIndex) SquareRowDivider()
        }
    }
    // Offered once the list is there, so the owner sees what exists before making another.
    if (shown != null) {
        item(key = "make a library") {
            Column(Modifier.padding(start = Popcorn.spacing.gutter, end = Popcorn.spacing.gutter, top = Popcorn.spacing.s)) {
                SecondaryButton(
                    "Make a library",
                    onClick = { actions.onNew(null) },
                    glyph = PopcornIcons.Add,
                    enabled = state.canMakeMore,
                    modifier = Modifier.testTag("make a library"),
                )
                if (!state.canMakeMore) {
                    FieldNote("A stash can have $LIBRARY_LIMIT libraries.")
                }
            }
        }
    }
}

private fun LazyListScope.foldersSection(folders: List<UnassignedFolder>, actions: LibrarySettingsActions) {
    if (folders.isEmpty()) return
    item(key = "folders heading") {
        SectionHeading("Folders not in a library")
        SectionSentence("Only owners see what is in them. Make one a library to share it.")
    }
    itemsIndexed(folders, key = { _, folder -> "folder ${folder.name}" }) { index, folder ->
        FolderRow(folder, onMake = { actions.onNew(folder.name) })
        if (index < folders.lastIndex) SquareRowDivider()
    }
}

/**
 * One library in the owner's list: its icon, its name, and what it is, takes and who sees it.
 * The row opens the editor; More moves it up or down or deletes it, and TalkBack offers the
 * same three as actions on the row.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun LibrarySettingsRow(
    library: Library,
    state: LibrarySettingsUi,
    index: Int,
    total: Int,
    onOpen: () -> Unit,
    onMove: (Int) -> Unit,
    onDelete: () -> Unit,
) {
    val colors = Popcorn.colors
    var menu by rememberSaveable { mutableStateOf(false) }
    val canMove = !state.moving && state.deletingId == null
    val deleting = state.deletingId == library.id
    val rowActions = buildList {
        if (canMove && index > 0) add(CustomAccessibilityAction("Move up") { onMove(-1); true })
        if (canMove && index < total - 1) add(CustomAccessibilityAction("Move down") { onMove(1); true })
        if (!deleting) add(CustomAccessibilityAction("Delete") { onDelete(); true })
    }
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 72.dp)
            .testTag("library row ${library.name}")
            .clickable(enabled = !deleting, onClickLabel = "Edit ${library.name}", role = Role.Button, onClick = onOpen)
            .semantics { customActions = rowActions }
            .padding(start = Popcorn.spacing.gutter, end = Popcorn.spacing.xxs, top = Popcorn.spacing.s, bottom = Popcorn.spacing.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        GlyphSquare(LibraryIcons.glyph(library.icon))
        Spacer(Modifier.width(Popcorn.spacing.m))
        Column(Modifier.weight(1f)) {
            Text(
                library.name,
                style = Popcorn.type.item,
                color = if (deleting) colors.textMuted else colors.text,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(Popcorn.spacing.s),
                verticalArrangement = Arrangement.spacedBy(2.dp),
            ) {
                if (deleting) {
                    Text("Deleting", style = Popcorn.type.metadata, color = colors.textMuted)
                } else {
                    Text(kindLine(library.contentType, library.structure), style = Popcorn.type.metadata, color = colors.textMuted)
                    Text(takesLine(library.claims), style = Popcorn.type.metadata, color = colors.textMuted)
                    Text(whoSeesLine(library.visibility, library.memberIds, state.people), style = Popcorn.type.metadata, color = colors.textMuted)
                    folderProblem(library.folderState)?.let { StatusWords(it) }
                }
            }
        }
        Box {
            IconButton(onClick = { menu = true }, enabled = !deleting, modifier = Modifier.testTag("more ${library.name}")) {
                LucideIcon(PopcornIcons.More, contentDescription = "More for ${library.name}", tint = colors.textMuted)
            }
            DropdownMenu(
                expanded = menu,
                onDismissRequest = { menu = false },
                shape = Popcorn.shapes.control,
                containerColor = colors.surface,
                border = BorderStroke(1.dp, colors.divider),
                shadowElevation = 4.dp,
            ) {
                MenuEntry("Move up", enabled = canMove && index > 0) {
                    menu = false
                    onMove(-1)
                }
                MenuEntry("Move down", enabled = canMove && index < total - 1) {
                    menu = false
                    onMove(1)
                }
                HorizontalDivider(color = colors.divider)
                MenuEntry("Delete", destructive = true) {
                    menu = false
                    onDelete()
                }
            }
        }
    }
}

@Composable
private fun MenuEntry(text: String, enabled: Boolean = true, destructive: Boolean = false, onClick: () -> Unit) {
    val colors = Popcorn.colors
    DropdownMenuItem(
        text = { Text(text, style = Popcorn.type.item) },
        onClick = onClick,
        enabled = enabled,
        colors = MenuDefaults.itemColors(
            textColor = if (destructive) colors.error else colors.text,
            disabledTextColor = colors.textMuted.copy(alpha = 0.6f),
        ),
        modifier = Modifier.widthIn(min = 160.dp),
    )
}

/** A top-level folder that is not a library: its name as on the disk, what is in it, and "Make a library". */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun FolderRow(folder: UnassignedFolder, onMake: () -> Unit) {
    val colors = Popcorn.colors
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 72.dp)
            .testTag("folder row ${folder.name}")
            .clickable(onClickLabel = "Make a library from ${folder.name}", role = Role.Button, onClick = onMake)
            .padding(horizontal = Popcorn.spacing.gutter, vertical = Popcorn.spacing.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        GlyphSquare(PopcornIcons.Folder)
        Spacer(Modifier.width(Popcorn.spacing.m))
        Column(Modifier.weight(1f)) {
            // The whole name: a short one stays on one line, a long one wraps at its spaces
            // (a phone shows no tooltip, so an ellipsis would hide the rest for good).
            Text(folderInSentence(folder.name), style = FolderNameStyle, color = colors.text)
            FlowRow(horizontalArrangement = Arrangement.spacedBy(Popcorn.spacing.s)) {
                folderCounts(folder).forEach { Text(it, style = Popcorn.type.metadata, color = colors.textMuted) }
            }
        }
        Spacer(Modifier.width(Popcorn.spacing.s))
        Text("Make a library", style = Popcorn.type.action, color = colors.text)
    }
}

@Composable
private fun LibrariesFailed(message: String, onRetry: () -> Unit) {
    Column(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = Popcorn.spacing.gutter, vertical = Popcorn.spacing.s)
            .semantics { liveRegion = LiveRegionMode.Polite },
    ) {
        Text("Libraries did not load. $message", style = Popcorn.type.body, color = Popcorn.colors.textMuted)
        Spacer(Modifier.height(Popcorn.spacing.s))
        SecondaryButton("Try again", onClick = onRetry)
    }
}

/** A library row while the list loads, at the row's final size. */
@Composable
private fun LibraryRowPlaceholder() {
    Row(
        Modifier
            .fillMaxWidth()
            .height(72.dp)
            .padding(horizontal = Popcorn.spacing.gutter)
            .clearAndSetSemantics { },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        PlaceholderBlock(width = 40.dp, height = 40.dp, corner = 8.dp)
        Spacer(Modifier.width(Popcorn.spacing.m))
        Column {
            PlaceholderBlock(width = 112.dp, height = 14.dp)
            Spacer(Modifier.height(Popcorn.spacing.xs))
            PlaceholderBlock(width = 200.dp, height = 12.dp)
        }
    }
}

/** The person's initials on a circle, 48dp. */
@Composable
private fun Avatar(name: String) {
    val initials = name.split(' ').filter { it.isNotBlank() }.take(2).joinToString("") { it.first().uppercase() }
    Box(
        Modifier
            .size(48.dp)
            .background(Popcorn.colors.subtle, Popcorn.shapes.avatar)
            .clearAndSetSemantics { },
        contentAlignment = Alignment.Center,
    ) {
        Text(initials.ifEmpty { "?" }, style = Popcorn.type.section, color = Popcorn.colors.text)
    }
}

/** The sign-out confirmation, with how many share sheet uploads it stops (android/DESIGN.md, Settings). */
fun signOutSentence(uploadsRunning: Int): String {
    val base = "PopcornStash forgets your sign-in and everything it kept on this phone."
    return when {
        uploadsRunning <= 0 -> base
        uploadsRunning == 1 -> "$base 1 upload stops."
        else -> "$base $uploadsRunning uploads stop."
    }
}
