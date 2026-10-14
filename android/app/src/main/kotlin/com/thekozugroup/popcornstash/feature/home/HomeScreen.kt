package com.thekozugroup.popcornstash.feature.home

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.wrapContentHeight
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.thekozugroup.popcornstash.data.api.Home
import com.thekozugroup.popcornstash.data.api.HomeLibrary
import com.thekozugroup.popcornstash.data.api.Item
import com.thekozugroup.popcornstash.data.api.Library
import com.thekozugroup.popcornstash.data.repo.Loadable
import com.thekozugroup.popcornstash.feature.library.MediaFormat
import com.thekozugroup.popcornstash.feature.library.PhotoTile
import com.thekozugroup.popcornstash.feature.library.displayTitle
import com.thekozugroup.popcornstash.feature.library.libraryCountLine
import com.thekozugroup.popcornstash.feature.library.tileShape
import com.thekozugroup.popcornstash.navigation.TabPage
import com.thekozugroup.popcornstash.ui.components.ConnectionLostRow
import com.thekozugroup.popcornstash.ui.components.EmptyState
import com.thekozugroup.popcornstash.ui.components.LoadFailed
import com.thekozugroup.popcornstash.ui.components.MediaTile
import com.thekozugroup.popcornstash.ui.components.PlaceholderLine
import com.thekozugroup.popcornstash.ui.components.PlaceholderShelf
import com.thekozugroup.popcornstash.ui.components.PlaceholderTile
import com.thekozugroup.popcornstash.ui.components.PrimaryButton
import com.thekozugroup.popcornstash.ui.components.Shelf
import com.thekozugroup.popcornstash.ui.components.TabRootScaffold
import com.thekozugroup.popcornstash.ui.components.TileShape
import com.thekozugroup.popcornstash.ui.components.shelfTileWidth
import com.thekozugroup.popcornstash.ui.icons.LibraryIcons
import com.thekozugroup.popcornstash.ui.icons.LucideGlyph
import com.thekozugroup.popcornstash.ui.icons.LucideIcon
import com.thekozugroup.popcornstash.ui.icons.PopcornIcons
import com.thekozugroup.popcornstash.ui.media.LocalPictures
import com.thekozugroup.popcornstash.ui.theme.Popcorn

/** What Home's tiles and rows do when tapped. */
class HomeActions(
    val resume: (Item) -> Unit = {},
    val startOver: (Item) -> Unit = {},
    val openItem: (Item) -> Unit = {},
    val openLibrary: (Library) -> Unit = {},
    val openShow: (Library, String) -> Unit = { _, _ -> },
    val openPhoto: (Library, Item) -> Unit = { _, _ -> },
    val openOtherFiles: () -> Unit = {},
    val makeLibrary: () -> Unit = {},
    val retry: () -> Unit = {},
)

/**
 * Home, the start tab (android/DESIGN.md, Home): continue watching when there is something
 * to resume, then every library the person can see in the owner's order, each with its
 * icon, name and a shelf of its newest items, then Other files when the person has any.
 */
@Composable
fun HomeScreen(
    page: TabPage,
    home: Loadable<Home>,
    isOwner: Boolean,
    actions: HomeActions,
    modifier: Modifier = Modifier,
    connectionLost: Boolean = false,
    format: MediaFormat = MediaFormat(),
) {
    val value = home.valueOrNull
    TabRootScaffold(page = page, title = "Home", modifier = modifier) {
        if (connectionLost) item(key = "offline") { ConnectionLostRow() }
        when {
            value == null && home is Loadable.Failed -> item(key = "failed") {
                LoadFailed(home.message, actions.retry, Modifier.fillParentMaxHeight(0.7f).wrapContentHeight())
            }
            value == null -> item(key = "loading") { HomePlaceholder() }
            value.libraries.isEmpty() -> item(key = "empty") {
                NoLibraries(isOwner, actions.makeLibrary, Modifier.fillParentMaxHeight(0.75f).wrapContentHeight())
            }
            else -> {
                item(key = "top") { Spacer(Modifier.height(Popcorn.spacing.xs)) }
                if (value.continueWatching.isNotEmpty()) {
                    item(key = "continue") {
                        val icons = value.libraries.associate { it.library.id to it.library.icon }
                        Column {
                            Text(
                                "Continue watching",
                                style = Popcorn.type.title,
                                color = Popcorn.colors.text,
                                modifier = Modifier
                                    .padding(horizontal = Popcorn.spacing.gutter)
                                    .semantics { heading() },
                            )
                            Spacer(Modifier.height(Popcorn.spacing.s))
                            ResumeCarousel(
                                items = value.continueWatching,
                                format = format,
                                libraryIcon = { icons[it.libraryId] },
                                onResume = actions.resume,
                                onDetails = actions.openItem,
                                onFromStart = actions.startOver,
                            )
                            Spacer(Modifier.height(Popcorn.spacing.xxl - Popcorn.spacing.s))
                        }
                    }
                }
                items(value.libraries, key = { "library ${it.library.id}" }) { entry ->
                    LibrarySection(entry, format, actions)
                    Spacer(Modifier.height(Popcorn.spacing.xxl - Popcorn.spacing.s))
                }
                if (value.otherFilesCount > 0) {
                    item(key = "other files") {
                        HeaderRow(
                            glyph = PopcornIcons.OtherFiles,
                            name = "Other files",
                            count = format.count(value.otherFilesCount, "file"),
                            onClick = actions.openOtherFiles,
                        )
                    }
                }
            }
        }
    }
}

