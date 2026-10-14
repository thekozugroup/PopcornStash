package com.thekozugroup.popcornstash.ui.components

import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.snap
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.LocalRippleConfiguration
import androidx.compose.material3.PlainTooltip
import androidx.compose.material3.Text
import androidx.compose.material3.TooltipAnchorPosition
import androidx.compose.material3.TooltipBox
import androidx.compose.material3.TooltipDefaults
import androidx.compose.material3.rememberTooltipState
import androidx.compose.material3.ripple
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.ClipOp
import androidx.compose.ui.graphics.drawOutline
import androidx.compose.ui.graphics.drawscope.clipRect
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.CollectionInfo
import androidx.compose.ui.semantics.CollectionItemInfo
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.collectionInfo
import androidx.compose.ui.semantics.collectionItemInfo
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import com.thekozugroup.popcornstash.navigation.Tab
import com.thekozugroup.popcornstash.ui.icons.LucideIcon
import com.thekozugroup.popcornstash.ui.theme.OnAccentRipple
import com.thekozugroup.popcornstash.ui.theme.Popcorn
import com.thekozugroup.popcornstash.ui.theme.PopcornMotion

/**
 * The app's navigation (android/DESIGN.md, Floating toolbar): one rounded bar of square
 * targets, icons only, with an Action Blue square behind the selected one that slides
 * to the tapped square. The bar's outer radius is the square radius plus its padding,
 * so the selected square sits concentric in the end squares.
 *
 * The square carries a thin `surface` ring, so its edge keeps 3:1 against whatever the glass
 * shows. Each icon is white where the square is under it and `textMuted` elsewhere, drawn
 * from the square's own position, so the colours move with the square while it slides.
 *
 * The bar is frosted glass where the phone can blur ([glass], see FrostedGlass.kt): the pages
 * scroll under it softened and tinted with `surface`. [backdrop] is the recording the
 * [GlassTier.Captured] glass draws from; without it, or on [GlassTier.Opaque], the bar is
 * the opaque `surface`.
 *
 * [readyToImport] is the number of files waiting in the Inbox; it badges Downloads.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun FloatingNavToolbar(
    selected: Tab,
    onSelect: (Tab) -> Unit,
    modifier: Modifier = Modifier,
    readyToImport: Int = 0,
    tabs: List<Tab> = Tab.entries,
    glass: GlassTier = GlassTier.Opaque,
    backdrop: GlassBackdrop? = null,
) {
    val tokens = Popcorn.toolbar
    val colors = Popcorn.colors
    val reduceMotion = Popcorn.reduceMotion
    val barShape = RoundedCornerShape(tokens.outerRadius)
    val squareShape = RoundedCornerShape(tokens.squareRadius)
    val frosted = glass == GlassTier.Backdrop || (glass == GlassTier.Captured && backdrop != null)
    val glassStyle = GlassStyle(
        tint = colors.glass,
        opaque = colors.surface,
        blur = tokens.glassBlur,
        saturation = tokens.glassSaturation,
        elevation = tokens.elevation,
        shadow = colors.toolbarShadow,
    )

    BoxWithConstraints(modifier) {
        // Shrink the squares on a narrow screen, never below the 48dp touch minimum.
        val room = maxWidth - tokens.margin * 2 - tokens.padding * 2 - tokens.gap * (tabs.size - 1)
        val square: Dp = (room / tabs.size).coerceIn(tokens.minSquare, tokens.square)
        val selectedIndex = tabs.indexOf(selected).coerceAtLeast(0)
        // From the bar's start edge; read only while placing and drawing, so a slide recomposes nothing.
        val indicatorOffset = animateDpAsState(
            targetValue = (square + tokens.gap) * selectedIndex,
            animationSpec = if (reduceMotion) snap() else PopcornMotion.indicatorSpring(),
            label = "toolbar indicator",
        )
        val ringShape = RoundedCornerShape(tokens.squareRadius + tokens.indicatorRing)

        Box(
            Modifier
                .testTag("toolbar")
                .frostedGlass(glass, backdrop, barShape, glassStyle)
                .border(tokens.border, if (frosted) colors.glassEdge else colors.divider, barShape)
                .padding(tokens.padding)
                .semantics { collectionInfo = CollectionInfo(rowCount = 1, columnCount = tabs.size) }
                .selectableGroup(),
        ) {
            Box(
                Modifier
                    .offset { IntOffset(indicatorOffset.value.roundToPx(), 0) }
                    .testTag("toolbar indicator")
                    .size(square)
                    .drawBehind {
                        // Outside the square, so the blue keeps its size and its neighbour is always `surface`.
                        val ring = tokens.indicatorRing.toPx()
                        val outline = ringShape.createOutline(
                            Size(size.width + ring * 2, size.height + ring * 2),
                            layoutDirection,
                            this,
                        )
                        translate(-ring, -ring) { drawOutline(outline, colors.surface) }
                    }
                    .background(colors.accent, squareShape),
            )
            Row {
                tabs.forEachIndexed { index, tab ->
                    if (index > 0) Spacer(Modifier.width(tokens.gap))
                    ToolbarSquare(
                        tab = tab,
                        index = index,
                        count = tabs.size,
                        selected = tab == selected,
                        size = square,
                        indicatorStart = { indicatorOffset.value - (square + tokens.gap) * index },
                        badge = if (tab == Tab.Downloads) readyToImport else 0,
                        onClick = { onSelect(tab) },
                    )
                }
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ToolbarSquare(
    tab: Tab,
    index: Int,
    count: Int,
    selected: Boolean,
    size: Dp,
    /** Where the selection square starts, measured from this square's start edge. */
    indicatorStart: () -> Dp,
    badge: Int,
    onClick: () -> Unit,
) {
    val tokens = Popcorn.toolbar
    val colors = Popcorn.colors
    val description = when {
        tab == Tab.Downloads && badge > 0 -> "${tab.label}, $badge ready to import"
        else -> tab.label
    }
    // A press on the blue square lightens it only as far as white keeps 4.5:1 (OnAccentRipple).
    val ripples = if (selected) OnAccentRipple else LocalRippleConfiguration.current
    TooltipBox(
        positionProvider = TooltipDefaults.rememberTooltipPositionProvider(TooltipAnchorPosition.Above),
        tooltip = { PlainTooltip { Text(tab.label) } },
        state = rememberTooltipState(),
    ) {
        Box(Modifier.size(size)) {
            CompositionLocalProvider(LocalRippleConfiguration provides ripples) {
                Box(
                    Modifier
                        .testTag("tab ${tab.name}")
                        .size(size)
                        .clip(RoundedCornerShape(tokens.squareRadius))
                        .selectable(
                            selected = selected,
                            onClick = onClick,
                            role = Role.Tab,
                            interactionSource = remember { MutableInteractionSource() },
                            indication = ripple(color = if (selected) colors.onAccent else colors.text),
                        )
                        .semantics {
                            contentDescription = description
                            collectionItemInfo = CollectionItemInfo(
                                rowIndex = 0,
                                rowSpan = 1,
                                columnIndex = index,
                                columnSpan = 1,
                            )
                        },
                ) {
                    // The icon twice: muted beside the square, white over it.
                    IconPart(indicatorStart, overSquare = false) {
                        LucideIcon(tab.glyph, contentDescription = null, tint = colors.textMuted, size = tokens.icon)
                    }
                    IconPart(indicatorStart, overSquare = true) {
                        LucideIcon(tab.glyph, contentDescription = null, tint = colors.onAccent, size = tokens.icon)
                    }
                }
            }
            if (badge > 0) {
                ToolbarBadge(
                    count = badge,
                    modifier = Modifier
                        .align(Alignment.TopEnd)
                        .offset(x = tokens.badgeOffset, y = -tokens.badgeOffset),
                )
            }
        }
    }
}

