package com.thekozugroup.popcornstash.navigation

import androidx.compose.animation.ContentTransform
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.slideOutVertically
import androidx.compose.animation.togetherWith
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.LayoutDirection
import com.thekozugroup.popcornstash.ui.theme.PopcornMotion

/**
 * The tapped square's position in the toolbar's order: +1 after, -1 before, 0 unchanged.
 */
fun slideDirection(from: Tab, to: Tab): Int = to.ordinal.compareTo(from.ordinal)

/**
 * A tab change follows the selection along the toolbar's axis. Bottom navigation mirrors
 * horizontally in RTL; the physical left rail keeps its vertical order. Under reduce
 * motion the pages crossfade in 150 ms instead.
 */
fun tabTransition(
    from: Tab,
    to: Tab,
    reduceMotion: Boolean,
    vertical: Boolean = true,
    layoutDirection: LayoutDirection = LayoutDirection.Ltr,
): ContentTransform {
    if (reduceMotion) return crossfade()
    val direction = slideDirection(from, to) * (if (!vertical && layoutDirection == LayoutDirection.Rtl) -1 else 1)
    if (!vertical) return slideInHorizontally(PopcornMotion.pageSpring()) { width -> width * direction } togetherWith
        slideOutHorizontally(PopcornMotion.pageSpring()) { width -> -width * direction }
    return slideInVertically(PopcornMotion.pageSpring()) { height -> height * direction } togetherWith
        slideOutVertically(PopcornMotion.pageSpring()) { height -> -height * direction }
}

/**
 * A page opening ([forward]) or closing inside a tab, or a step in sign-in: a short
 * shift with a fade on the page spring, from the end side (the right, or the left in a
 * right-to-left layout). Under reduce motion a crossfade.
 */
fun pushTransition(
    forward: Boolean,
    density: Density,
    reduceMotion: Boolean,
    layoutDirection: LayoutDirection = LayoutDirection.Ltr,
): ContentTransform {
    if (reduceMotion) return crossfade()
    val shift = with(density) { PopcornMotion.pushShift.roundToPx() }
    // Slide offsets are in screen pixels and do not mirror: -1 brings the page from the left.
    val sign = (if (forward) 1 else -1) * (if (layoutDirection == LayoutDirection.Rtl) -1 else 1)
    return (slideInHorizontally(PopcornMotion.pageSpring()) { shift * sign } + fadeIn(PopcornMotion.pageSpring())) togetherWith
        (slideOutHorizontally(PopcornMotion.pageSpring()) { -shift * sign } + fadeOut(PopcornMotion.pageSpring()))
}

private fun crossfade(): ContentTransform =
    fadeIn(PopcornMotion.fast()) togetherWith fadeOut(PopcornMotion.fast())
