package com.thekozugroup.popcornstash.ui.components

import androidx.compose.material3.SegmentedButtonColors
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import com.thekozugroup.popcornstash.ui.icons.LucideIcon
import com.thekozugroup.popcornstash.ui.icons.PopcornIcons
import com.thekozugroup.popcornstash.ui.theme.Popcorn

// Segmented choices (seasons, "A movie" or "A camera video") in the app's neutral choice style
// (android/DESIGN.md, Choices and confirmations). Which segment is chosen never rests on a tint
// alone: the chosen one carries a check and a `text` edge, both far above 3:1 against the page,
// and every segment's edge is `fieldOutline` (3:1 or more in both themes).

/** The colours of a segmented choice on [background] (`page` on a page, `surface` in a sheet). */
@Composable
fun choiceSegmentColors(background: Color): SegmentedButtonColors {
    val colors = Popcorn.colors
    return SegmentedButtonDefaults.colors(
        activeContainerColor = colors.subtle,
        activeContentColor = colors.text,
        activeBorderColor = colors.text,
        inactiveContainerColor = background,
        inactiveContentColor = colors.textMuted,
        inactiveBorderColor = colors.fieldOutline,
    )
}

/** The check before the chosen segment's words; nothing before the others. */
@Composable
fun ChoiceSegmentMark(selected: Boolean) {
    SegmentedButtonDefaults.Icon(active = selected, activeContent = {
        LucideIcon(PopcornIcons.Chosen, contentDescription = null, size = 18.dp)
    })
}
