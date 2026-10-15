package com.thekozugroup.popcornstash.feature.library

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyGridScope
import androidx.compose.foundation.lazy.grid.LazyGridState
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.material3.DropdownMenu
import com.thekozugroup.popcornstash.ui.components.PopcornMenuItem
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.thekozugroup.popcornstash.data.api.Item
import com.thekozugroup.popcornstash.data.api.Layout
import com.thekozugroup.popcornstash.data.api.Library
import com.thekozugroup.popcornstash.data.api.MediaKind
import com.thekozugroup.popcornstash.data.api.PhotoMonth
import com.thekozugroup.popcornstash.data.api.ShowSummary
import com.thekozugroup.popcornstash.ui.components.EmptyState
import com.thekozugroup.popcornstash.ui.components.LoadFailed
import com.thekozugroup.popcornstash.ui.components.MediaTile
import com.thekozugroup.popcornstash.ui.components.PageTopBar
import com.thekozugroup.popcornstash.ui.components.PlaceholderTile
import com.thekozugroup.popcornstash.ui.components.SecondaryButton
import com.thekozugroup.popcornstash.ui.components.TileShape
import com.thekozugroup.popcornstash.ui.components.TopBarAction
import com.thekozugroup.popcornstash.ui.icons.LibraryIcons
import com.thekozugroup.popcornstash.ui.icons.LucideGlyph
import com.thekozugroup.popcornstash.ui.icons.LucideIcon
import com.thekozugroup.popcornstash.ui.icons.PopcornIcons
import com.thekozugroup.popcornstash.ui.media.LocalPictures
import com.thekozugroup.popcornstash.ui.theme.Popcorn
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.filter

/** What a library page does when tapped. */
class LibraryActions(
    val back: () -> Unit = {},
    val retry: () -> Unit = {},
    val loadMore: () -> Unit = {},
    val sort: (ItemSort) -> Unit = {},
    val openItem: (Item) -> Unit = {},
    val openShow: (String) -> Unit = {},
    val openPhoto: (Item) -> Unit = {},
    val play: (Item) -> Unit = {},
    val edit: () -> Unit = {},
)

/**
 * A library (android/DESIGN.md, A library): a grid in the library's own shape, newest first
 * (posters or frames, sortable), its shows for a series library, or the month timeline of a
 * photos library. Owners get "Edit library" in the top bar.
 */
@Composable
fun LibraryScreen(
    name: String,
    icon: String,
    ui: LibraryViewModel.Ui,
    isOwner: Boolean,
    bottomClearance: Dp,
    actions: LibraryActions,
    modifier: Modifier = Modifier,
    format: MediaFormat = MediaFormat(),
    gridState: LazyGridState = rememberLazyGridState(),
) {
    val library = ui.library
    val title = library?.name ?: name
    val glyph = LibraryIcons.glyph(library?.icon ?: icon)
    val content = ui.content
    Column(modifier.fillMaxSize()) {
        PageTopBar(
            title = title,
            onBack = actions.back,
            scrolled = gridState.canScrollBackward,
            actions = {
                if (content is LibraryViewModel.Content.Items && library != null) {
                    SortAction(content.sort, ItemSort.optionsFor(library.layout), actions.sort)
                }
                if (isOwner) TopBarAction(PopcornIcons.EditLibrary, "Edit library", actions.edit)
            },
        )
        Box(Modifier.fillMaxSize().testTag("library page")) {
            when (content) {
                LibraryViewModel.Content.Loading -> PlaceholderGrid(library?.layout ?: Layout.Posters, bottomClearance)
                is LibraryViewModel.Content.Failed -> LoadFailed(content.message, actions.retry, Modifier.align(Alignment.Center))
                is LibraryViewModel.Content.Items -> if (content.page.items.isEmpty()) {
                    EmptyLibrary(title, glyph, library, isOwner, actions.edit)
                } else {
                    ItemGrid(content, library?.layout ?: Layout.Posters, glyph, gridState, bottomClearance, format, actions)
                }
                is LibraryViewModel.Content.Shows -> if (content.shows.isEmpty()) {
                    EmptyLibrary(title, glyph, library, isOwner, actions.edit)
                } else {
                    ShowGrid(content.shows, glyph, gridState, bottomClearance, format, actions)
                }
                is LibraryViewModel.Content.Timeline -> if (content.page.items.isEmpty()) {
                    EmptyLibrary(title, glyph, library, isOwner, actions.edit)
                } else {
                    TimelineGrid(content.page, glyph, gridState, bottomClearance, format, actions)
                }
            }
        }
    }
}

