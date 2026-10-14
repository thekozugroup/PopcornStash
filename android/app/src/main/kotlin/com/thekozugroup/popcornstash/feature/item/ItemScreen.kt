package com.thekozugroup.popcornstash.feature.item

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.OutlinedIconToggleButton
import androidx.compose.material3.PlainTooltip
import androidx.compose.material3.Text
import androidx.compose.material3.TooltipAnchorPosition
import androidx.compose.material3.TooltipBox
import androidx.compose.material3.TooltipDefaults
import androidx.compose.material3.rememberTooltipState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.thekozugroup.popcornstash.data.api.Category
import com.thekozugroup.popcornstash.data.api.Item
import com.thekozugroup.popcornstash.data.api.ItemDetail
import com.thekozugroup.popcornstash.data.api.ItemStatus
import com.thekozugroup.popcornstash.data.api.MediaKind
import com.thekozugroup.popcornstash.data.repo.Loadable
import com.thekozugroup.popcornstash.feature.library.MediaFormat
import com.thekozugroup.popcornstash.feature.library.displayTitle
import com.thekozugroup.popcornstash.ui.components.LoadFailed
import com.thekozugroup.popcornstash.ui.components.MediaPicture
import com.thekozugroup.popcornstash.ui.components.PageTopBar
import com.thekozugroup.popcornstash.ui.components.PlaceholderLine
import com.thekozugroup.popcornstash.ui.components.PlaceholderTile
import com.thekozugroup.popcornstash.ui.components.PrimaryButton
import com.thekozugroup.popcornstash.ui.components.QuietButton
import com.thekozugroup.popcornstash.ui.components.SecondaryButton
import com.thekozugroup.popcornstash.ui.components.TileShape
import com.thekozugroup.popcornstash.ui.icons.LibraryIcons
import com.thekozugroup.popcornstash.ui.icons.LucideIcon
import com.thekozugroup.popcornstash.ui.icons.PopcornIcons
import com.thekozugroup.popcornstash.ui.media.LocalPictures
import com.thekozugroup.popcornstash.ui.media.StashImage
import com.thekozugroup.popcornstash.ui.theme.Popcorn

/** What an item page does when tapped. */
class ItemActions(
    val back: () -> Unit = {},
    val retry: () -> Unit = {},
    val play: (fromStart: Boolean) -> Unit = {},
    val toggleFavourite: () -> Unit = {},
    val openNext: (Item) -> Unit = {},
    val viewPhoto: () -> Unit = {},
)

/**
 * An item (android/DESIGN.md, Item detail): its picture, title and one line of facts, the
 * one primary button ("Resume from 12:04" or "Play") with "Start from the beginning" under
 * it, the favourite, the facts as label-over-value columns, and for an episode the next one
 * in the same library.
 */
@Composable
fun ItemScreen(
    title: String,
    ui: ItemViewModel.Ui,
    bottomClearance: Dp,
    actions: ItemActions,
    modifier: Modifier = Modifier,
    libraryIcon: String? = null,
    format: MediaFormat = MediaFormat(),
    listState: LazyListState = rememberLazyListState(),
) {
    val detail = ui.item.valueOrNull
    val headingGone by remember { derivedStateOf { listState.firstVisibleItemIndex > 1 } }
    Column(modifier.fillMaxSize()) {
        PageTopBar(
            title = detail?.let { format.displayTitle(it.item) } ?: title,
            onBack = actions.back,
            scrolled = listState.canScrollBackward,
            titleVisible = detail == null || headingGone,
        )
        when {
            detail == null && ui.item is Loadable.Failed -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                LoadFailed(ui.item.message, actions.retry)
            }
            detail == null -> ItemPlaceholder()
            else -> ItemContent(detail, ui, libraryIcon, listState, bottomClearance, format, actions)
        }
    }
}

