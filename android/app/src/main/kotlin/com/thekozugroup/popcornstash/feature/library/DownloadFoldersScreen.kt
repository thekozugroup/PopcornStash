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

/** Source download roots with optional collapsed browsing and explicit physical edits. */
@Composable
fun DownloadFoldersScreen(id: String, name: String, services: LibraryServices, isOwner: Boolean, bottomClearance: Dp,
    onBack: () -> Unit, onAllVideos: () -> Unit, onFiles: () -> Unit, onOpen: (Item) -> Unit) {
    var key by rememberSaveable(id) { mutableStateOf("") }
    var folder by rememberSaveable(id, key) { mutableStateOf("") }
    var collapsed by rememberSaveable(id) { mutableStateOf(false) }
    var tree by remember(id, key, folder) { mutableStateOf<DownloadTree?>(null) }
    var preview by remember(id, key, folder) { mutableStateOf<RenameFolderPreview?>(null) }
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
    var organizing by rememberSaveable(id,key) { mutableStateOf(false) }
    var targetMenu by remember { mutableStateOf(false) }
    var sortMenu by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val api = services.api
    val pictures = com.thekozugroup.popcornstash.ui.media.LocalPictures.current
    val group = groups?.firstOrNull { it.key == key }
    fun message(e: Exception): String { if (e is CancellationException) throw e; return (e as? ApiException)?.message ?: "PopcornStash did not answer. Try again." }
    fun request(after: String? = null) = ItemQuery(libraryId = id, downloadGroup = key, downloadFolder = if (collapsed) null else folder, q = query.trim(), sort = sort, order = if (sort == "title") "asc" else "desc", limit = 60, cursor = after)
    fun up() { if (folder.isNotEmpty()) folder = folder.substringBeforeLast('/', "") else if (key.isNotEmpty()) key = "" else onBack() }
    BackHandler(key.isNotEmpty() || busy) { if (!busy) up() }
    LifecycleEventEffect(Lifecycle.Event.ON_RESUME) { revision++ }
    LaunchedEffect(services) { services.events.collect { event -> if (event is StashEvent.LibraryChanged || event is StashEvent.LibrariesChanged) revision++ } }
    LaunchedEffect(services) { services.reconnects.collect { revision++ } }
    LaunchedEffect(id, key, folder, collapsed, query, sort, revision) {
        val current = ++generation
        loading = true; error = null
        delay(250)
        try {
            val nextGroups = api.downloadFolders(id)
            val page = if (key.isNotEmpty()) api.items(request()) else null
            val nextTree = if (key.isNotEmpty() && !collapsed) api.downloadTree(id,key,folder) else null
            if (current == generation) { groups = nextGroups; tree = nextTree; files = page?.items.orEmpty(); cursor = page?.nextCursor }
        } catch (e: Exception) { if (e is CancellationException) throw e; if (current == generation) error = message(e) }
        finally { if (current == generation) loading = false }
    }
    fun saveName() {
        busy = true; error = null
        scope.launch {
            try {
                if (editing == "rename") {
                    val value=preview
                    if(value==null) {preview=api.previewFolderRename(id,key,folder,newName.trim());return@launch}
                    api.renameDirectory(id,key,folder,newName.trim(),value.token)
                    if(folder.isNotEmpty()) {val parent=folder.substringBeforeLast('/',"");folder=if(parent.isEmpty())newName.trim() else "$parent/${newName.trim()}"}
                } else {key=api.createDownloadDirectory(id,newName.trim(),key.ifEmpty{null},folder)}
                editing=null;preview=null;revision++
            }
            catch (e: Exception) { preview = null; error = message(e) }
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
        PageTopBar(title = folder.substringAfterLast('/').ifEmpty { group?.name ?: name }, onBack = if (busy) null else ::up,
            actions = { TopBarAction(PopcornIcons.StartAgain, "Refresh folders", { if (!busy) revision++ }) })
        LazyVerticalGrid(columns = GridCells.Adaptive(180.dp), contentPadding = PaddingValues(start = Popcorn.spacing.gutter, end = Popcorn.spacing.gutter, bottom = bottomClearance), horizontalArrangement = Arrangement.spacedBy(12.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            item("view", span = { GridItemSpan(maxLineSpan) }) {
                Column(gutter, verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        SecondaryButton(if (key.isEmpty()) "All videos" else "Download folders", { if (key.isEmpty()) onAllVideos() else key = "" }, enabled = !busy)
                        if(group!=null && isOwner) QuietButton(if(organizing) "Close folder actions" else "Folder actions",{organizing=!organizing},enabled=!busy)
                        else QuietButton("Organize files", onFiles, enabled = !busy)
                    }
                    if(group==null) Text("Each download has its own folder. Open one to browse or organize its videos.", style = Popcorn.type.metadata, color = Popcorn.colors.textMuted)
                    Row(horizontalArrangement=Arrangement.spacedBy(8.dp),verticalAlignment=androidx.compose.ui.Alignment.CenterVertically) {
                    OutlinedTextField(query, { query = it; selected = emptyList(); loading = true }, label = { Text(if (key.isEmpty()) "Find a download folder" else "Search this folder") }, singleLine = true,
                        enabled = !busy, colors = popcornFieldColors(), shape = Popcorn.shapes.control, modifier = Modifier.weight(1f))
                    Box {
                        QuietButton(if (sort == "size") "Largest first" else if (sort == "added") "Recently added" else "Name", { sortMenu = true }, enabled = !busy)
                        DropdownMenu(sortMenu, { sortMenu = false }, shape = Popcorn.shapes.panel, containerColor = Popcorn.colors.surface) {
                            listOf("title" to "Name", "added" to "Recently added", "size" to "Largest first").forEach { (value, label) -> PopcornMenuItem(text = { Text(label) }, onClick = { sort = value; sortMenu = false }) }
                        }
                    }
                    }
                    if (group != null) {
                        if(folder.isNotEmpty()) FlowRow(horizontalArrangement=Arrangement.spacedBy(8.dp)) {
                            QuietButton(group.name,{folder=""},enabled=!busy && folder.isNotEmpty())
                            folder.split('/').filter{it.isNotEmpty()}.forEachIndexed { index,part ->
                                QuietButton(part,{folder=folder.split('/').take(index+1).joinToString("/")},enabled=!busy && index<folder.split('/').lastIndex)
                            }
                        }
                        CheckRow("Collapse subfolders",collapsed,{collapsed=!collapsed;folder="";selected=emptyList();loading=true},enabled=!busy)
                    }
                    if (isOwner && (group==null || organizing)) {
                        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            SecondaryButton(if(group==null) "New folder" else "New subfolder", { editing = "create"; newName = "";preview=null }, enabled = !busy)
                            if (group != null) QuietButton("Rename folder", { editing = "rename"; newName = folder.substringAfterLast('/').ifEmpty {group.name};preview=null }, enabled = !busy)
                        }
                        if (group != null) {
                            FlattenDownloadFiles(id, key, api, onMoved = { revision++ }, onBusy = { busy = it })
                            QuietButton("Organize files",onFiles,enabled=!busy)
                        }
                    }
                    if(isOwner && group!=null) {
                            SecondaryButton(if (selecting) "Done selecting" else "Select videos", { selecting = !selecting; selected = emptyList() }, enabled = !busy)
                            if (selecting) {
                                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
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
                if(!collapsed) items(tree?.folders.orEmpty(),key={"folder:${it.folder}"},span={GridItemSpan(maxLineSpan)}) { child ->
                    Column { ListRow(child.name,horizontalPadding=0.dp,supporting="${child.itemCount} videos · ${services.format.bytes(child.size)}",onClick=if(busy)null else ({folder=child.folder;selected=emptyList();loading=true}),leading={LucideIcon(PopcornIcons.Folder,null)});RowDivider(inset=false) }
                }
                items(files, key = { it.id }) { item ->
                    Column {
                        if (selecting) CheckRow("Select ${item.title}", item.id in selected, { selected = if (item.id in selected) selected - item.id else selected + item.id }, enabled = !busy && (selected.size < 500 || item.id in selected))
                        MediaTile(item.title, services.format.tileLine(item), pictures.thumb(item, large = true), TileShape.Frame, com.thekozugroup.popcornstash.ui.icons.LibraryIcons.glyph("clapperboard"), { if (!busy) onOpen(item) }, progress = services.format.watchedFraction(item.progress))
                    }
                }
                if (files.isEmpty()) item("empty", span = { GridItemSpan(maxLineSpan) }) { Text(if (query.isBlank()) if(collapsed) "This download folder has no videos yet." else "Open a subfolder or turn on Collapse subfolders to see its videos." else "No videos match your search.", modifier = gutter, color = Popcorn.colors.textMuted) }
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
    if (editing != null) ConfirmDialog(title = if (editing == "create") "New folder" else "Rename folder", confirm = if(editing=="rename") {if(preview==null) "Preview rename" else "Rename on Atlas"} else "Create folder", onConfirm = ::saveName, onDismiss = { if (!busy) editing = null }, enabled = !busy, confirmEnabled = newName.isNotBlank() && !busy && preview?.blocked.orEmpty().isEmpty()) {
        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(if(editing=="rename") "Renames the actual folder on Atlas, including subfolders and other files. Video details and watch history stay with each video." else "Creates a folder on Atlas inside this location.", style = Popcorn.type.metadata)
            preview?.let { value ->
                Text("From: ${value.from}",style=Popcorn.type.metadata)
                Text("To: ${value.to}",style=Popcorn.type.metadata)
                Text("${value.fileCount} files in ${value.folderCount} folders",style=Popcorn.type.metadata)
                value.blocked.forEach{Text(it,color=Popcorn.colors.error,style=Popcorn.type.metadata)}
            }
            OutlinedTextField(newName, { newName = it;preview=null }, label = { Text("Folder name") }, singleLine = true, enabled = !busy, colors = popcornFieldColors(), shape = Popcorn.shapes.control)
            error?.let { Text(it, color = Popcorn.colors.error) }
        }
    }
}
