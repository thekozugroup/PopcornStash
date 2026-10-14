package com.thekozugroup.popcornstash.ui.theme

import androidx.compose.material3.ColorScheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Immutable
import androidx.compose.ui.graphics.Color

/**
 * The Popcorn palette: Tailwind v4 zinc, blue and status shades, converted from the
 * web theme's oklch values (web/src/themes/popcorn/popcornTheme.ts) to sRGB.
 */
object PopcornPalette {
    val White = Color(0xFFFFFFFF)
    val Black = Color(0xFF000000)

    val Zinc50 = Color(0xFFFAFAFA)
    val Zinc100 = Color(0xFFF4F4F5)
    val Zinc200 = Color(0xFFE4E4E7)
    val Zinc300 = Color(0xFFD4D4D8)
    val Zinc400 = Color(0xFF9F9FA9)
    val Zinc500 = Color(0xFF71717B)
    val Zinc600 = Color(0xFF52525C)
    val Zinc700 = Color(0xFF3F3F46)
    val Zinc800 = Color(0xFF27272A)
    val Zinc900 = Color(0xFF18181B)
    val Zinc950 = Color(0xFF09090B)

    val Blue400 = Color(0xFF51A2FF)
    val Blue500 = Color(0xFF2B7FFF)
    val Blue600 = Color(0xFF155DFC)

    val Emerald400 = Color(0xFF00D492)
    val Emerald500 = Color(0xFF00BC7D)
    val Emerald700 = Color(0xFF007A55)
    val Amber400 = Color(0xFFFFB900)
    val Amber500 = Color(0xFFFE9A00)
    val Amber700 = Color(0xFFBB4D00)
    val Red400 = Color(0xFFFF6467)
    val Red500 = Color(0xFFFB2C36)
    val Red700 = Color(0xFFC10007)
}

/** Semantic colours. Names follow android/DESIGN.md; screens use these, never the palette. */
@Immutable
data class PopcornColors(
    val page: Color,
    val surface: Color,
    val subtle: Color,
    val divider: Color,
    val outline: Color,
    /** The edge of a text field: zinc-500, 3:1 or more against the page in light and dark. */
    val fieldOutline: Color,
    val text: Color,
    val textMuted: Color,
    /** Action Blue: blue-600 in both themes, the one blue for fills, focus and progress. */
    val accent: Color,
    /** Words, icons and press feedback on Action Blue: white, 5.25:1 on blue-600. */
    val onAccent: Color,
    val link: Color,
    /** The track under a download's progress: 3:1 or more against Action Blue in both themes. */
    val progressTrack: Color,
    val mediaHairline: Color,
    /** The track under the watched bar on a picture: dark enough that Action Blue keeps 3:1 over any picture. */
    val mediaProgressTrack: Color,
    val stage: Color,
    val mediaControl: Color,
    val onMedia: Color,
    val badge: Color,
    val onBadge: Color,
    val toolbarShadow: Color,
    /** The floating toolbar's glass: `surface` at the alpha that keeps `textMuted` icons at 3:1 over any picture. */
    val glass: Color,
    /** The 1dp edge of the glass toolbar. */
    val glassEdge: Color,
    val connected: Color,
    val connectedDot: Color,
    val warning: Color,
    val warningDot: Color,
    val error: Color,
    val errorDot: Color,
    val isDark: Boolean,
)

val LightPopcornColors = PopcornColors(
    page = PopcornPalette.Zinc50,
    surface = PopcornPalette.White,
    subtle = PopcornPalette.Zinc100,
    divider = PopcornPalette.Zinc200,
    outline = PopcornPalette.Zinc300,
    fieldOutline = PopcornPalette.Zinc500,
    text = PopcornPalette.Zinc900,
    textMuted = PopcornPalette.Zinc600,
    accent = PopcornPalette.Blue600,
    onAccent = PopcornPalette.White,
    link = PopcornPalette.Blue600,
    progressTrack = PopcornPalette.Zinc100,
    mediaHairline = PopcornPalette.Zinc950.copy(alpha = 0.08f),
    mediaProgressTrack = PopcornPalette.Zinc950.copy(alpha = 0.9f),
    stage = PopcornPalette.Black,
    mediaControl = PopcornPalette.Zinc950.copy(alpha = 0.62f),
    onMedia = PopcornPalette.Zinc50,
    badge = PopcornPalette.Zinc900,
    onBadge = PopcornPalette.Zinc50,
    toolbarShadow = PopcornPalette.Zinc950.copy(alpha = 0.18f),
    glass = PopcornPalette.White.copy(alpha = 0.8f),
    glassEdge = PopcornPalette.Zinc950.copy(alpha = 0.1f),
    connected = PopcornPalette.Emerald700,
    connectedDot = PopcornPalette.Emerald500,
    warning = PopcornPalette.Amber700,
    warningDot = PopcornPalette.Amber500,
    error = PopcornPalette.Red700,
    errorDot = PopcornPalette.Red500,
    isDark = false,
)