@Composable
private fun ItemContent(
    detail: ItemDetail,
    ui: ItemViewModel.Ui,
    libraryIcon: String?,
    listState: LazyListState,
    bottomClearance: Dp,
    format: MediaFormat,
    actions: ItemActions,
) {
    val item = detail.item
    val gutter = Popcorn.spacing.gutter
    val pictures = LocalPictures.current
    val isVideo = item.kind == MediaKind.Video
    val isPhoto = item.kind == MediaKind.Photo
    LazyColumn(
        state = listState,
        contentPadding = PaddingValues(top = Popcorn.spacing.xs, bottom = bottomClearance),
        modifier = Modifier.fillMaxSize().testTag("item page"),
    ) {
        item(key = "picture") {
            if (isPhoto) {
                // A photo keeps its own shape (within reason) and opens the viewer.
                val ratio = if ((item.width ?: 0) > 0 && (item.height ?: 0) > 0) {
                    (item.width!!.toFloat() / item.height!!).coerceIn(0.75f, 1.78f)
                } else {
                    1f
                }
                Box(
                    Modifier
                        .padding(horizontal = gutter)
                        .fillMaxWidth()
                        .aspectRatio(ratio)
                        .clip(Popcorn.shapes.media)
                        .background(Popcorn.colors.subtle)
                        .clickable(onClickLabel = "View", onClick = actions.viewPhoto),
                ) {
                    StashImage(pictures.thumb(item, large = true), contentDescription = null, modifier = Modifier.fillMaxSize())
                }
            } else {
                MediaPicture(
                    url = pictures.thumb(item, large = true),
                    shape = TileShape.Frame,
                    glyph = LibraryIcons.glyph(libraryIcon),
                    progress = format.watchedFraction(item.progress),
                    modifier = Modifier.padding(horizontal = gutter).fillMaxWidth(),
                )
            }
        }
        item(key = "heading") {
            Column(Modifier.padding(horizontal = gutter)) {
                Spacer(Modifier.height(Popcorn.spacing.l))
                Text(
                    format.displayTitle(item),
                    style = Popcorn.type.headline,
                    color = Popcorn.colors.text,
                    modifier = Modifier.semantics { heading() },
                )
                val line = format.detailLine(item)
                if (line.isNotBlank()) {
                    Spacer(Modifier.height(Popcorn.spacing.xxs))
                    Text(line, style = Popcorn.type.metadata, color = Popcorn.colors.textMuted)
                }
                if (item.status == ItemStatus.Missing) {
                    Spacer(Modifier.height(Popcorn.spacing.m))
                    MissingNotice()
                }
            }
        }
        item(key = "actions") {
            Column(Modifier.padding(start = gutter, end = gutter, top = Popcorn.spacing.l)) {
                if (isVideo) {
                    val resumeAt = item.progress?.takeIf { !it.finished && format.watchedFraction(it) != null }?.positionS
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        PrimaryButton(
                            text = if (resumeAt != null) "Resume from ${format.clock(resumeAt)}" else "Play",
                            glyph = PopcornIcons.Play,
                            onClick = { actions.play(false) },
                            modifier = Modifier.weight(1f),
                        )
                        Spacer(Modifier.width(Popcorn.spacing.xs))
                        FavouriteToggle(item.favorite, ui.savingFavourite, actions.toggleFavourite)
                    }
                    if (resumeAt != null) {
                        QuietButton(
                            "Start from the beginning",
                            onClick = { actions.play(true) },
                            modifier = Modifier.padding(top = Popcorn.spacing.xxs).offset(x = (-12).dp),
                        )
                    }
                } else {
                    SecondaryButton(
                        text = if (item.favorite) "Remove from favourites" else "Add to favourites",
                        glyph = PopcornIcons.Favourite,
                        onClick = actions.toggleFavourite,
                        modifier = Modifier.semantics { stateDescription = if (item.favorite) "In favourites" else "Not in favourites" },
                    )
                }
            }
        }
        val next = ui.next
        if (item.category == Category.Episode && next != null) {
            item(key = "next") {
                Column(Modifier.padding(top = Popcorn.spacing.xl)) {
                    NextEpisodeRow(next, libraryIcon, format) { actions.openNext(next) }
                }
            }
        }
        item(key = "facts") {
            Column(Modifier.padding(start = gutter, end = gutter, top = Popcorn.spacing.xl)) {
                HorizontalDivider(thickness = 1.dp, color = Popcorn.colors.divider)
                Spacer(Modifier.height(Popcorn.spacing.l))
                Facts(facts(detail, format))
            }
        }
    }
}

