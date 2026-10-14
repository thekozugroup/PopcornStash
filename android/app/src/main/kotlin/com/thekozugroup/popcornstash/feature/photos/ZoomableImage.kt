package com.thekozugroup.popcornstash.feature.photos

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.AnimationVector2D
import androidx.compose.animation.core.VectorConverter
import androidx.compose.animation.core.spring
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.calculateCentroid
import androidx.compose.foundation.gestures.calculatePan
import androidx.compose.foundation.gestures.calculateZoom
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.positionChanged
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.unit.IntSize
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.launch

/**
 * How far a photo is zoomed and moved. 1x shows it whole; a pinch zooms to [MAX_SCALE] around
 * the fingers, a double tap toggles [DOUBLE_TAP_SCALE] at the tapped point, and a zoomed photo
 * pans within its edges.
 */
@Stable
class ZoomState {
    var scale by mutableFloatStateOf(1f)
        internal set
    var offset by mutableStateOf(Offset.Zero)
        internal set
    internal var size = IntSize.Zero

    val zoomed: Boolean get() = scale > 1.01f

    /** Zooms by [zoom] around [centroid] (in the box, from its top start) and moves by [pan]. */
    fun transform(centroid: Offset, pan: Offset, zoom: Float) {
        val newScale = (scale * zoom).coerceIn(1f, MAX_SCALE)
        val factor = newScale / scale
        val center = Offset(size.width / 2f, size.height / 2f)
        val fromCenter = centroid - center
        // Keep the point under the fingers where it is, then follow the pan.
        val moved = fromCenter * (1 - factor) + offset * factor + pan
        scale = newScale
        offset = clamp(moved, newScale)
    }

    /** Where a double tap at [tap] leads: back to whole, or [DOUBLE_TAP_SCALE] around the tap. */
    fun doubleTapTarget(tap: Offset): Pair<Float, Offset> {
        if (zoomed) return 1f to Offset.Zero
        val center = Offset(size.width / 2f, size.height / 2f)
        return DOUBLE_TAP_SCALE to clamp((tap - center) * (1 - DOUBLE_TAP_SCALE), DOUBLE_TAP_SCALE)
    }

    fun reset() {
        scale = 1f
        offset = Offset.Zero
    }

    private fun clamp(offset: Offset, scale: Float): Offset {
        val maxX = (scale - 1) * size.width / 2f
        val maxY = (scale - 1) * size.height / 2f
        return Offset(offset.x.coerceIn(-maxX, maxX), offset.y.coerceIn(-maxY, maxY))
    }

    companion object {
        const val MAX_SCALE = 5f
        const val DOUBLE_TAP_SCALE = 2.5f
    }
}

/**
 * A picture that zooms and pans in place. While it is whole, a one-finger swipe is left to
 * the pager around it, so the person moves between photos; two fingers always zoom. A tap
 * calls [onTap] (the viewer shows or hides its controls).
 */
@Composable
fun Zoomable(
    state: ZoomState,
    reduceMotion: Boolean,
    onTap: () -> Unit,
    modifier: Modifier = Modifier,
    content: @Composable BoxScope.() -> Unit,
) {
    val scope = rememberCoroutineScope()
    val tap by rememberUpdatedState(onTap)
    Box(
        modifier
            .fillMaxSize()
            .onSizeChanged { state.size = it }
            .pointerInput(state) {
                awaitEachGesture {
                    awaitFirstDown(requireUnconsumed = false)
                    do {
                        val event = awaitPointerEvent()
                        val fingers = event.changes.count { it.pressed }
                        if (fingers >= 2 || state.zoomed) {
                            state.transform(event.calculateCentroid(useCurrent = true), event.calculatePan(), event.calculateZoom())
                            event.changes.forEach { if (it.positionChanged()) it.consume() }
                        }
                    } while (event.changes.any { it.pressed })
                    if (!state.zoomed) state.reset()
                }
            }
            .pointerInput(state, reduceMotion) {
                detectTapGestures(
                    onTap = { tap() },
                    onDoubleTap = { point ->
                        val (scale, offset) = state.doubleTapTarget(point)
                        if (reduceMotion) {
                            state.scale = scale
                            state.offset = offset
                        } else {
                            scope.launch { animateTo(state, scale, offset) }
                        }
                    },
                )
            }
            .graphicsLayer {
                scaleX = state.scale
                scaleY = state.scale
                translationX = state.offset.x
                translationY = state.offset.y
            },
        content = content,
    )
}

private suspend fun animateTo(state: ZoomState, scale: Float, offset: Offset) = coroutineScope {
    val scaleAnim = androidx.compose.animation.core.Animatable(state.scale)
    val offsetAnim: Animatable<Offset, AnimationVector2D> = Animatable(state.offset, Offset.VectorConverter)
    launch { scaleAnim.animateTo(scale, spring(stiffness = 500f)) { state.scale = value } }
    launch { offsetAnim.animateTo(offset, spring(stiffness = 500f)) { state.offset = value } }
}

/** A zoom state per photo, back to whole whenever the photo leaves the screen. */
@Composable
fun rememberZoomState(key: Any): ZoomState {
    val state = remember(key) { ZoomState() }
    LaunchedEffect(key) { state.reset() }
    return state
}