val DarkPopcornColors = PopcornColors(
    page = PopcornPalette.Zinc950,
    surface = PopcornPalette.Zinc900,
    subtle = PopcornPalette.Zinc800,
    divider = PopcornPalette.Zinc800,
    outline = PopcornPalette.Zinc700,
    fieldOutline = PopcornPalette.Zinc500,
    text = PopcornPalette.Zinc50,
    textMuted = PopcornPalette.Zinc400,
    accent = PopcornPalette.Blue600,
    onAccent = PopcornPalette.White,
    link = PopcornPalette.Blue400,
    // zinc-800 would leave Action Blue at 2.84:1; zinc-900 gives 3.38:1.
    progressTrack = PopcornPalette.Zinc900,
    mediaHairline = PopcornPalette.White.copy(alpha = 0.08f),
    mediaProgressTrack = PopcornPalette.Zinc950.copy(alpha = 0.9f),
    stage = PopcornPalette.Black,
    mediaControl = PopcornPalette.Zinc950.copy(alpha = 0.62f),
    onMedia = PopcornPalette.Zinc50,
    badge = PopcornPalette.Zinc50,
    onBadge = PopcornPalette.Zinc900,
    toolbarShadow = PopcornPalette.Black.copy(alpha = 0.6f),
    glass = PopcornPalette.Zinc900.copy(alpha = 0.8f),
    glassEdge = PopcornPalette.White.copy(alpha = 0.1f),
    connected = PopcornPalette.Emerald400,
    connectedDot = PopcornPalette.Emerald500,
    warning = PopcornPalette.Amber400,
    warningDot = PopcornPalette.Amber500,
    error = PopcornPalette.Red400,
    errorDot = PopcornPalette.Red500,
    isDark = true,
)

/** Maps the Popcorn colours onto Material roles so stock components match the app. */
fun PopcornColors.toColorScheme(): ColorScheme {
    val base = if (isDark) darkColorScheme() else lightColorScheme()
    return base.copy(
        primary = accent,
        onPrimary = onAccent,
        primaryContainer = accent,
        onPrimaryContainer = onAccent,
        // Blue words on the inverse surface (a snackbar's action) take the link blue for that surface:
        // blue-400 on light's zinc-900 (6.72:1; blue-600 would be 3.38:1), blue-600 on dark's zinc-50 (5.03:1).
        inversePrimary = if (isDark) PopcornPalette.Blue600 else PopcornPalette.Blue400,
        secondary = text,
        onSecondary = page,
        secondaryContainer = subtle,
        onSecondaryContainer = text,
        tertiary = text,
        onTertiary = page,
        tertiaryContainer = subtle,
        onTertiaryContainer = text,
        background = page,
        onBackground = text,
        surface = page,
        onSurface = text,
        surfaceVariant = subtle,
        onSurfaceVariant = textMuted,
        surfaceTint = page,
        inverseSurface = if (isDark) PopcornPalette.Zinc50 else PopcornPalette.Zinc900,
        inverseOnSurface = if (isDark) PopcornPalette.Zinc900 else PopcornPalette.Zinc50,
        error = error,
        onError = if (isDark) PopcornPalette.Zinc950 else PopcornPalette.White,
        errorContainer = error,
        onErrorContainer = if (isDark) PopcornPalette.Zinc950 else PopcornPalette.White,
        outline = outline,
        outlineVariant = divider,
        scrim = PopcornPalette.Zinc950,
        surfaceBright = surface,
        surfaceDim = page,
        surfaceContainer = surface,
        surfaceContainerHigh = surface,
        surfaceContainerHighest = surface,
        surfaceContainerLow = surface,
        surfaceContainerLowest = surface,
    )
}
