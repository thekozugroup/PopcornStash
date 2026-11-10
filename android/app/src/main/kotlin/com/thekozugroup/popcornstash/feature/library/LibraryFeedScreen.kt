package com.thekozugroup.popcornstash.feature.library

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.thekozugroup.popcornstash.data.api.DownloadFolder
import com.thekozugroup.popcornstash.data.api.Item
import com.thekozugroup.popcornstash.data.api.Library
import com.thekozugroup.popcornstash.feature.home.ResumeCarousel
import com.thekozugroup.popcornstash.ui.components.ConnectionLostRow
import com.thekozugroup.popcornstash.ui.components.EmptyState
import com.thekozugroup.popcornstash.ui.components.LoadFailed
import com.thekozugroup.popcornstash.ui.components.PageTopBar
import com.thekozugroup.popcornstash.ui.components.PlaceholderShelf
import com.thekozugroup.popcornstash.ui.components.PopcornMenuItem
import com.thekozugroup.popcornstash.ui.components.SecondaryButton
import com.thekozugroup.popcornstash.ui.components.TileShape
import com.thekozugroup.popcornstash.ui.components.TopBarAction
import com.thekozugroup.popcornstash.ui.icons.LibraryIcons
import com.thekozugroup.popcornstash.ui.icons.LucideIcon
import com.thekozugroup.popcornstash.ui.icons.PopcornIcons
import com.thekozugroup.popcornstash.ui.theme.Popcorn
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.filter

enum class VideoLibraryView(val label: String) { Feed("Feed"), Folders("Folder view"), All("All videos") }

@Composable
fun VideoLibraryMenu(current: VideoLibraryView, onView: (VideoLibraryView) -> Unit) {
    var open by remember { mutableStateOf(false) }
    Box {
        TopBarAction(PopcornIcons.More, "Library view", { open = true })
        DropdownMenu(open, { open = false }, shape = Popcorn.shapes.panel, containerColor = Popcorn.colors.surface,
            border = BorderStroke(1.dp, Popcorn.colors.divider)) {
            VideoLibraryView.entries.forEach { view ->
                PopcornMenuItem(
                    text = { Text(view.label, style = Popcorn.type.body) },
                    trailingIcon = { if (view == current) LucideIcon(PopcornIcons.Chosen, null, size = 20.dp) },
                    modifier = Modifier.semantics { selected = view == current },
                    onClick = { open = false; onView(view) },
                )
            }
        }
    }
}

class LibraryFeedActions(
    val back: () -> Unit = {},
    val view: (VideoLibraryView) -> Unit = {},
    val retry: () -> Unit = {},
    val loadMore: () -> Unit = {},
    val openItem: (Item) -> Unit = {},
    val openCollection: (DownloadFolder) -> Unit = {},
    val resume: (Item) -> Unit = {},
    val startOver: (Item) -> Unit = {},
    val search: () -> Unit = {},
    val edit: () -> Unit = {},
)

