package com.thekozugroup.popcornstash.ui.components

import androidx.compose.foundation.gestures.snapping.SnapPosition
import androidx.compose.foundation.gestures.snapping.rememberSnapFlingBehavior
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.Dp
import com.thekozugroup.popcornstash.ui.theme.Popcorn

/** How wide a shelf's tiles are for each picture shape (android/DESIGN.md, Tiles and shelves). */
@Composable
fun shelfTileWidth(shape: TileShape): Dp = when (shape) {
    TileShape.Poster -> Popcorn.media.shelfPoster
    TileShape.Frame -> Popcorn.media.shelfFrame
    TileShape.Square -> Popcorn.media.shelfPhoto
}

/**
 * A row of tiles that scrolls sideways edge to edge, keeping the page gutter as its padding,
 * and settles with a tile at the start.
 */
@Composable
fun Shelf(modifier: Modifier = Modifier, content: LazyListScope.() -> Unit) {
    val state = rememberLazyListState()
    LazyRow(
        state = state,
        contentPadding = PaddingValues(horizontal = Popcorn.spacing.gutter),
        horizontalArrangement = Arrangement.spacedBy(Popcorn.media.shelfGap),
        flingBehavior = rememberSnapFlingBehavior(state, SnapPosition.Start),
        modifier = modifier,
        content = content,
    )
}

/** A shelf while Home loads: [count] tiles of [shape] in `subtle`. */
@Composable
fun PlaceholderShelf(shape: TileShape, count: Int = 4, caption: Boolean = true) {
    val width = shelfTileWidth(shape)
    LazyRow(
        contentPadding = PaddingValues(horizontal = Popcorn.spacing.gutter),
        horizontalArrangement = Arrangement.spacedBy(Popcorn.media.shelfGap),
        userScrollEnabled = false,
    ) {
        items(count) { PlaceholderTile(shape, Modifier.width(width), caption = caption) }
    }
}
