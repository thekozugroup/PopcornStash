package com.thekozugroup.popcornstash.feature.player

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntRect
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Popup
import androidx.compose.ui.window.PopupPositionProvider
import androidx.compose.ui.window.PopupProperties
import androidx.compose.ui.window.SecureFlagPolicy
import com.thekozugroup.popcornstash.ui.media.LocalPictures
import com.thekozugroup.popcornstash.ui.media.StashImage
import com.thekozugroup.popcornstash.ui.theme.Popcorn
import kotlinx.coroutines.delay
import kotlin.math.ceil

internal fun previewSecond(positionMs: Long, durationMs: Long): Long {
    val interval = ceil(durationMs.coerceAtLeast(1) / 200_000.0).toLong().coerceAtLeast(2)
    val position = positionMs.coerceIn(0, (durationMs - 100).coerceAtLeast(0)) / 1000
    return position / interval * interval
}

internal class PreviewPosition(private val gap: Int) : PopupPositionProvider {
    override fun calculatePosition(anchorBounds: IntRect, windowSize: IntSize, layoutDirection: LayoutDirection, popupContentSize: IntSize): IntOffset =
        IntOffset(
            (anchorBounds.center.x - popupContentSize.width / 2).coerceIn(0, (windowSize.width - popupContentSize.width).coerceAtLeast(0)),
            (anchorBounds.top - popupContentSize.height - gap).coerceIn(0, (windowSize.height - popupContentSize.height).coerceAtLeast(0)),
        )
}

/** A separate secure window avoids clipping the frame inside the rounded control plate. */
@Composable
internal fun SeekPreview(itemId: String, positionMs: Long, durationMs: Long) {
    val colors = Popcorn.colors
    val second = previewSecond(positionMs, durationMs)
    var loadedSecond by remember(itemId) { mutableStateOf<Long?>(null) }
    LaunchedEffect(itemId, second) {
        loadedSecond = null
        delay(180)
        loadedSecond = second
    }
    val gap = with(LocalDensity.current) { 12.dp.roundToPx() }
    Popup(
        popupPositionProvider = remember(gap) { PreviewPosition(gap) },
        properties = PopupProperties(focusable = false, securePolicy = SecureFlagPolicy.Inherit),
    ) {
        Column(
            Modifier.width(160.dp).clip(Popcorn.shapes.media).background(colors.stage)
                .testTag("seek preview").clearAndSetSemantics {},
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            StashImage(
                url = loadedSecond?.takeIf { it == second }?.let { LocalPictures.current.preview(itemId, it) },
                contentDescription = null,
                modifier = Modifier.fillMaxWidth().height(90.dp),
                contentScale = ContentScale.Fit,
                cache = false,
                placeholder = { Box(Modifier.fillMaxWidth().height(90.dp).background(colors.mediaControl)) },
            )
            Text(PlayerFormat.clock(positionMs / 1000.0), style = Popcorn.type.metadata,
                color = colors.onMedia, modifier = Modifier.padding(8.dp))
        }
    }
}
