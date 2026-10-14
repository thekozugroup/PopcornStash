package com.thekozugroup.popcornstash.feature.downloads

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.thekozugroup.popcornstash.data.api.MediaKind
import com.thekozugroup.popcornstash.ui.icons.LucideGlyph
import com.thekozugroup.popcornstash.ui.icons.LucideGlyphs
import com.thekozugroup.popcornstash.ui.icons.LucideIcon
import com.thekozugroup.popcornstash.ui.media.LocalPictures
import com.thekozugroup.popcornstash.ui.media.StashImage
import com.thekozugroup.popcornstash.ui.theme.Popcorn

/** The glyph that stands for a kind of file when it has no picture. */
fun glyphForKind(kind: MediaKind): LucideGlyph = when (kind) {
    MediaKind.Video -> LucideGlyphs.Film
    MediaKind.Photo -> LucideGlyphs.Image
    MediaKind.Audio -> LucideGlyphs.Music
    else -> LucideGlyphs.FileText
}

/**
 * A square media tile (android/DESIGN.md, Tiles): the server's `sm` thumbnail at [path]
 * cropped to fill, or the file's glyph on `subtle`, with media corners and the hairline
 * inside the edge. The picture comes through the session's image loader (Home's), so the
 * token goes only to the server and the picture leaves with the session.
 */
@Composable
fun MediaThumb(path: String?, glyph: LucideGlyph, size: Dp, modifier: Modifier = Modifier) {
    val shape = Popcorn.shapes.media
    Box(
        modifier
            .size(size)
            .clip(shape)
            .background(Popcorn.colors.subtle)
            .clearAndSetSemantics { },
        contentAlignment = Alignment.Center,
    ) {
        StashImage(
            url = LocalPictures.current.url(path),
            contentDescription = null,
            modifier = Modifier.fillMaxSize(),
            placeholder = {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    LucideIcon(glyph, contentDescription = null, tint = Popcorn.colors.textMuted, size = Popcorn.media.placeholderIcon)
                }
            },
        )
        // The hairline sits over the picture, so a dark frame keeps its edge on a dark page.
        Box(Modifier.matchParentSize().border(1.dp, Popcorn.colors.mediaHairline, shape))
    }
}
