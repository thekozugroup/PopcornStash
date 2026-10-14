package com.thekozugroup.popcornstash.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.PlainTooltip
import androidx.compose.material3.Text
import androidx.compose.material3.TooltipAnchorPosition
import androidx.compose.material3.TooltipBox
import androidx.compose.material3.TooltipDefaults
import androidx.compose.material3.rememberTooltipState
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.thekozugroup.popcornstash.ui.icons.LucideGlyph
import com.thekozugroup.popcornstash.ui.icons.LucideIcon
import com.thekozugroup.popcornstash.ui.theme.LocalPlayerTokens
import com.thekozugroup.popcornstash.ui.theme.Popcorn

/**
 * A control laid over a picture (android/DESIGN.md, Player and photo viewer): the glyph in
 * `onMedia` on a `mediaControl` disc, its [label] as the content description and as a
 * tooltip on long press. At least 48dp, the ripple clipped to the disc.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MediaDisc(
    glyph: LucideGlyph,
    label: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    size: Dp = LocalPlayerTokens.current.disc,
    iconSize: Dp = 24.dp,
) {
    val colors = Popcorn.colors
    TooltipBox(
        positionProvider = TooltipDefaults.rememberTooltipPositionProvider(TooltipAnchorPosition.Below),
        tooltip = { PlainTooltip { Text(label) } },
        state = rememberTooltipState(),
    ) {
        Box(
            modifier
                .size(size)
                .clip(CircleShape)
                .background(colors.mediaControl)
                .clickable(role = Role.Button, onClick = onClick)
                .semantics { contentDescription = label },
            contentAlignment = Alignment.Center,
        ) {
            LucideIcon(glyph, contentDescription = null, tint = colors.onMedia, size = iconSize)
        }
    }
}

/** Words or a seek bar laid over a picture: a `mediaControl` plate with the panel's 16dp corners, as tall as a disc. */
@Composable
fun MediaPlate(modifier: Modifier = Modifier, content: @Composable RowScope.() -> Unit) {
    val tokens = LocalPlayerTokens.current
    Row(
        modifier
            .clip(Popcorn.shapes.panel)
            .background(Popcorn.colors.mediaControl)
            .height(tokens.disc)
            .padding(horizontal = tokens.plateHorizontalPadding),
        verticalAlignment = Alignment.CenterVertically,
        content = content,
    )
}
