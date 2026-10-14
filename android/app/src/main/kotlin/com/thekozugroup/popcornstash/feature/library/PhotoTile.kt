package com.thekozugroup.popcornstash.feature.library

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.role
import androidx.compose.ui.unit.dp
import com.thekozugroup.popcornstash.data.api.Item
import com.thekozugroup.popcornstash.data.api.MediaKind
import com.thekozugroup.popcornstash.ui.components.MediaPicture
import com.thekozugroup.popcornstash.ui.components.TileShape
import com.thekozugroup.popcornstash.ui.icons.LucideGlyph
import com.thekozugroup.popcornstash.ui.icons.LucideIcon
import com.thekozugroup.popcornstash.ui.icons.PopcornIcons
import com.thekozugroup.popcornstash.ui.media.LocalPictures
import com.thekozugroup.popcornstash.ui.theme.Popcorn

/**
 * A photo as a square with no caption (android/DESIGN.md, Tiles and shelves). A phone clip
 * kept with the photos carries a small play disc so it reads as a video before it is tapped.
 * A screen reader hears its date, and that it is a video.
 */
@Composable
fun PhotoTile(
    item: Item,
    label: String,
    glyph: LucideGlyph,
    modifier: Modifier = Modifier,
    cornered: Boolean = true,
    onClick: () -> Unit,
) {
    val video = item.kind == MediaKind.Video
    val spoken = if (video) "Video, $label" else label
    MediaPicture(
        url = LocalPictures.current.thumb(item),
        shape = TileShape.Square,
        glyph = glyph,
        cornered = cornered,
        modifier = modifier
            .clickable(role = Role.Button, onClick = onClick)
            .clearAndSetSemantics {
                contentDescription = spoken
                role = Role.Button
                onClick(label = if (video) "Play" else "View") {
                    onClick()
                    true
                }
            },
    ) {
        if (video) {
            Box(
                Modifier
                    .align(Alignment.BottomStart)
                    .padding(6.dp)
                    .size(24.dp)
                    .background(Popcorn.colors.mediaControl, Popcorn.shapes.avatar),
                contentAlignment = Alignment.Center,
            ) {
                LucideIcon(PopcornIcons.Play, contentDescription = null, tint = Popcorn.colors.onMedia, size = 14.dp)
            }
        }
    }
}
