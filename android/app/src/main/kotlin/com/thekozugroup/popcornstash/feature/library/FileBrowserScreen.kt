package com.thekozugroup.popcornstash.feature.library

import androidx.compose.foundation.selection.toggleable
import androidx.compose.ui.semantics.Role
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.thekozugroup.popcornstash.data.api.*
import com.thekozugroup.popcornstash.feature.downloads.popcornFieldColors
import com.thekozugroup.popcornstash.ui.components.*
import com.thekozugroup.popcornstash.ui.icons.LucideIcon
import com.thekozugroup.popcornstash.ui.icons.PopcornIcons
import com.thekozugroup.popcornstash.ui.theme.Popcorn
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/** Search and explicit file organization share the same scoped, cursor-paged file list. */
@Composable
fun FileBrowserScreen(
    services: LibraryServices,
    bottomClearance: Dp,
    isOwner: Boolean,
    onOpen: (Item) -> Unit,
    libraryId: String? = null,
    libraryName: String = "Search",
    organize: Boolean = false,
    inbox: Boolean = false,
    onBack: (() -> Unit)? = null,
) {
    val api = services.api
    val libraries by services.data.libraries.collectAsStateWithLifecycle()
    var scopeId by rememberSaveable(libraryId) { mutableStateOf(libraryId) }
    var query by rememberSaveable { mutableStateOf("") }
    var folder by rememberSaveable { mutableStateOf("") }
    var kind by rememberSaveable { mutableStateOf("all") }
    var selected by rememberSaveable { mutableStateOf(emptyList<String>()) }
    var files by remember { mutableStateOf(emptyList<Item>()) }
    var folders by remember { mutableStateOf(emptyList<String>()) }
    var cursor by remember { mutableStateOf<String?>(null) }
    var loading by remember { mutableStateOf(true) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var notice by remember { mutableStateOf<String?>(null) }
    var revision by remember { mutableIntStateOf(0) }
    var scopeMenu by remember { mutableStateOf(false) }
    var typeMenu by remember { mutableStateOf(false) }
    var movePicker by remember { mutableStateOf(false) }
    var destination by rememberSaveable { mutableStateOf("") }
    var createFolder by remember { mutableStateOf(false) }
    var newName by rememberSaveable { mutableStateOf("") }
    var confirmMove by remember { mutableStateOf(false) }
    var confirmTrash by remember { mutableStateOf(false) }
    val actions = rememberCoroutineScope()
    val types = listOf("all" to "All types", "video" to "Videos", "photo" to "Images", "subtitle" to "Subtitles", "audio" to "Audio", "other" to "Other files")
    val visible = files.filter { kind == "all" || fileType(it) == kind }
    val selectedVisible = selected.filter { id -> visible.any { it.id == id } }
    val canOrganize = organize && libraryId != null
    val title = if (inbox) "Manage Inbox" else if (organize) libraryName else "Search"
    fun request(pageCursor: String? = null) = ItemQuery(
        libraryId = scopeId, folder = if (canOrganize) folder else null,
        includeInbox = inbox || scopeId == null, place = if (inbox) Place.Incoming else null,
        q = query.trim(), sort = "title", order = "asc", cursor = pageCursor,
        limit = if (pageCursor == null || (!organize && !inbox)) 200 else minOf(200, 500 - files.size),
    )
    fun message(e: Exception): String {
        if (e is CancellationException) throw e
        return (e as? ApiException)?.message ?: "PopcornStash did not answer. Try again."
    }
    LaunchedEffect(services) { if (libraryId == null && !inbox) services.data.refreshLibraries() }
    LifecycleEventEffect(Lifecycle.Event.ON_RESUME) { revision++ }
    LaunchedEffect(query, scopeId, folder, revision) {
        loading = true
        files = emptyList(); folders = emptyList(); cursor = null
        delay(300)
        try {
            val page = api.items(request())
            val children = if (canOrganize) api.libraryFolders(requireNotNull(libraryId), folder).folders else emptyList()
            files = page.items; cursor = page.nextCursor; folders = children
        } catch (e: Exception) { error = message(e) }
        finally { loading = false }
    }
    fun changeFolder(value: String) { folder = value; selected = emptyList(); files = emptyList(); folders = emptyList(); loading = true }
    fun more() {
        val next = cursor ?: return
        busy = true; error = null
        actions.launch {
            try {
                val page = api.items(request(next))
                files = (files + page.items).distinctBy { it.id }; cursor = page.nextCursor
            } catch (e: Exception) { error = message(e) }
            finally { busy = false }
        }
    }
    fun submitMove() {
        busy = true; error = null
        actions.launch {
            try {
                val result = api.moveLibraryFiles(requireNotNull(libraryId), selectedVisible, destination)
                selected = result.failed.map { it.id }
                notice = "${result.completed.size} files moved. Watch history is preserved."
                if (result.failed.isNotEmpty()) error = "${result.failed.size} files did not move. ${result.failed.first().message}"
                revision++
            } catch (e: Exception) { error = message(e) }
            finally { busy = false; confirmMove = false }
        }
    }
    fun submitTrash() {
        busy = true; error = null
        actions.launch {
            try {
                val result = api.trashInboxFiles(selectedVisible)
                files = files.filterNot { it.id in result.completed }
                selected = result.failed.map { it.id }
                notice = "${result.completed.size} files moved to Trash."
                if (result.failed.isNotEmpty()) error = "${result.failed.size} files stayed in Inbox. ${result.failed.first().message}"
                services.data.refreshInbox()
            } catch (e: Exception) { error = message(e) }
            finally { busy = false; confirmTrash = false }
        }
    }
    val gutter = Modifier.padding(horizontal = Popcorn.spacing.gutter, vertical = Popcorn.spacing.s)
    Column(Modifier.fillMaxSize().testTag("file browser")) {
        PageTopBar(title = title, onBack = onBack, actions = {
            TopBarAction(PopcornIcons.StartAgain, "Refresh files", { if (!busy) { error = null; revision++ } })
        })
        LazyColumn(contentPadding = PaddingValues(bottom = bottomClearance)) {
            item("search") {
                OutlinedTextField(value = query, onValueChange = { query = it; selected = emptyList(); loading = true }, label = { Text(if (canOrganize) "Search this folder and subfolders" else "Search titles and filenames") },
                    singleLine = true, enabled = !busy, colors = popcornFieldColors(), shape = Popcorn.shapes.control,
                    modifier = gutter.fillMaxWidth())
            }
            if (libraryId == null && !inbox) item("scope") {
                Box(gutter) {
                    SecondaryButton(libraries.valueOrNull?.firstOrNull { it.id == scopeId }?.name ?: "All libraries and Inbox", { scopeMenu = true }, enabled = !busy)
                    DropdownMenu(expanded = scopeMenu, onDismissRequest = { scopeMenu = false }, shape = Popcorn.shapes.panel, containerColor = Popcorn.colors.surface) {
                        PopcornMenuItem(text = { Text("All libraries and Inbox") }, onClick = { if (scopeId != null) { scopeId = null; selected = emptyList(); loading = true }; scopeMenu = false })
                        libraries.valueOrNull.orEmpty().forEach { library -> PopcornMenuItem(text = { Text(library.name) }, onClick = { if (scopeId != library.id) { scopeId = library.id; selected = emptyList(); loading = true }; scopeMenu = false }) }
                    }
                }
            }
            if (canOrganize) {
                item("folder") {
                    Column(gutter, verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        Text(folder.ifEmpty { "Library root" }, style = Popcorn.type.item, color = Popcorn.colors.text)
                        if (folder.isNotEmpty()) QuietButton("Up one folder", { changeFolder(folder.substringBeforeLast('/', "")) }, enabled = !busy)
                        if (isOwner) SecondaryButton("Create subfolder", { newName = ""; createFolder = true }, enabled = !busy)
                    }
                }
                items(folders, key = { "folder-$it" }) { child ->
                    ListRow(title = child.substringAfterLast('/'), onClick = if (busy) null else ({ changeFolder(child) }), leading = { LucideIcon(PopcornIcons.Folder, contentDescription = null) })
                }
            }
            item("filters") {
                Column(gutter, verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Box {
                        QuietButton(types.first { it.first == kind }.second, { typeMenu = true }, enabled = !busy)
                        DropdownMenu(expanded = typeMenu, onDismissRequest = { typeMenu = false }, shape = Popcorn.shapes.panel, containerColor = Popcorn.colors.surface) {
                            types.forEach { (value, label) -> PopcornMenuItem(text = { Text(label) }, onClick = { kind = value; selected = emptyList(); typeMenu = false }) }
                        }
                    }
                    if (isOwner && (canOrganize || inbox)) {
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            SecondaryButton("Select shown (${visible.size})", { selected = visible.map { it.id } }, enabled = !busy && !loading && visible.isNotEmpty(), modifier = Modifier.weight(1f))
                            QuietButton("Clear", { selected = emptyList() }, enabled = !busy && selected.isNotEmpty())
                        }
                        Text("${if (loading) selected.size else selectedVisible.size} selected", style = Popcorn.type.metadata, color = Popcorn.colors.textMuted)
                        if (canOrganize) PrimaryButton("Move selected", { destination = folder; movePicker = true }, enabled = !busy && !loading && selectedVisible.isNotEmpty())
                        if (inbox) SecondaryButton("Delete selected", { confirmTrash = true }, enabled = !busy && !loading && selectedVisible.isNotEmpty())
                    }
                    error?.let { Text(it, color = Popcorn.colors.error, style = Popcorn.type.body) }
                    notice?.let { Text(it, color = Popcorn.colors.textMuted, style = Popcorn.type.body) }
                }
            }
            if (loading) item("loading") { Text("Loading files…", modifier = gutter, color = Popcorn.colors.textMuted) }
            else {
                if (visible.isEmpty()) item("empty") { Text("No files match. Change the search, folder or file type.", modifier = gutter, color = Popcorn.colors.textMuted) }
                items(visible, key = { it.id }) { file ->
                    val selecting = isOwner && (canOrganize || inbox)
                    ListRow(title = file.title, supporting = file.relPath,
                        modifier = if (selecting) Modifier.toggleable(value = file.id in selected, enabled = !busy, role = Role.Checkbox,
                            onValueChange = { checked -> selected = if (checked) selected + file.id else selected - file.id }) else Modifier,
                        leading = if (selecting) ({ Checkbox(checked = file.id in selected, enabled = !busy, onCheckedChange = null) }) else null,
                        onClick = if (busy || selecting) null else ({ onOpen(file) }),
                        trailing = if (selecting) ({ QuietButton("Open", { onOpen(file) }, enabled = !busy) }) else null)
                    HorizontalDivider(color = Popcorn.colors.divider)
                }
                if (cursor != null) item("more") {
                    if ((!organize && !inbox) || files.size < 500) SecondaryButton("Load more files", ::more, enabled = !busy, modifier = gutter)
                    else Text("Showing 500 files. Narrow the search or move this selection before continuing.", modifier = gutter, color = Popcorn.colors.textMuted)
                }
            }
        }
    }
    if (createFolder) ConfirmDialog(title = "Create subfolder", confirm = "Create", onConfirm = {
        busy = true
        actions.launch {
            try { api.createLibraryFolder(requireNotNull(libraryId), folder, newName.trim()); createFolder = false; notice = "Folder created."; revision++ }
            catch (e: Exception) { error = message(e); createFolder = false }
            finally { busy = false }
        }
    }, onDismiss = { if (!busy) createFolder = false }, enabled = !busy, confirmEnabled = !busy && newName.isNotBlank()) {
        Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text("Inside ${folder.ifEmpty { "Library root" }}")
            OutlinedTextField(newName, { newName = it }, label = { Text("Folder name") }, singleLine = true, enabled = !busy, colors = popcornFieldColors(), shape = Popcorn.shapes.control)
        }
    }
    if (movePicker) FolderDestination(api, requireNotNull(libraryId), destination, { destination = it }, { movePicker = false }, { movePicker = false; confirmMove = true })
    if (confirmMove) ConfirmDialog("Move ${selectedVisible.size} files?", "Destination: ${destination.ifEmpty { "Library root" }}. Existing files are never overwritten. Only selected files move.", "Move files", ::submitMove, { if (!busy) confirmMove = false }, enabled = !busy)
    if (confirmTrash) ConfirmDialog("Delete ${selectedVisible.size} files?", "Selected files move to Trash and leave Inbox. Owners can restore them in Settings.", "Move to Trash", ::submitTrash, { if (!busy) confirmTrash = false }, destructive = true, enabled = !busy)
}

