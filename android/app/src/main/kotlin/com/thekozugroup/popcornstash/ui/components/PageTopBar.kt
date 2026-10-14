package com.thekozugroup.popcornstash.ui.components

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.IconButton
import androidx.compose.material3.PlainTooltip
import androidx.compose.material3.Text
import androidx.compose.material3.TooltipAnchorPosition
import androidx.compose.material3.TooltipBox
import androidx.compose.material3.TooltipDefaults
import androidx.compose.material3.rememberTooltipState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.thekozugroup.popcornstash.ui.icons.LucideGlyph
import com.thekozugroup.popcornstash.ui.icons.LucideIcon
import com.thekozugroup.popcornstash.ui.icons.PopcornIcons
import com.thekozugroup.popcornstash.ui.theme.Popcorn
import com.thekozugroup.popcornstash.ui.theme.PopcornMotion

/**
 * The top of a page (android/DESIGN.md, Top app bar). Part of the page, so it slides
 * with it. A tab's first page shows its title in the headline style; a pushed page
 * ([onBack] set) shows a back button and the title in the title style. A 1dp rule
 * appears under it once the page scrolls under it ([scrolled]). A page that starts with
 * its own large heading (an item, a show) shows the bar's title only once that heading
 * has scrolled away ([titleVisible]).
 */
@Composable
fun PageTopBar(
    title: String,
    modifier: Modifier = Modifier,
    onBack: (() -> Unit)? = null,
    scrolled: Boolean = false,
    titleVisible: Boolean = true,
    actions: @Composable RowScope.() -> Unit = {},
) {
    val colors = Popcorn.colors
    val titleAlpha by animateFloatAsState(
        targetValue = if (titleVisible) 1f else 0f,
        animationSpec = PopcornMotion.fast(),
        label = "top bar title",
    )
    val rule by animateColorAsState(
        targetValue = if (scrolled) colors.divider else Color.Transparent,
        animationSpec = PopcornMotion.fast(),
        label = "top bar rule",
    )
    Box(modifier.fillMaxWidth().windowInsetsPadding(WindowInsets.statusBars)) {
        Row(
            Modifier
                .fillMaxWidth()
                .height(64.dp)
                .padding(
                    start = if (onBack != null) Popcorn.spacing.xxs else Popcorn.spacing.gutter,
                    end = Popcorn.spacing.xxs,
                ),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.Start,
        ) {
            if (onBack != null) {
                TopBarAction(PopcornIcons.Back, "Back", onBack)
                Box(Modifier.width(Popcorn.spacing.xxs))
            }
            Text(
                text = title,
                style = if (onBack != null) Popcorn.type.title else Popcorn.type.headline,
                color = colors.text,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier
                    .weight(1f)
                    .graphicsLayer { alpha = titleAlpha }
                    .then(if (titleVisible) Modifier.semantics { heading() } else Modifier.clearAndSetSemantics { }),
            )
            actions()
        }
        HorizontalDivider(
            color = rule,
            thickness = 1.dp,
            modifier = Modifier.align(Alignment.BottomCenter),
        )
    }
}

/** A 48dp icon button for the top bar, with its name as the content description and a tooltip. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TopBarAction(glyph: LucideGlyph, label: String, onClick: () -> Unit) {
    TooltipBox(
        positionProvider = TooltipDefaults.rememberTooltipPositionProvider(TooltipAnchorPosition.Below),
        tooltip = { PlainTooltip { Text(label) } },
        state = rememberTooltipState(),
    ) {
        IconButton(onClick = onClick) {
            LucideIcon(glyph, contentDescription = label, tint = Popcorn.colors.text)
        }
    }
}
