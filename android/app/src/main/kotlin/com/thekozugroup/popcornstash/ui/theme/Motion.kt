package com.thekozugroup.popcornstash.ui.theme

import android.content.Context
import android.provider.Settings
import androidx.compose.animation.core.FiniteAnimationSpec
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect

/** Motion tokens (android/DESIGN.md, Motion). Nothing loops; every move has a reason. */
object PopcornMotion {
    /** Stiffness of the page spring: about 300 ms to settle, no bounce. */
    const val PAGE_STIFFNESS = 500f

    /** The toolbar's selection square: a slight settle, never a wobble. */
    const val INDICATOR_DAMPING = 0.85f
    const val INDICATOR_STIFFNESS = 500f

    /** Crossfades and short fades. */
    const val FAST_MS = 150

    /** The toolbar leaving for the player and the photo viewer. */
    const val TOOLBAR_SHOW_HIDE_MS = 200

    /** How far a pushed page travels as it fades in or out. */
    val pushShift = 30.dp

    /** Tab slides and pushes. */
    fun <T> pageSpring(): FiniteAnimationSpec<T> =
        spring(dampingRatio = Spring.DampingRatioNoBouncy, stiffness = PAGE_STIFFNESS)

    /** The selection square sliding along the toolbar. */
    fun <T> indicatorSpring(): FiniteAnimationSpec<T> =
        spring(dampingRatio = INDICATOR_DAMPING, stiffness = INDICATOR_STIFFNESS)

    fun <T> fast(): FiniteAnimationSpec<T> = tween(FAST_MS)
}

/**
 * True when the system's "Remove animations" setting is on (animator duration scale 0).
 * Tabs and pushes then crossfade, and the selection square moves without travelling.
 */
val LocalReduceMotion = staticCompositionLocalOf { false }

/** Reads the system animator scale, again each time the app comes back to the foreground. */
@Composable
fun rememberSystemReduceMotion(): Boolean {
    val context = LocalContext.current
    var reduce by remember { mutableStateOf(systemAnimationsOff(context)) }
    LifecycleEventEffect(Lifecycle.Event.ON_RESUME) { reduce = systemAnimationsOff(context) }
    return reduce
}

fun systemAnimationsOff(context: Context): Boolean =
    Settings.Global.getFloat(context.contentResolver, Settings.Global.ANIMATOR_DURATION_SCALE, 1f) == 0f
