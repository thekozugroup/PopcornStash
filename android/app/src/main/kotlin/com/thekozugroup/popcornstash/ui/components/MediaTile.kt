package com.thekozugroup.popcornstash.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.thekozugroup.popcornstash.ui.icons.LucideGlyph
import com.thekozugroup.popcornstash.ui.icons.LucideIcon
import com.thekozugroup.popcornstash.ui.media.StashImage
import com.thekozugroup.popcornstash.ui.theme.IconStroke
import com.thekozugroup.popcornstash.ui.theme.Popcorn

/** The shapes a picture takes (android/DESIGN.md, Tiles and shelves): width over height. */
enum class TileShape(val ratio: Float) {
    /** 2:3, for one-offs that are films and for shows. */
    Poster(2f / 3f),

    /** 16:9, for camera videos, continue watching and episodes. */
    Frame(16f / 9f),

    /** 1:1, for photos. */
    Square(1f),
}

/**
 * A picture as a tile shows it: cropped to [shape], 10dp corners, the 1dp hairline inside the
 * edge, and while there is no picture the `subtle` fill with [glyph] at 22dp. [progress]
 * (0 to 1) draws the watched part along the bottom.
 */
@Composable
fun MediaPicture(
    url: String?,
    shape: TileShape,
    glyph: LucideGlyph,
    modifier: Modifier = Modifier,
    progress: Float? = null,
    cornered: Boolean = true,
    overlay: @Composable BoxScope.() -> Unit = {},
) {
    val colors = Popcorn.colors
    val media = Popcorn.media
    val corners = if (cornered) Popcorn.shapes.media else RectangleShape
    Box(
        modifier
            .aspectRatio(shape.ratio)
            .clip(corners)
            .background(colors.subtle)
            .border(1.dp, colors.mediaHairline, corners),
    ) {
        StashImage(
            url = url,
            contentDescription = null,
            modifier = Modifier.fillMaxSize(),
            placeholder = {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    LucideIcon(glyph, contentDescription = null, tint = colors.textMuted, size = media.placeholderIcon, strokeWidth = IconStroke.Large)
                }
            },
        )
        if (progress != null) WatchedBar(progress, Modifier.align(Alignment.BottomCenter))
        overlay()
    }
}

/** The watched part of a video: a 4dp Action Blue bar over a zinc-950 40% track. */
@Composable
fun WatchedBar(fraction: Float, modifier: Modifier = Modifier) {
    val colors = Popcorn.colors
    Box(
        modifier
            .fillMaxWidth()
            .height(Popcorn.media.progressHeight)
            .background(colors.mediaProgressTrack),
    ) {
        Box(
            Modifier
                .fillMaxHeight()
                .fillMaxWidth(fraction.coerceIn(0f, 1f))
                .background(colors.accent),
        )
    }
}

/**
 * A tile: the picture, then 10dp down its title (two lines, then an ellipsis) and one
 * metadata line. The whole tile is one target; a screen reader hears the title, the line
 * and how much was watched.
 */
@Composable
fun MediaTile(
    title: String,
    line: String?,
    url: String?,
    shape: TileShape,
    glyph: LucideGlyph,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    progress: Float? = null,
    onClickLabel: String? = null,
) {
    Column(
        modifier
            .clip(Popcorn.shapes.media)
            .clickable(onClickLabel = onClickLabel, role = Role.Button, onClick = onClick)
            .semantics(mergeDescendants = true) {
                if (progress != null) stateDescription = "${(progress * 100).toInt()}% watched"
            },
    ) {
        MediaPicture(url, shape, glyph, Modifier.fillMaxWidth(), progress = progress)
        Spacer(Modifier.height(Popcorn.media.captionGap))
        Text(
            title,
            style = Popcorn.type.item,
            color = Popcorn.colors.text,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
        )
        if (!line.isNullOrBlank()) {
            Text(
                line,
                style = Popcorn.type.metadata,
                color = Popcorn.colors.textMuted,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        Spacer(Modifier.height(2.dp))
    }
}

/** A tile while its page loads: the picture's place in `subtle` and two short lines. */
@Composable
fun PlaceholderTile(shape: TileShape, modifier: Modifier = Modifier, caption: Boolean = true, cornered: Boolean = true) {
    Column(modifier) {
        Box(
            Modifier
                .fillMaxWidth()
                .aspectRatio(shape.ratio)
                .background(Popcorn.colors.subtle, if (cornered) Popcorn.shapes.media else RectangleShape),
        )
        if (caption) {
            Spacer(Modifier.height(Popcorn.media.captionGap + 4.dp))
            PlaceholderLine(Modifier.fillMaxWidth(0.8f))
            Spacer(Modifier.height(8.dp))
            PlaceholderLine(Modifier.fillMaxWidth(0.5f))
        }
    }
}

/** A line of text that is not there yet. */
@Composable
fun PlaceholderLine(modifier: Modifier = Modifier, height: Dp = 12.dp, color: Color = Popcorn.colors.subtle) {
    Box(modifier.height(height).background(color, Popcorn.shapes.tag))
}
