package com.thekozugroup.popcornstash.navigation

import androidx.compose.animation.ContentTransform
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.togetherWith
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.LayoutDirection
import com.thekozugroup.popcornstash.ui.theme.PopcornMotion

/**
 * Which way a tab change moves on the screen: the new page comes in from the side of the
 * tapped square. +1 when that square lies to the right of the current one, -1 to the left,
 * 0 for the same tab. In a right-to-left layout the toolbar is mirrored (Settings on the
 * left), so the direction is too.
 */
fun slideDirection(from: Tab, to: Tab, layoutDirection: LayoutDirection = LayoutDirection.Ltr): Int =
    to.ordinal.compareTo(from.ordinal) * layoutDirection.sign

/** +1 left to right, -1 right to left: slide offsets are in screen pixels and do not mirror. */
private val LayoutDirection.sign: Int get() = if (this == LayoutDirection.Rtl) -1 else 1

/**
 * A tab change (android/DESIGN.md, Tab transitions): a full-width, direction-aware
 * slide on the page spring, the old page leaving the other way. Under reduce motion
 * the pages crossfade in 150 ms instead.
 */
fun tabTransition(
    from: Tab,
    to: Tab,
    reduceMotion: Boolean,
    layoutDirection: LayoutDirection = LayoutDirection.Ltr,
): ContentTransform {
    if (reduceMotion) return crossfade()
    val direction = slideDirection(from, to, layoutDirection)
    return slideInHorizontally(PopcornMotion.pageSpring()) { width -> width * direction } togetherWith
        slideOutHorizontally(PopcornMotion.pageSpring()) { width -> -width * direction }
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
    val sign = (if (forward) 1 else -1) * layoutDirection.sign
    return (slideInHorizontally(PopcornMotion.pageSpring()) { shift * sign } + fadeIn(PopcornMotion.pageSpring())) togetherWith
        (slideOutHorizontally(PopcornMotion.pageSpring()) { -shift * sign } + fadeOut(PopcornMotion.pageSpring()))
}

private fun crossfade(): ContentTransform =
    fadeIn(PopcornMotion.fast()) togetherWith fadeOut(PopcornMotion.fast())