@Composable
private fun FolderDestination(api: ApiClient, id: String, folder: String, choose: (String) -> Unit, close: () -> Unit, confirm: () -> Unit) {
    var folders by remember(folder) { mutableStateOf<List<String>?>(null) }
    var error by remember(folder) { mutableStateOf<String?>(null) }
    LaunchedEffect(folder) {
        try { folders = api.libraryFolders(id, folder).folders }
        catch (e: ApiException) { error = e.message }
    }
    ConfirmDialog(title = "Choose destination", confirm = "Use this folder", onConfirm = confirm, onDismiss = close, confirmEnabled = folders != null && error == null) {
        Column(Modifier.heightIn(max = 400.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(folder.ifEmpty { "Library root" })
            if (folder.isNotEmpty()) QuietButton("Up one folder", { choose(folder.substringBeforeLast('/', "")) })
            folders?.forEach { child -> SecondaryButton(child.substringAfterLast('/'), { choose(child) }, glyph = PopcornIcons.Folder) }
            error?.let { Text(it, color = Popcorn.colors.error) }
            if (folders == null && error == null) Text("Reading folders…")
        }
    }
}

private fun fileType(file: Item): String = when {
    file.relPath.substringAfterLast('.').lowercase() in setOf("srt", "ass", "ssa", "vtt", "sub", "idx") -> "subtitle"
    file.kind == MediaKind.Video -> "video"
    file.kind == MediaKind.Photo -> "photo"
    file.kind == MediaKind.Audio -> "audio"
    else -> "other"
}
