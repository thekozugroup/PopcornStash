package com.thekozugroup.popcornstash.feature.library

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.grid.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect
import com.thekozugroup.popcornstash.data.api.*
import com.thekozugroup.popcornstash.feature.downloads.popcornFieldColors
import com.thekozugroup.popcornstash.feature.settings.CheckRow
import com.thekozugroup.popcornstash.ui.components.*
import com.thekozugroup.popcornstash.ui.icons.LucideIcon
import com.thekozugroup.popcornstash.ui.icons.PopcornIcons
import com.thekozugroup.popcornstash.ui.theme.Popcorn
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/** Presentation folders flatten source paths without moving a byte on the server. */
@Composable
fun DownloadFoldersScreen(id: String, name: String, services: LibraryServices, isOwner: Boolean, bottomClearance: Dp,
    onBack: () -> Unit, onAllVideos: () -> Unit, onFiles: () -> Unit, onOpen: (Item) -> Unit) {
    var key by rememberSaveable(id) { mutableStateOf("") }
    var query by rememberSaveable(id, key) { mutableStateOf("") }
    var sort by rememberSaveable(id) { mutableStateOf("title") }
    var groups by remember(id) { mutableStateOf<List<DownloadFolder>?>(null) }
    var files by remember(id, key) { mutableStateOf(emptyList<Item>()) }
    var cursor by remember(id, key) { mutableStateOf<String?>(null) }
    var selected by rememberSaveable(id, key) { mutableStateOf(emptyList<String>()) }
    var selecting by rememberSaveable(id, key) { mutableStateOf(false) }
    var target by rememberSaveable(id, key) { mutableStateOf("") }
    var loading by remember { mutableStateOf(true) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var revision by remember { mutableIntStateOf(0) }
    var generation by remember { mutableIntStateOf(0) }
    var editing by remember { mutableStateOf<String?>(null) }
    var newName by rememberSaveable { mutableStateOf("") }
    var targetMenu by remember { mutableStateOf(false) }
    var sortMenu by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val api = services.api
    val pictures = com.thekozugroup.popcornstash.ui.media.LocalPictures.current
    val group = groups?.firstOrNull { it.key == key }
    fun message(e: Exception): String { if (e is CancellationException) throw e; return (e as? ApiException)?.message ?: "PopcornStash did not answer. Try again." }
    fun request(after: String? = null) = ItemQuery(libraryId = id, downloadGroup = key, q = query.trim(), sort = sort, order = if (sort == "title") "asc" else "desc", limit = 60, cursor = after)
    fun up() { if (key.isNotEmpty()) key = "" else onBack() }
    BackHandler(key.isNotEmpty() || busy) { if (!busy) up() }
    LifecycleEventEffect(Lifecycle.Event.ON_RESUME) { revision++ }
    LaunchedEffect(services) { services.events.collect { event -> if (event is StashEvent.LibraryChanged || event is StashEvent.LibrariesChanged) revision++ } }
    LaunchedEffect(services) { services.reconnects.collect { revision++ } }
    LaunchedEffect(id, key, query, sort, revision) {
        val current = ++generation
        loading = true; error = null
        delay(250)
        try {
            val nextGroups = api.downloadFolders(id)
            val page = if (key.isNotEmpty()) api.items(request()) else null
            if (current == generation) { groups = nextGroups; files = page?.items.orEmpty(); cursor = page?.nextCursor }
        } catch (e: Exception) { if (e is CancellationException) throw e; if (current == generation) error = message(e) }
        finally { if (current == generation) loading = false }
    }
    fun saveName() {
        busy = true; error = null
        scope.launch {
            try { key = api.nameDownloadFolder(id, newName.trim(), if (editing == "rename") key else null); editing = null; revision++ }
            catch (e: Exception) { error = message(e) }
            finally { busy = false }
        }
    }
    fun move() {
        busy = true; error = null
        scope.launch {
            try { api.moveToDownloadFolder(id, target, selected); selected = emptyList(); selecting = false; revision++ }
            catch (e: Exception) { error = message(e) }
            finally { busy = false }
        }
    }
    val gutter = Modifier.padding(vertical = Popcorn.spacing.s)
    Column(Modifier.fillMaxSize()) {
        PageTopBar(title = group?.name ?: name, onBack = if (busy) null else ::up,
            actions = { TopBarAction(PopcornIcons.StartAgain, "Refresh folders", { if (!busy) revision++ }) })
        LazyVerticalGrid(columns = GridCells.Adaptive(180.dp), contentPadding = PaddingValues(start = Popcorn.spacing.gutter, end = Popcorn.spacing.gutter, bottom = bottomClearance), horizontalArrangement = Arrangement.spacedBy(12.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            item("view", span = { GridItemSpan(maxLineSpan) }) {
                Column(gutter, verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        SecondaryButton(if (key.isEmpty()) "All videos" else "Download folders", { if (key.isEmpty()) onAllVideos() else key = "" }, enabled = !busy)
                        QuietButton("Organize files", onFiles, enabled = !busy)
                    }
                    Text("Folders group videos from each download, including nested source folders. Files stay in place.", style = Popcorn.type.metadata, color = Popcorn.colors.textMuted)
                    OutlinedTextField(query, { query = it; selected = emptyList(); loading = true }, label = { Text(if (key.isEmpty()) "Find a download folder" else "Search this folder") }, singleLine = true,
                        enabled = !busy, colors = popcornFieldColors(), shape = Popcorn.shapes.control, modifier = Modifier.fillMaxWidth())
                    Box {
                        QuietButton(if (sort == "size") "Largest first" else if (sort == "added") "Recently added" else "Name", { sortMenu = true }, enabled = !busy)
                        DropdownMenu(sortMenu, { sortMenu = false }, shape = Popcorn.shapes.panel, containerColor = Popcorn.colors.surface) {
                            listOf("title" to "Name", "added" to "Recently added", "size" to "Largest first").forEach { (value, label) -> PopcornMenuItem(text = { Text(label) }, onClick = { sort = value; sortMenu = false }) }
                        }
                    }
                    if (isOwner) {
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            SecondaryButton("New folder", { editing = "create"; newName = "" }, enabled = !busy)
                            if (group != null) QuietButton("Rename folder", { editing = "rename"; newName = group.name }, enabled = !busy)
                        }
                        if (group != null) {
                            FlattenDownloadFiles(id, key, api, onMoved = { revision++ }, onBusy = { busy = it })
                            SecondaryButton(if (selecting) "Done selecting" else "Select videos", { selecting = !selecting; selected = emptyList() }, enabled = !busy)
                            if (selecting) {
                                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                    QuietButton("Select shown", { selected = files.map { it.id } }, enabled = !busy && !loading && files.size in 1..500)
                                    Text("${selected.size} selected", style = Popcorn.type.metadata, color = Popcorn.colors.textMuted)
                                }
                                Box {
                                    SecondaryButton(groups?.firstOrNull { it.key == target }?.name ?: "Move to folder", { targetMenu = true }, enabled = !busy)
                                    DropdownMenu(targetMenu, { targetMenu = false }, shape = Popcorn.shapes.panel, containerColor = Popcorn.colors.surface) {
                                        groups.orEmpty().filter { it.key != key }.forEach { dest -> PopcornMenuItem(text = { Text(dest.name) }, onClick = { target = dest.key; targetMenu = false }) }
                                    }
                                }
                                PrimaryButton("Move selected", ::move, enabled = !busy && !loading && target.isNotEmpty() && selected.size in 1..500, busy = busy)
                            }
                        }
                    }
                    error?.let { Text(it, color = Popcorn.colors.error, style = Popcorn.type.body) }
                }
            }
            if (loading) item("loading", span = { GridItemSpan(maxLineSpan) }) { Text("Loading folders…", modifier = gutter, color = Popcorn.colors.textMuted) }
            else if (key.isEmpty()) {
                val visible = groups.orEmpty().filter { it.name.contains(query, ignoreCase = true) }.let { values -> when (sort) { "size" -> values.sortedByDescending { it.size }; "added" -> values.sortedByDescending { it.addedAt }; else -> values.sortedBy { it.name.lowercase() } } }
                items(visible, key = { it.key }, span = { GridItemSpan(maxLineSpan) }) { folder ->
                    Column {
                        ListRow(folder.name, horizontalPadding = 0.dp, supporting = "${folder.itemCount} videos · ${services.format.bytes(folder.size)}", onClick = if (busy) null else ({ key = folder.key }), leading = { LucideIcon(PopcornIcons.Folder, null) })
                        RowDivider(inset = false)
                    }
                }
                if (visible.isEmpty()) item("empty", span = { GridItemSpan(maxLineSpan) }) { Text(if (error != null) "Try Refresh folders." else "No download folders match. New downloads appear here after import.", modifier = gutter, color = Popcorn.colors.textMuted) }
            } else {
                items(files, key = { it.id }) { item ->
                    Column {
                        if (selecting) CheckRow("Select ${item.title}", item.id in selected, { selected = if (item.id in selected) selected - item.id else selected + item.id }, enabled = !busy && (selected.size < 500 || item.id in selected))
                        MediaTile(item.title, services.format.tileLine(item), pictures.thumb(item, large = true), TileShape.Frame, com.thekozugroup.popcornstash.ui.icons.LibraryIcons.glyph("clapperboard"), { if (!busy) onOpen(item) }, progress = services.format.watchedFraction(item.progress))
                    }
                }
                if (files.isEmpty()) item("empty", span = { GridItemSpan(maxLineSpan) }) { Text(if (query.isBlank()) "This folder is empty. Select videos in another folder to move them here." else "No videos match your search.", modifier = gutter, color = Popcorn.colors.textMuted) }
                if (cursor != null) item("more", span = { GridItemSpan(maxLineSpan) }) {
                    SecondaryButton("Load more videos", {
                        busy = true
                        val current = generation
                        val nextRequest = request(cursor)
                        scope.launch {
                            try { val page = api.items(nextRequest); if (current == generation) { files = (files + page.items).distinctBy { it.id }; cursor = page.nextCursor } }
                            catch (e: Exception) { error = message(e) }
                            finally { busy = false }
                        }
                    }, enabled = !busy, modifier = gutter)
                }
            }
        }
    }
    if (editing != null) ConfirmDialog(title = if (editing == "create") "New folder" else "Rename folder", confirm = "Save folder", onConfirm = ::saveName, onDismiss = { if (!busy) editing = null }, enabled = !busy, confirmEnabled = newName.isNotBlank() && !busy) {
        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text("This changes the library view. Original files stay in place.", style = Popcorn.type.metadata)
            OutlinedTextField(newName, { newName = it }, label = { Text("Folder name") }, singleLine = true, enabled = !busy, colors = popcornFieldColors(), shape = Popcorn.shapes.control)
            error?.let { Text(it, color = Popcorn.colors.error) }
        }
    }
}
