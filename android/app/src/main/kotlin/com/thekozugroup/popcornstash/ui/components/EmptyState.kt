package com.thekozugroup.popcornstash.ui.components

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import com.thekozugroup.popcornstash.ui.icons.LucideGlyph
import com.thekozugroup.popcornstash.ui.icons.LucideIcon
import com.thekozugroup.popcornstash.ui.theme.IconStroke
import com.thekozugroup.popcornstash.ui.theme.Popcorn

/**
 * An empty or waiting page (android/DESIGN.md, Empty states): the place's glyph at 28dp,
 * a title, one or two sentences on how the space fills, and at most one action.
 */
@Composable
fun EmptyState(
    glyph: LucideGlyph,
    title: String,
    body: String,
    modifier: Modifier = Modifier,
    action: (@Composable () -> Unit)? = null,
) {
    val colors = Popcorn.colors
    Column(
        modifier
            .fillMaxWidth()
            .padding(horizontal = Popcorn.spacing.gutter),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        LucideIcon(glyph, contentDescription = null, tint = colors.textMuted, size = 28.dp, strokeWidth = IconStroke.Large)
        Spacer(Modifier.height(Popcorn.spacing.m))
        Text(title, style = Popcorn.type.section, color = colors.text, textAlign = TextAlign.Center)
        Spacer(Modifier.height(Popcorn.spacing.xs))
        Text(
            body,
            style = Popcorn.type.body,
            color = colors.textMuted,
            textAlign = TextAlign.Center,
            modifier = Modifier.widthIn(max = 360.dp),
        )
        if (action != null) {
            Spacer(Modifier.height(Popcorn.spacing.l))
            action()
        }
    }
}
