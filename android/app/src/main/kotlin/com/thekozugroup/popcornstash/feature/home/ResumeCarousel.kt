package com.thekozugroup.popcornstash.feature.home

import androidx.compose.animation.core.snap
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.PagerDefaults
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.IconButton
import androidx.compose.material3.PlainTooltip
import androidx.compose.material3.Text
import androidx.compose.material3.TooltipAnchorPosition
import androidx.compose.material3.TooltipBox
import androidx.compose.material3.TooltipDefaults
import androidx.compose.material3.rememberTooltipState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.thekozugroup.popcornstash.data.api.Item
import com.thekozugroup.popcornstash.feature.library.MediaFormat
import com.thekozugroup.popcornstash.ui.components.MediaPicture
import com.thekozugroup.popcornstash.ui.components.TileShape
import com.thekozugroup.popcornstash.ui.icons.LibraryIcons
import com.thekozugroup.popcornstash.ui.icons.LucideIcon
import com.thekozugroup.popcornstash.ui.icons.PopcornIcons
import com.thekozugroup.popcornstash.ui.media.LocalPictures
import com.thekozugroup.popcornstash.ui.theme.Popcorn

/**
 * Continue watching (android/DESIGN.md, Home): a pager of 16:9 frames, each card the
 * gutter-to-gutter width minus the peek of the next card, snapping one card at a time
 * (no glide under reduce motion). Tapping a card resumes it at once; its More button
 * offers "Details" and "Start from the beginning". No dots: the peek shows there is more.
 */
@Composable
fun ResumeCarousel(
    items: List<Item>,
    format: MediaFormat,
    libraryIcon: (Item) -> String?,
    onResume: (Item) -> Unit,
    onDetails: (Item) -> Unit,
    onFromStart: (Item) -> Unit,
    modifier: Modifier = Modifier,
) {
    val carousel = Popcorn.carousel
    val gutter = Popcorn.spacing.gutter
    val state = rememberPagerState { items.size }
    val fling = if (Popcorn.reduceMotion) {
        PagerDefaults.flingBehavior(state, snapAnimationSpec = snap())
    } else {
        PagerDefaults.flingBehavior(state)
    }
    HorizontalPager(
        state = state,
        contentPadding = PaddingValues(start = gutter, end = gutter + carousel.peek),
        pageSpacing = carousel.gap,
        flingBehavior = fling,
        key = { items[it].id },
        verticalAlignment = Alignment.Top,
        modifier = modifier.fillMaxWidth().testTag("continue watching"),
    ) { index ->
        val item = items[index]
        ResumeCard(item, format, libraryIcon(item), onResume, onDetails, onFromStart)
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ResumeCard(
    item: Item,
    format: MediaFormat,
    icon: String?,
    onResume: (Item) -> Unit,
    onDetails: (Item) -> Unit,
    onFromStart: (Item) -> Unit,
) {
    val colors = Popcorn.colors
    val pictures = LocalPictures.current
    val description = format.resumeDescription(item)
    var menu by remember { mutableStateOf(false) }
    Column(Modifier.fillMaxWidth()) {
        MediaPicture(
            url = pictures.thumb(item, large = true),
            shape = TileShape.Frame,
            glyph = LibraryIcons.glyph(icon),
            progress = format.watchedFraction(item.progress),
            modifier = Modifier
                .fillMaxWidth()
                .clip(Popcorn.shapes.media)
                .clickable(role = Role.Button) { onResume(item) }
                .clearAndSetSemantics {
                    contentDescription = description
                    role = Role.Button
                },
        )
        Spacer(Modifier.height(Popcorn.media.captionGap))
        Row(verticalAlignment = Alignment.Top) {
            Column(Modifier.weight(1f).padding(top = 2.dp).clearAndSetSemantics { }) {
                Text(
                    format.resumeTitle(item),
                    style = Popcorn.type.item,
                    color = colors.text,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    format.resumeLine(item),
                    style = Popcorn.type.metadata,
                    color = colors.textMuted,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            // The button takes no more room than the two lines beside it; its 48dp target reaches
            // past them, and its glyph lines up with the frame's right edge.
            Box(Modifier.size(width = 24.dp, height = 40.dp), contentAlignment = Alignment.Center) {
                TooltipBox(
                    positionProvider = TooltipDefaults.rememberTooltipPositionProvider(TooltipAnchorPosition.Above),
                    tooltip = { PlainTooltip { Text("More") } },
                    state = rememberTooltipState(),
                ) {
                    IconButton(
                        onClick = { menu = true },
                        modifier = Modifier
                            .requiredSize(48.dp)
                            .semantics { contentDescription = "More for ${format.resumeTitle(item)}" },
                    ) {
                        LucideIcon(PopcornIcons.More, contentDescription = null, tint = colors.textMuted, size = 20.dp)
                    }
                }
                DropdownMenu(
                    expanded = menu,
                    onDismissRequest = { menu = false },
                    containerColor = colors.surface,
                    shape = Popcorn.shapes.control,
                    border = BorderStroke(1.dp, colors.divider),
                ) {
                    DropdownMenuItem(
                        text = { Text("Details", style = Popcorn.type.body) },
                        onClick = {
                            menu = false
                            onDetails(item)
                        },
                    )
                    DropdownMenuItem(
                        text = { Text("Start from the beginning", style = Popcorn.type.body) },
                        onClick = {
                            menu = false
                            onFromStart(item)
                        },
                    )
                }
            }
        }
    }
}
