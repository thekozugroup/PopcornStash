package com.thekozugroup.popcornstash.feature.player

import android.content.Context
import androidx.compose.foundation.Image
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
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
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
import androidx.lifecycle.compose.LifecycleResumeEffect
import coil3.compose.asPainter
import coil3.network.HttpException
import coil3.request.CachePolicy
import coil3.request.ErrorResult
import coil3.request.ImageRequest
import coil3.request.ImageResult
import coil3.request.SuccessResult
import com.thekozugroup.popcornstash.ui.media.LocalPictures
import com.thekozugroup.popcornstash.ui.media.LocalStashImages
import com.thekozugroup.popcornstash.ui.media.Pictures
import com.thekozugroup.popcornstash.ui.theme.LocalPlayerTokens
import com.thekozugroup.popcornstash.ui.theme.Popcorn
import kotlinx.coroutines.delay
import kotlin.math.ceil
import kotlin.math.roundToInt

internal fun previewSecond(positionMs: Long, durationMs: Long): Long {
    val interval = ceil(durationMs.coerceAtLeast(1) / 200_000.0).toLong().coerceAtLeast(2)
    val position = positionMs.coerceIn(0, (durationMs - 100).coerceAtLeast(0)) / 1000
    return position / interval * interval
}

internal class PreviewPosition(private val gap: Int, private val fraction: Float) : PopupPositionProvider {
    override fun calculatePosition(anchorBounds: IntRect, windowSize: IntSize, layoutDirection: LayoutDirection, popupContentSize: IntSize): IntOffset {
        val fromLeft = if (layoutDirection == LayoutDirection.Rtl) 1f - fraction else fraction
        val x = anchorBounds.left + (anchorBounds.width * fromLeft).roundToInt()
        return IntOffset(
            (x - popupContentSize.width / 2).coerceIn(0, (windowSize.width - popupContentSize.width).coerceAtLeast(0)),
            (anchorBounds.top - popupContentSize.height - gap).coerceIn(0, (windowSize.height - popupContentSize.height).coerceAtLeast(0)),
        )
    }
}

internal fun previewRequest(context: Context, pictures: Pictures, itemId: String, second: Long): ImageRequest =
    ImageRequest.Builder(context).data(pictures.preview(itemId, second))
        .diskCachePolicy(CachePolicy.DISABLED).memoryCachePolicy(CachePolicy.DISABLED).build()

private val ImageResult.httpStatus: Int?
    get() = ((this as? ErrorResult)?.throwable as? HttpException)?.response?.code

private data class PreviewFrame(val second: Long, val image: coil3.Image)

/** Ten decoded frames for a single item and foreground visit. Accessed on the UI thread. */
internal class SeekPreviewFrames(private val fetch: suspend (Long) -> ImageResult) {
    private var recent by mutableStateOf<List<PreviewFrame>>(emptyList())
    private var closed = false

    fun imageAt(second: Long): coil3.Image? = recent.find { it.second == second }?.image

    suspend fun load(second: Long, stillSelected: () -> Boolean = { true }) {
        if (closed) return
        val hit = recent.find { it.second == second }
        if (hit != null) {
            if (hit !== recent.last()) recent = recent - hit + hit
            return
        }
        var result = fetch(second)
        if (!closed && result.httpStatus == 409 && stillSelected()) {
            delay(300)
            if (closed || !stillSelected()) return
            result = fetch(second)
        }
        if (closed) return
        if (result is SuccessResult) {
            recent = (recent + PreviewFrame(second, result.image)).takeLast(10)
        } else {
            when (result.httpStatus) {
                401, 403, 404 -> recent = emptyList()
            }
        }
    }

    fun close() {
        closed = true
        recent = emptyList()
    }
}

@Composable
internal fun rememberSeekPreviewFrames(itemId: String?): SeekPreviewFrames? {
    val pictures = LocalPictures.current
    val loader = LocalStashImages.current
    val context = LocalContext.current
    if (itemId == null || loader == null) return null
    return key(itemId, pictures, loader, context) {
        var current by remember { mutableStateOf<SeekPreviewFrames?>(null) }
        LifecycleResumeEffect(Unit) {
            val frames = SeekPreviewFrames { second ->
                loader.execute(previewRequest(context, pictures, itemId, second))
            }
            current = frames
            onPauseOrDispose {
                frames.close()
                current = null
            }
        }
        current
    }
}

/** The secure popup uses the slider track as its anchor, including a stationary press before a drag. */
@Composable
internal fun SeekPreview(frames: SeekPreviewFrames?, fraction: Float, durationMs: Long) {
    val positionMs = (fraction * durationMs).toLong()
    val second = previewSecond(positionMs, durationMs)
    val target by rememberUpdatedState(second)
    LaunchedEffect(frames) {
        if (frames != null) {
            // Let a slow extraction finish, then load only the latest sample crossed by the finger.
            snapshotFlow { target }.collect { sample -> frames.load(sample) { target == sample } }
        }
    }
    val tokens = LocalPlayerTokens.current
    val gap = with(LocalDensity.current) { (12.dp + (tokens.seekThumb - tokens.seekTrack) / 2).roundToPx() }
    Popup(
        popupPositionProvider = remember(gap, fraction) { PreviewPosition(gap, fraction) },
        properties = PopupProperties(focusable = false, securePolicy = SecureFlagPolicy.Inherit),
    ) {
        SeekPreviewContent(frames?.imageAt(second), positionMs)
    }
}

@Composable
internal fun SeekPreviewContent(image: coil3.Image?, positionMs: Long) {
    val colors = Popcorn.colors
    val context = LocalContext.current
    Column(
        Modifier.width(160.dp).clip(Popcorn.shapes.media).background(colors.stage)
            .testTag("seek preview").clearAndSetSemantics {},
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        val frameArea = Modifier.fillMaxWidth().height(90.dp)
        if (image == null) {
            Box(frameArea.background(colors.mediaControl))
        } else {
            Image(remember(image, context) { image.asPainter(context) }, contentDescription = null,
                modifier = frameArea, contentScale = ContentScale.Fit)
        }
        Text(PlayerFormat.clock(positionMs / 1000.0), style = Popcorn.type.metadata,
            color = colors.onMedia, modifier = Modifier.padding(8.dp))
    }
}