/** A lazy vertical feed; each collection remains a bounded horizontal shelf. */
@Composable
fun LibraryFeedScreen(
    library: Library,
    ui: LibraryFeedViewModel.Ui,
    bottomClearance: Dp,
    actions: LibraryFeedActions,
    isOwner: Boolean = false,
    connectionLost: Boolean = false,
    format: MediaFormat = MediaFormat(),
    listState: LazyListState = rememberLazyListState(),
) {
    val shape = library.layout.tileShape()
    val feed = ui.feed
    val loadMore by rememberUpdatedState(actions.loadMore)
    LaunchedEffect(listState, feed?.nextCursor, ui.moreFailed) {
        if (feed?.nextCursor == null || ui.moreFailed != null) return@LaunchedEffect
        snapshotFlow {
            val info = listState.layoutInfo
            info.totalItemsCount > 0 && (info.visibleItemsInfo.lastOrNull()?.index ?: -1) >= info.totalItemsCount - 3
        }.distinctUntilChanged().filter { it }.collect { loadMore() }
    }
    Column(Modifier.fillMaxSize()) {
        PageTopBar(library.name, onBack = actions.back, scrolled = listState.canScrollBackward, actions = {
            TopBarAction(PopcornIcons.Search, "Search library", actions.search)
            if (isOwner) TopBarAction(PopcornIcons.EditLibrary, "Edit library", actions.edit)
            VideoLibraryMenu(VideoLibraryView.Feed, actions.view)
        })
        LazyColumn(state = listState, modifier = Modifier.fillMaxSize().testTag("library feed"),
            contentPadding = PaddingValues(bottom = bottomClearance)) {
            if (connectionLost) item("offline") { ConnectionLostRow() }
            when {
                feed == null && ui.error != null -> item("failed") { LoadFailed(ui.error, actions.retry, Modifier.fillParentMaxHeight(0.7f)) }
                feed == null -> item("loading") {
                    Column {
                        Spacer(Modifier.height(Popcorn.spacing.m))
                        PlaceholderShelf(TileShape.Frame)
                        Spacer(Modifier.height(Popcorn.spacing.xxl))
                        PlaceholderShelf(shape)
                    }
                }
                else -> {
                    if (ui.error != null) item("refresh failed") { LoadFailed(ui.error, actions.retry) }
                    if (feed.continueWatching.isNotEmpty()) item("continue") {
                        Column {
                            Text("Continue watching", style = Popcorn.type.title, color = Popcorn.colors.text,
                                modifier = Modifier.padding(horizontal = Popcorn.spacing.gutter, vertical = Popcorn.spacing.s).semantics { heading() })
                            ResumeCarousel(feed.continueWatching, format, { library.icon }, actions.resume, actions.openItem, actions.startOver)
                            Spacer(Modifier.height(Popcorn.spacing.xxl - Popcorn.spacing.s))
                        }
                    }
                    if (feed.suggested.isNotEmpty()) item("suggested") {
                        VideoShelf("Suggested for you", feed.suggested, format, actions.openItem, shape, { library.icon })
                        Spacer(Modifier.height(Popcorn.spacing.xxl - Popcorn.spacing.s))
                    }
                    if (feed.recent.isNotEmpty()) item("recent") {
                        VideoShelf("Recently added", feed.recent, format, actions.openItem, shape, { library.icon }, onSeeAll = { actions.view(VideoLibraryView.All) })
                        Spacer(Modifier.height(Popcorn.spacing.xxl - Popcorn.spacing.s))
                    }
                    items(feed.collections, key = { "collection ${it.folder.key}" }) { collection ->
                        VideoShelf(collection.folder.name, collection.recent, format, actions.openItem, shape, { library.icon },
                            count = format.count(collection.folder.itemCount.toLong(), "video"), onSeeAll = { actions.openCollection(collection.folder) })
                        Spacer(Modifier.height(Popcorn.spacing.xxl - Popcorn.spacing.s))
                    }
                    if (feed.continueWatching.isEmpty() && feed.suggested.isEmpty() && feed.recent.isEmpty() && feed.collections.isEmpty()) item("empty") {
                        EmptyState(LibraryIcons.glyph(library.icon), "Nothing in ${library.name} yet", emptyLibraryLine(library), Modifier.fillParentMaxHeight(0.7f))
                    }
                    if (feed.nextCursor != null) item("more") {
                        Column(Modifier.padding(horizontal = Popcorn.spacing.gutter)) {
                            ui.moreFailed?.let { Text(it, style = Popcorn.type.body, color = Popcorn.colors.error) }
                            SecondaryButton(if (ui.moreFailed == null) "More collections" else "Try again", actions.loadMore, enabled = !ui.loadingMore && !ui.loading, busy = ui.loadingMore)
                        }
                    }
                }
            }
        }
    }
}
