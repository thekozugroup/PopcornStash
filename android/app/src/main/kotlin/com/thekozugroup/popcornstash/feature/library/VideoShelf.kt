package com.thekozugroup.popcornstash.feature.library

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.thekozugroup.popcornstash.data.api.Item
import com.thekozugroup.popcornstash.ui.components.MediaTile
import com.thekozugroup.popcornstash.ui.components.Shelf
import com.thekozugroup.popcornstash.ui.components.TileShape
import com.thekozugroup.popcornstash.ui.components.shelfTileWidth
import com.thekozugroup.popcornstash.ui.icons.LibraryIcons
import com.thekozugroup.popcornstash.ui.media.LocalPictures
import com.thekozugroup.popcornstash.ui.theme.Popcorn

/** Feed rows reuse the library's existing picture shapes, spacing and snapping shelves. */
@Composable
fun VideoShelf(
    title: String,
    items: List<Item>,
    format: MediaFormat,
    onOpen: (Item) -> Unit,
    shape: TileShape = TileShape.Frame,
    libraryIcon: (Item) -> String? = { null },
    count: String? = null,
    onSeeAll: (() -> Unit)? = null,
) {
    val pictures = LocalPictures.current
    val fontScale = LocalDensity.current.fontScale.coerceAtLeast(1f)
    Column {
        Row(
            Modifier.fillMaxWidth().heightIn(min = 56.dp)
                .then(if (onSeeAll != null) Modifier.clickable(role = Role.Button, onClickLabel = "See all $title", onClick = onSeeAll) else Modifier)
                .padding(horizontal = Popcorn.spacing.gutter)
                .semantics(mergeDescendants = true) { heading() },
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(Modifier.weight(1f)) {
                Text(title, style = Popcorn.type.title, color = Popcorn.colors.text, maxLines = 2, overflow = TextOverflow.Ellipsis)
                if (count != null) Text(count, style = Popcorn.type.metadata, color = Popcorn.colors.textMuted)
            }
            if (onSeeAll != null) {
                Spacer(Modifier.width(Popcorn.spacing.s))
                Text("See all", style = Popcorn.type.action, color = Popcorn.colors.link)
            }
        }
        Spacer(Modifier.height(Popcorn.spacing.s))
        if (items.isEmpty()) {
            Text("No videos yet", style = Popcorn.type.metadata, color = Popcorn.colors.textMuted, modifier = Modifier.padding(horizontal = Popcorn.spacing.gutter))
        } else {
            BoxWithConstraints(Modifier.fillMaxWidth()) {
                val width = (shelfTileWidth(shape) * fontScale).coerceAtMost(maxWidth - Popcorn.spacing.gutter * 2)
                Shelf(Modifier.testTag("shelf $title")) {
                    items(items, key = { it.id }) { item ->
                        MediaTile(
                            title = format.displayTitle(item),
                            line = format.tileLine(item),
                            url = pictures.thumb(item, large = true),
                            shape = shape,
                            glyph = LibraryIcons.glyph(libraryIcon(item) ?: "clapperboard"),
                            onClick = { onOpen(item) },
                            progress = format.watchedFraction(item.progress),
                            modifier = Modifier.width(width),
                        )
                    }
                }
            }
        }
    }
}
