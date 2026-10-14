package com.thekozugroup.popcornstash.navigation

import androidx.compose.animation.ContentTransform
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.togetherWith
import androidx.compose.ui.unit.Density
import com.thekozugroup.popcornstash.ui.theme.PopcornMotion

/**
 * Which way a tab change moves: the new page comes in from the side of the tapped
 * square. +1 when it lies to the right of the current tab, -1 to the left, 0 for the same tab.
 */
fun slideDirection(from: Tab, to: Tab): Int = to.ordinal.compareTo(from.ordinal)

/**
 * A tab change (android/DESIGN.md, Tab transitions): a full-width, direction-aware
 * slide on the page spring, the old page leaving the other way. Under reduce motion
 * the pages crossfade in 150 ms instead.
 */
fun tabTransition(from: Tab, to: Tab, reduceMotion: Boolean): ContentTransform {
    if (reduceMotion) return crossfade()
    val direction = slideDirection(from, to)
    return slideInHorizontally(PopcornMotion.pageSpring()) { width -> width * direction } togetherWith
        slideOutHorizontally(PopcornMotion.pageSpring()) { width -> -width * direction }
}

/**
 * A page opening ([forward]) or closing inside a tab, or a step in sign-in: a short
 * shift with a fade on the page spring. Under reduce motion a crossfade.
 */
fun pushTransition(forward: Boolean, density: Density, reduceMotion: Boolean): ContentTransform {
    if (reduceMotion) return crossfade()
    val shift = with(density) { PopcornMotion.pushShift.roundToPx() }
    val sign = if (forward) 1 else -1
    return (slideInHorizontally(PopcornMotion.pageSpring()) { shift * sign } + fadeIn(PopcornMotion.pageSpring())) togetherWith
        (slideOutHorizontally(PopcornMotion.pageSpring()) { -shift * sign } + fadeOut(PopcornMotion.pageSpring()))
}

private fun crossfade(): ContentTransform =
    fadeIn(PopcornMotion.fast()) togetherWith fadeOut(PopcornMotion.fast())
