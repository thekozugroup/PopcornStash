package com.thekozugroup.popcornstash.ui.icons

import androidx.compose.foundation.layout.size
import androidx.compose.material3.Icon
import androidx.compose.material3.LocalContentColor
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.addPathNodes
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.thekozugroup.popcornstash.ui.theme.IconStroke

/** One Lucide icon: its kebab-case name and its SVG path data on the 24 x 24 grid. */
@Immutable
data class LucideGlyph(val name: String, val paths: List<String>)

private val vectorCache = HashMap<Triple<String, Float, Boolean>, ImageVector>()

/**
 * The glyph as a stroked vector, with round caps and joins like Lucide's SVGs. [filled] also
 * fills its closed shapes (a heart that is on), as the web does with `fill: currentColor`.
 */
fun LucideGlyph.toImageVector(strokeWidth: Float = IconStroke.Regular, filled: Boolean = false): ImageVector =
    synchronized(vectorCache) {
        vectorCache.getOrPut(Triple(name, strokeWidth, filled)) {
            ImageVector.Builder(
                name = "lucide.$name",
                defaultWidth = 24.dp,
                defaultHeight = 24.dp,
                viewportWidth = 24f,
                viewportHeight = 24f,
            ).apply {
                for (data in paths) {
                    addPath(
                        pathData = addPathNodes(data),
                        fill = if (filled) SolidColor(Color.Black) else null,
                        stroke = SolidColor(Color.Black),
                        strokeLineWidth = strokeWidth,
                        strokeLineCap = StrokeCap.Round,
                        strokeLineJoin = StrokeJoin.Round,
                    )
                }
            }.build()
        }
    }

/**
 * Draws a Lucide glyph at [size] in [tint]. Pass a [contentDescription] only when the
 * icon is the only label of what it stands for; icons beside words are decorative.
 */
@Composable
fun LucideIcon(
    glyph: LucideGlyph,
    contentDescription: String?,
    modifier: Modifier = Modifier,
    tint: Color = LocalContentColor.current,
    size: Dp = 24.dp,
    strokeWidth: Float = IconStroke.Regular,
    filled: Boolean = false,
) {
    val vector = remember(glyph, strokeWidth, filled) { glyph.toImageVector(strokeWidth, filled) }
    Icon(
        imageVector = vector,
        contentDescription = contentDescription,
        modifier = modifier.size(size),
        tint = tint,
    )
}