/** "Sort" in the top bar: a menu of the library's orders, the chosen one ticked. */
@Composable
fun SortAction(current: ItemSort, options: List<ItemSort>, onSort: (ItemSort) -> Unit) {
    var open by remember { mutableStateOf(false) }
    Box {
        TopBarAction(PopcornIcons.Sort, "Sort", onClick = { open = true })
        DropdownMenu(
            expanded = open,
            onDismissRequest = { open = false },
            containerColor = Popcorn.colors.surface,
            shape = Popcorn.shapes.panel,
            border = BorderStroke(1.dp, Popcorn.colors.divider),
        ) {
            options.forEach { option ->
                PopcornMenuItem(
                    text = { Text(option.label, style = Popcorn.type.body) },
                    trailingIcon = {
                        if (option == current) LucideIcon(PopcornIcons.Chosen, contentDescription = null, tint = Popcorn.colors.text, size = 20.dp)
                    },
                    onClick = {
                        open = false
                        onSort(option)
                    },
                    modifier = Modifier.semantics { selected = option == current },
                )
            }
        }
    }
}

@Composable
private fun EmptyLibrary(name: String, glyph: LucideGlyph, library: Library?, isOwner: Boolean, onEdit: () -> Unit) {
    EmptyState(
        glyph = glyph,
        title = "Nothing in $name yet",
        body = library?.let(::emptyLibraryLine) ?: "Files sorted into $name show up here.",
        modifier = Modifier.fillMaxSize().padding(bottom = 96.dp),
        action = if (isOwner) {
            { SecondaryButton("Edit library", onClick = onEdit) }
        } else {
            null
        },
    )
}

/** Asks for the next page once the person is within a few rows of the end. */
@Composable
private fun LoadMoreNearEnd(state: LazyGridState, onLoadMore: () -> Unit) {
    val loadMore by rememberUpdatedState(onLoadMore)
    LaunchedEffect(state) {
        snapshotFlow {
            val info = state.layoutInfo
            val last = info.visibleItemsInfo.lastOrNull()?.index ?: 0
            info.totalItemsCount > 0 && last >= info.totalItemsCount - 12
        }.distinctUntilChanged().filter { it }.collect { loadMore() }
    }
}

private fun LazyGridScope.moreFooter(page: Paged<*>, onLoadMore: () -> Unit) {
    val failed = page.moreFailed
    if (failed != null) {
        item(key = "more failed", span = { GridItemSpan(maxLineSpan) }) { LoadFailed(failed, onLoadMore) }
    }
}

@Composable
private fun ItemGrid(
    content: LibraryViewModel.Content.Items,
    layout: Layout,
    glyph: LucideGlyph,
    state: LazyGridState,
    bottomClearance: Dp,
    format: MediaFormat,
    actions: LibraryActions,
) {
    val shape = layout.tileShape()
    val pictures = LocalPictures.current
    LoadMoreNearEnd(state, actions.loadMore)
    LazyVerticalGrid(
        columns = GridCells.Adaptive(if (shape == TileShape.Frame) Popcorn.media.gridFrameMin else Popcorn.media.gridPosterMin),
        state = state,
        contentPadding = gridPadding(bottomClearance),
        horizontalArrangement = Arrangement.spacedBy(Popcorn.media.gridGap),
        verticalArrangement = Arrangement.spacedBy(Popcorn.media.gridRowGap),
        modifier = Modifier.fillMaxSize().testTag("library grid"),
    ) {
        items(content.page.items, key = { it.id }) { item ->
            MediaTile(
                title = format.displayTitle(item),
                line = format.tileLine(item),
                url = pictures.thumb(item, large = true),
                shape = shape,
                glyph = glyph,
                progress = format.watchedFraction(item.progress),
                onClick = { actions.openItem(item) },
            )
        }
        if (content.page.loadingMore) {
            items(3, key = { "more $it" }) { PlaceholderTile(shape) }
        }
        moreFooter(content.page, actions.loadMore)
    }
}

@Composable
private fun ShowGrid(
    shows: List<ShowSummary>,
    glyph: LucideGlyph,
    state: LazyGridState,
    bottomClearance: Dp,
    format: MediaFormat,
    actions: LibraryActions,
) {
    val pictures = LocalPictures.current
    LazyVerticalGrid(
        columns = GridCells.Adaptive(Popcorn.media.gridPosterMin),
        state = state,
        contentPadding = gridPadding(bottomClearance),
        horizontalArrangement = Arrangement.spacedBy(Popcorn.media.gridGap),
        verticalArrangement = Arrangement.spacedBy(Popcorn.media.gridRowGap),
        modifier = Modifier.fillMaxSize().testTag("library grid"),
    ) {
        items(shows, key = { it.showName }) { show ->
            MediaTile(
                title = show.showName,
                line = showLine(show, format),
                url = show.posterItemId?.let { pictures.thumbOf(it, large = true) },
                shape = TileShape.Poster,
                glyph = glyph,
                onClick = { actions.openShow(show.showName) },
            )
        }
    }
}