/** The favourite beside Play: a 48dp outlined square with the heart, filled while it is on. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun FavouriteToggle(on: Boolean, saving: Boolean, onToggle: () -> Unit) {
    val label = if (on) "Remove from favourites" else "Add to favourites"
    TooltipBox(
        positionProvider = TooltipDefaults.rememberTooltipPositionProvider(TooltipAnchorPosition.Above),
        tooltip = { PlainTooltip { Text(label) } },
        state = rememberTooltipState(),
    ) {
        OutlinedIconToggleButton(
            checked = on,
            onCheckedChange = { if (!saving) onToggle() },
            shape = Popcorn.shapes.control,
            border = BorderStroke(1.dp, Popcorn.colors.outline),
            colors = IconButtonDefaults.outlinedIconToggleButtonColors(
                contentColor = Popcorn.colors.text,
                checkedContainerColor = Popcorn.colors.subtle,
                checkedContentColor = Popcorn.colors.text,
            ),
            modifier = Modifier
                .size(48.dp)
                .semantics { contentDescription = label },
        ) {
            LucideIcon(PopcornIcons.Favourite, contentDescription = null, tint = Popcorn.colors.text, filled = on, size = 22.dp)
        }
    }
}

/** "Next: S01E03 The lantern keepers", a row with the episode's frame that opens it. */
@Composable
private fun NextEpisodeRow(next: Item, libraryIcon: String?, format: MediaFormat, onOpen: () -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .clickable(onClickLabel = "Open", onClick = onOpen)
            .padding(horizontal = Popcorn.spacing.gutter, vertical = Popcorn.spacing.xs)
            .semantics(mergeDescendants = true) { },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        MediaPicture(
            url = LocalPictures.current.thumb(next),
            shape = TileShape.Frame,
            glyph = LibraryIcons.glyph(libraryIcon),
            modifier = Modifier.width(96.dp),
        )
        Spacer(Modifier.width(Popcorn.spacing.s))
        Column(Modifier.weight(1f)) {
            Text("Next", style = Popcorn.type.metadata, color = Popcorn.colors.textMuted)
            Text(
                format.join(MediaFormat.episodeCode(next.season, next.episode), MediaFormat.episodeName(next), separator = " "),
                style = Popcorn.type.item,
                color = Popcorn.colors.text,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
        }
        LucideIcon(PopcornIcons.Open, contentDescription = null, tint = Popcorn.colors.textMuted, size = 20.dp)
    }
}

/** What a file is, in words: video, audio, subtitles, camera, dimensions, size. */
fun facts(detail: ItemDetail, format: MediaFormat): List<Pair<String, String>> = buildList {
    val media = detail.media
    format.videoSummary(media?.video)?.let { add("Video" to it) }
    if (detail.kind == MediaKind.Photo && (detail.width ?: 0) > 0 && (detail.height ?: 0) > 0) {
        add("Dimensions" to "${detail.width} × ${detail.height}")
    }
    media?.let { format.audioSummary(it.audio) }?.let { add("Audio" to it) }
    media?.let { format.subtitleSummary(it.subtitles) }?.let { add("Subtitles" to it) }
    detail.camera?.takeIf { it.isNotBlank() }?.let { add("Camera" to it) }
    add("File size" to format.bytes(detail.size))
}

/** Label over value, two columns. */
@Composable
private fun Facts(facts: List<Pair<String, String>>) {
    Column(verticalArrangement = Arrangement.spacedBy(Popcorn.spacing.m)) {
        facts.chunked(2).forEach { row ->
            Row(horizontalArrangement = Arrangement.spacedBy(Popcorn.spacing.m)) {
                row.forEach { (label, value) ->
                    Column(Modifier.weight(1f).semantics(mergeDescendants = true) { }) {
                        Text(label, style = Popcorn.type.metadata, color = Popcorn.colors.textMuted)
                        Spacer(Modifier.height(2.dp))
                        Text(value, style = Popcorn.type.item, color = Popcorn.colors.text)
                    }
                }
                if (row.size == 1) Spacer(Modifier.weight(1f))
            }
        }
    }
}

@Composable
private fun MissingNotice() {
    Row(verticalAlignment = Alignment.Top, modifier = Modifier.semantics(mergeDescendants = true) { }) {
        Box(Modifier.padding(top = 6.dp).size(8.dp).background(Popcorn.colors.warningDot, Popcorn.shapes.avatar))
        Spacer(Modifier.width(Popcorn.spacing.xs))
        Text(
            "This file is missing. PopcornStash cannot find it on the server right now. If it was moved, it shows up again after the next scan.",
            style = Popcorn.type.metadata,
            color = Popcorn.colors.warning,
        )
    }
}

@Composable
private fun ItemPlaceholder() {
    val gutter = Popcorn.spacing.gutter
    Column(Modifier.padding(top = Popcorn.spacing.xs)) {
        PlaceholderTile(TileShape.Frame, Modifier.padding(horizontal = gutter), caption = false)
        Spacer(Modifier.height(Popcorn.spacing.l))
        PlaceholderLine(Modifier.padding(horizontal = gutter).width(220.dp), height = 28.dp)
        Spacer(Modifier.height(Popcorn.spacing.s))
        PlaceholderLine(Modifier.padding(horizontal = gutter).width(120.dp))
        Spacer(Modifier.height(Popcorn.spacing.l))
        PlaceholderLine(Modifier.padding(horizontal = gutter).fillMaxWidth().heightIn(min = 48.dp), height = 48.dp)
    }
}
