package com.thekozugroup.popcornstash.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
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
