package com.thekozugroup.popcornstash.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material.ripple.RippleAlpha
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.RippleConfiguration
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.runtime.staticCompositionLocalOf

val LocalPopcornColors = staticCompositionLocalOf { LightPopcornColors }
val LocalPopcornType = staticCompositionLocalOf { DefaultPopcornType }
val LocalPopcornShapes = staticCompositionLocalOf { DefaultPopcornShapes }
val LocalPopcornSpacing = staticCompositionLocalOf { PopcornSpacing() }
val LocalToolbarTokens = staticCompositionLocalOf { ToolbarTokens() }
val LocalMediaTokens = staticCompositionLocalOf { MediaTokens() }
val LocalCarouselTokens = staticCompositionLocalOf { CarouselTokens() }

/** The Popcorn tokens for the current theme: `Popcorn.colors.accent`, `Popcorn.type.title`, ... */
object Popcorn {
    val colors: PopcornColors
        @Composable @ReadOnlyComposable get() = LocalPopcornColors.current
    val type: PopcornType
        @Composable @ReadOnlyComposable get() = LocalPopcornType.current
    val shapes: PopcornShapes
        @Composable @ReadOnlyComposable get() = LocalPopcornShapes.current
    val spacing: PopcornSpacing
        @Composable @ReadOnlyComposable get() = LocalPopcornSpacing.current
    val toolbar: ToolbarTokens
        @Composable @ReadOnlyComposable get() = LocalToolbarTokens.current
    val media: MediaTokens
        @Composable @ReadOnlyComposable get() = LocalMediaTokens.current
    val carousel: CarouselTokens
        @Composable @ReadOnlyComposable get() = LocalCarouselTokens.current
    val reduceMotion: Boolean
        @Composable @ReadOnlyComposable get() = LocalReduceMotion.current
}

/**
 * Material 3 themed with the Popcorn tokens (android/DESIGN.md). Light and dark follow
 * the system; there is no wallpaper colour, because the library's pictures bring the
 * colour. [reduceMotion] defaults to the system's "Remove animations" setting.
 */
@Composable
fun PopcornTheme(
    darkTheme: Boolean = isSystemInDarkTheme(),
    reduceMotion: Boolean = rememberSystemReduceMotion(),
    content: @Composable () -> Unit,
) {
    val colors = if (darkTheme) DarkPopcornColors else LightPopcornColors
    MaterialTheme(
        colorScheme = colors.toColorScheme(),
        typography = DefaultPopcornType.toTypography(),
        shapes = popcornMaterialShapes(),
    ) {
        CompositionLocalProvider(
            LocalPopcornColors provides colors,
            LocalPopcornType provides DefaultPopcornType,
            LocalPopcornShapes provides DefaultPopcornShapes,
            LocalReduceMotion provides reduceMotion,
            LocalContentColor provides colors.text,
            content = content,
        )
    }
}

/**
 * Press feedback on Action Blue (the primary button, the toolbar's selected square): a white
 * ripple, 8% while pressed, focused or dragged (white words keep 4.61:1) and 4% while a mouse
 * hovers (4.92:1). Material's own 10% would leave them at 4.45:1 (android/DESIGN.md, Colour).
 * Material draws the focus or hover layer and the press together, so a press on a focused
 * button is 15.4% white (4.06:1) for as long as the key is held; lowering focus far enough to
 * avoid that would leave keyboard focus too faint to see.
 */
val OnAccentRipple = RippleConfiguration(
    color = PopcornPalette.White,
    rippleAlpha = RippleAlpha(draggedAlpha = 0.08f, focusedAlpha = 0.08f, hoveredAlpha = 0.04f, pressedAlpha = 0.08f),
)