/** A library on Home: its header row, then a shelf of its newest items in its own shape. */
@Composable
private fun LibrarySection(entry: HomeLibrary, format: MediaFormat, actions: HomeActions) {
    val library = entry.library
    val glyph = LibraryIcons.glyph(library.icon)
    val pictures = LocalPictures.current
    Column(Modifier.testTag("library ${library.name}")) {
        val empty = library.itemCount == 0L && (library.showCount ?: 0L) == 0L
        HeaderRow(glyph, library.name, if (empty) null else format.libraryCountLine(library), onClick = { actions.openLibrary(library) })
        val tiles = shelfTiles(entry)
        if (tiles.isEmpty()) {
            Text(
                "Nothing here yet",
                style = Popcorn.type.metadata,
                color = Popcorn.colors.textMuted,
                modifier = Modifier.padding(horizontal = Popcorn.spacing.gutter),
            )
            return@Column
        }
        // The header row's own padding already parts the name from the shelf.
        Spacer(Modifier.height(Popcorn.spacing.xxs))
        val shape = library.layout.tileShape()
        val width = shelfTileWidth(shape)
        Shelf {
            items(tiles, key = { it.key }) { tile ->
                when (tile) {
                    is ShelfTile.Show -> MediaTile(
                        title = tile.name,
                        line = format.showTileLine(tile),
                        url = tile.posterId?.let { pictures.thumbOf(it, large = true) } ?: pictures.thumb(tile.latest, large = true),
                        shape = shape,
                        glyph = glyph,
                        onClick = { actions.openShow(library, tile.name) },
                        modifier = Modifier.width(width),
                    )
                    is ShelfTile.Single -> {
                        val item = tile.item
                        if (shape == TileShape.Square) {
                            PhotoTile(item, format.displayTitle(item), glyph, Modifier.width(width)) { actions.openPhoto(library, item) }
                        } else {
                            MediaTile(
                                title = format.displayTitle(item),
                                line = format.tileLine(item),
                                url = pictures.thumb(item, large = true),
                                shape = shape,
                                glyph = glyph,
                                progress = format.watchedFraction(item.progress),
                                onClick = { actions.openItem(item) },
                                modifier = Modifier.width(width),
                            )
                        }
                    }
                }
            }
        }
    }
}

/**
 * A 56dp row that opens a place: its glyph in `textMuted`, its name, a count beside it and a
 * trailing chevron; the whole row is one target.
 */
@Composable
fun HeaderRow(glyph: LucideGlyph, name: String, count: String?, onClick: () -> Unit, modifier: Modifier = Modifier) {
    val colors = Popcorn.colors
    Row(
        modifier
            .fillMaxWidth()
            .heightIn(min = 56.dp)
            .clickable(onClickLabel = "Open", role = Role.Button, onClick = onClick)
            .padding(horizontal = Popcorn.spacing.gutter)
            .semantics(mergeDescendants = true) { },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        LucideIcon(glyph, contentDescription = null, tint = colors.textMuted)
        Spacer(Modifier.width(Popcorn.spacing.s))
        // The name gives way (with an ellipsis) before the count does; the chevron keeps the end.
        Row(Modifier.weight(1f), verticalAlignment = Alignment.CenterVertically) {
            Text(
                name,
                style = Popcorn.type.section,
                color = colors.text,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f, fill = false).semantics { heading() },
            )
            if (count != null) {
                Spacer(Modifier.width(Popcorn.spacing.xs))
                Text(count, style = Popcorn.type.metadata, color = colors.textMuted, maxLines = 1)
            }
        }
        Spacer(Modifier.width(Popcorn.spacing.s))
        // Sits on the gutter, lined up with the shelf's right edge on a phone.
        LucideIcon(PopcornIcons.Open, contentDescription = null, tint = colors.textMuted, size = 20.dp)
    }
}

/** No library to show: members are told it comes when shared; owners can make one. */
@Composable
private fun NoLibraries(isOwner: Boolean, onMake: () -> Unit, modifier: Modifier) {
    if (isOwner) {
        EmptyState(
            glyph = PopcornIcons.Home,
            title = "No libraries yet",
            body = "A library is a folder of videos or photos with a name and an icon. Make one to see it here.",
            modifier = modifier,
            action = { PrimaryButton("Make a library", onClick = onMake) },
        )
    } else {
        EmptyState(
            glyph = PopcornIcons.Home,
            title = "Nothing has been shared with you yet",
            body = "Libraries appear here once the owner shares them with you.",
            modifier = modifier,
        )
    }
}

/** Home while it loads: the carousel's card and two shelves in `subtle`, at their final sizes. */
@Composable
private fun HomePlaceholder() {
    val gutter = Popcorn.spacing.gutter
    Column(Modifier.clearAndSetSemantics { contentDescription = "Loading" }) {
        Spacer(Modifier.height(Popcorn.spacing.xs))
        PlaceholderLine(Modifier.padding(horizontal = gutter).width(160.dp), height = 20.dp)
        Spacer(Modifier.height(Popcorn.spacing.s))
        PlaceholderTile(
            TileShape.Frame,
            Modifier.padding(start = gutter, end = gutter + Popcorn.carousel.peek),
        )
        listOf(TileShape.Poster, TileShape.Frame).forEach { shape ->
            Spacer(Modifier.height(Popcorn.spacing.xxl))
            Row(Modifier.padding(horizontal = gutter).heightIn(min = 32.dp), verticalAlignment = Alignment.CenterVertically) {
                PlaceholderLine(Modifier.width(24.dp), height = 24.dp)
                Spacer(Modifier.width(Popcorn.spacing.s))
                PlaceholderLine(Modifier.width(120.dp), height = 16.dp)
            }
            Spacer(Modifier.height(Popcorn.spacing.m))
            PlaceholderShelf(shape)
        }
    }
}