/** "3 seasons" for a show with more than one, otherwise "12 episodes". */
fun showLine(show: ShowSummary, format: MediaFormat): String =
    if (show.seasonCount > 1) format.count(show.seasonCount, "season") else format.count(show.itemCount, "episode")

/**
 * The photo timeline: month headings with their count, square tiles four across with 3dp
 * gaps from edge to edge. A photo opens the full-screen viewer; a phone clip plays.
 */
@Composable
private fun TimelineGrid(
    page: Paged<PhotoMonth>,
    glyph: LucideGlyph,
    state: LazyGridState,
    bottomClearance: Dp,
    format: MediaFormat,
    actions: LibraryActions,
) {
    val gap = Popcorn.media.photoGap
    LoadMoreNearEnd(state, actions.loadMore)
    LazyVerticalGrid(
        columns = GridCells.Adaptive(Popcorn.media.gridPhotoMin),
        state = state,
        contentPadding = PaddingValues(bottom = bottomClearance),
        horizontalArrangement = Arrangement.spacedBy(gap),
        verticalArrangement = Arrangement.spacedBy(gap),
        modifier = Modifier.fillMaxSize().testTag("timeline"),
    ) {
        page.items.forEachIndexed { index, month ->
            item(key = "month ${month.month}", span = { GridItemSpan(maxLineSpan) }, contentType = "month") {
                MonthHeading(month, first = index == 0, format = format)
            }
            items(month.items, key = { it.id }, contentType = { "photo" }) { photo ->
                PhotoTile(photo, format.displayTitle(photo), glyph, cornered = false) {
                    if (photo.kind == MediaKind.Video) actions.play(photo) else actions.openPhoto(photo)
                }
            }
        }
        if (page.loadingMore) {
            items(8, key = { "more $it" }) { PlaceholderTile(TileShape.Square, caption = false, cornered = false) }
        }
        moreFooter(page, actions.loadMore)
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun MonthHeading(month: PhotoMonth, first: Boolean, format: MediaFormat) {
    // The month as the server groups it ("2026-07" is July 2026 on the camera's clock), never
    // moved to this phone's time zone.
    val label = format.month(month.month)
    val count = format.count(month.count, "photo")
    // The count follows the month while there is room and goes under it when there is not
    // (large text): neither is ever broken inside a word.
    FlowRow(
        Modifier
            .fillMaxWidth()
            .padding(
                start = Popcorn.spacing.gutter,
                end = Popcorn.spacing.gutter,
                top = if (first) Popcorn.spacing.xs else Popcorn.spacing.l,
                bottom = Popcorn.spacing.s - Popcorn.media.photoGap,
            )
            .clearAndSetSemantics {
                contentDescription = "$label, $count"
                heading()
            },
        horizontalArrangement = Arrangement.spacedBy(Popcorn.spacing.xs),
        itemVerticalAlignment = Alignment.Bottom,
    ) {
        Text(label, style = Popcorn.type.title, color = Popcorn.colors.text)
        Text(
            MediaFormat.unbroken(count),
            style = Popcorn.type.metadata,
            color = Popcorn.colors.textMuted,
            maxLines = 1,
            modifier = Modifier.padding(bottom = 3.dp),
        )
    }
}

/** A grid of placeholder tiles in the library's shape while it loads. */
@Composable
private fun PlaceholderGrid(layout: Layout, bottomClearance: Dp) {
    val shape = layout.tileShape()
    val photos = shape == TileShape.Square
    LazyVerticalGrid(
        columns = GridCells.Adaptive(
            when (shape) {
                TileShape.Square -> Popcorn.media.gridPhotoMin
                TileShape.Frame -> Popcorn.media.gridFrameMin
                TileShape.Poster -> Popcorn.media.gridPosterMin
            },
        ),
        userScrollEnabled = false,
        contentPadding = if (photos) PaddingValues(top = 48.dp, bottom = bottomClearance) else gridPadding(bottomClearance),
        horizontalArrangement = Arrangement.spacedBy(if (photos) Popcorn.media.photoGap else Popcorn.media.gridGap),
        verticalArrangement = Arrangement.spacedBy(if (photos) Popcorn.media.photoGap else Popcorn.media.gridRowGap),
        modifier = Modifier.fillMaxSize().clearAndSetSemantics { contentDescription = "Loading" },
    ) {
        items(if (photos) 24 else 12) { PlaceholderTile(shape, caption = !photos, cornered = !photos) }
    }
}

@Composable
private fun gridPadding(bottomClearance: Dp) = PaddingValues(
    start = Popcorn.spacing.gutter,
    end = Popcorn.spacing.gutter,
    top = Popcorn.spacing.xs,
    bottom = bottomClearance,
)
