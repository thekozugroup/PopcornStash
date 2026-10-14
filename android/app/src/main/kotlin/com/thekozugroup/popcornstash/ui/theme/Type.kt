package com.thekozugroup.popcornstash.ui.theme

import androidx.compose.material3.Typography
import androidx.compose.runtime.Immutable
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp

/**
 * The Popcorn type ramp (android/DESIGN.md, Type). One family, the device's system
 * sans; hierarchy comes from size and weight. Sizes are sp so they follow the
 * system font size.
 */
@Immutable
data class PopcornType(
    val headline: TextStyle,
    val title: TextStyle,
    val section: TextStyle,
    val body: TextStyle,
    val item: TextStyle,
    val action: TextStyle,
    val metadata: TextStyle,
    val tag: TextStyle,
)

private val SystemSans = FontFamily.Default

val DefaultPopcornType = PopcornType(
    headline = TextStyle(
        fontFamily = SystemSans,
        fontWeight = FontWeight.SemiBold,
        fontSize = 28.sp,
        lineHeight = 34.sp,
        letterSpacing = (-0.025).em,
    ),
    title = TextStyle(
        fontFamily = SystemSans,
        fontWeight = FontWeight.SemiBold,
        fontSize = 20.sp,
        lineHeight = 28.sp,
        letterSpacing = (-0.025).em,
    ),
    section = TextStyle(
        fontFamily = SystemSans,
        fontWeight = FontWeight.SemiBold,
        fontSize = 16.sp,
        lineHeight = 24.sp,
    ),
    body = TextStyle(
        fontFamily = SystemSans,
        fontWeight = FontWeight.Normal,
        fontSize = 16.sp,
        lineHeight = 24.sp,
    ),
    item = TextStyle(
        fontFamily = SystemSans,
        fontWeight = FontWeight.Medium,
        fontSize = 14.sp,
        lineHeight = 20.sp,
    ),
    action = TextStyle(
        fontFamily = SystemSans,
        fontWeight = FontWeight.SemiBold,
        fontSize = 14.sp,
        lineHeight = 20.sp,
    ),
    metadata = TextStyle(
        fontFamily = SystemSans,
        fontWeight = FontWeight.Normal,
        fontSize = 13.sp,
        lineHeight = 18.sp,
        fontFeatureSettings = "tnum",
    ),
    tag = TextStyle(
        fontFamily = SystemSans,
        fontWeight = FontWeight.Medium,
        fontSize = 12.sp,
        lineHeight = 16.sp,
        fontFeatureSettings = "tnum",
    ),
)

/** Material roles carry the same styles, so stock components (sheets, tooltips, buttons) match. */
fun PopcornType.toTypography(): Typography {
    val base = Typography()
    return base.copy(
        headlineLarge = headline,
        headlineMedium = headline,
        headlineSmall = title,
        titleLarge = title,
        titleMedium = section,
        titleSmall = item,
        bodyLarge = body,
        bodyMedium = item.copy(fontWeight = FontWeight.Normal),
        bodySmall = metadata,
        labelLarge = action,
        labelMedium = metadata.copy(fontWeight = FontWeight.Medium),
        labelSmall = tag,
    )
}