/**
 * The part of a square's icon that is over the selection square ([overSquare]) or beside it.
 * The square is as tall as this one and only slides along the bar, so its vertical edge is the
 * boundary; its rounded corners stay clear of the icon, which keeps the square's corner radius
 * of space on every side, even at the 48dp minimum.
 */
@Composable
private fun BoxScope.IconPart(indicatorStart: () -> Dp, overSquare: Boolean, icon: @Composable () -> Unit) {
    Box(
        Modifier
            .matchParentSize()
            .drawWithContent {
                val start = indicatorStart().toPx()
                // The square is this square's width; in a right-to-left bar its start is the right edge.
                val left = if (layoutDirection == LayoutDirection.Ltr) start else -start
                clipRect(left, 0f, left + size.width, size.height, if (overSquare) ClipOp.Intersect else ClipOp.Difference) {
                    this@drawWithContent.drawContent()
                }
            },
        contentAlignment = Alignment.Center,
    ) { icon() }
}

/** The count of files ready to import, in the corner of the Downloads square. Never a status colour. */
@Composable
private fun ToolbarBadge(count: Int, modifier: Modifier = Modifier) {
    val tokens = Popcorn.toolbar
    val colors = Popcorn.colors
    val shape = RoundedCornerShape(tokens.badgeHeight / 2)
    Text(
        text = if (count > 99) "99+" else count.toString(),
        style = Popcorn.type.tag,
        color = colors.onBadge,
        textAlign = TextAlign.Center,
        modifier = modifier
            .clearAndSetSemantics { }
            .defaultMinSize(minWidth = tokens.badgeHeight, minHeight = tokens.badgeHeight)
            .border(tokens.badgeRing, colors.surface, shape)
            .background(colors.badge, shape)
            .padding(horizontal = 5.dp, vertical = 1.dp),
    )
}
