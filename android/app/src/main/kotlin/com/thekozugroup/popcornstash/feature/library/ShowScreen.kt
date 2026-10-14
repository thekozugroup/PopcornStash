package com.thekozugroup.popcornstash.feature.library

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.thekozugroup.popcornstash.data.api.Item
import com.thekozugroup.popcornstash.data.api.ShowDetail
import com.thekozugroup.popcornstash.data.repo.Loadable
import com.thekozugroup.popcornstash.ui.components.LoadFailed
import com.thekozugroup.popcornstash.ui.components.MediaPicture
import com.thekozugroup.popcornstash.ui.components.PageTopBar
import com.thekozugroup.popcornstash.ui.components.PlaceholderLine
import com.thekozugroup.popcornstash.ui.components.PlaceholderTile
import com.thekozugroup.popcornstash.ui.components.PrimaryButton
import com.thekozugroup.popcornstash.ui.components.TileShape
import com.thekozugroup.popcornstash.ui.icons.LibraryIcons
import com.thekozugroup.popcornstash.ui.icons.LucideGlyph
import com.thekozugroup.popcornstash.ui.icons.LucideIcon
import com.thekozugroup.popcornstash.ui.icons.PopcornIcons
import com.thekozugroup.popcornstash.ui.media.LocalPictures
import com.thekozugroup.popcornstash.ui.theme.Popcorn

/** What a show page does when tapped. */
class ShowActions(
    val back: () -> Unit = {},
    val retry: () -> Unit = {},
    val selectSeason: (Int) -> Unit = {},
    val play: (Item, fromStart: Boolean) -> Unit = { _, _ -> },
    val openEpisode: (Item) -> Unit = {},
)

/**
 * A show (android/DESIGN.md, Series): its picture, "Resume S01E02" or "Play S01E01" as the
 * one primary button, the season picker, then the season's episodes with what was watched.
 */
@Composable
fun ShowScreen(
    name: String,
    icon: String,
    ui: ShowViewModel.Ui,
    bottomClearance: Dp,
    actions: ShowActions,
    modifier: Modifier = Modifier,
    format: MediaFormat = MediaFormat(),
    listState: LazyListState = rememberLazyListState(),
) {
    val glyph = LibraryIcons.glyph(icon)
    val show = ui.show.valueOrNull
    // The bar's title waits until the page's own heading (under the picture) has scrolled away.
    val headingGone by remember { derivedStateOf { listState.firstVisibleItemIndex > 1 } }
    Column(modifier.fillMaxSize()) {
        PageTopBar(title = name, onBack = actions.back, scrolled = listState.canScrollBackward, titleVisible = show == null || headingGone)
        when {
            show == null && ui.show is Loadable.Failed -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                LoadFailed(ui.show.message, actions.retry)
            }
            show == null -> ShowPlaceholder()
            else -> ShowContent(show, ui, glyph, listState, bottomClearance, format, actions)
        }
    }
}

@Composable
private fun ShowContent(
    show: ShowDetail,
    ui: ShowViewModel.Ui,
    glyph: LucideGlyph,
    listState: LazyListState,
    bottomClearance: Dp,
    format: MediaFormat,
    actions: ShowActions,
) {
    val pictures = LocalPictures.current
    val gutter = Popcorn.spacing.gutter
    val upNext = ui.upNext
    val season = show.seasons.firstOrNull { it.season == ui.season } ?: show.seasons.firstOrNull()
    val episodeCount = show.seasons.sumOf { it.episodes.size }.toLong()
    LazyColumn(
        state = listState,
        contentPadding = PaddingValues(top = Popcorn.spacing.xs, bottom = bottomClearance),
        modifier = Modifier.fillMaxSize().testTag("show page"),
    ) {
        item(key = "picture") {
            MediaPicture(
                url = show.posterItemId?.let { pictures.thumbOf(it, large = true) },
                shape = TileShape.Frame,
                glyph = glyph,
                modifier = Modifier.padding(horizontal = gutter).fillMaxWidth(),
            )
        }
        item(key = "heading") {
            Column(Modifier.padding(horizontal = gutter)) {
                Spacer(Modifier.height(Popcorn.spacing.l))
                Text(show.showName, style = Popcorn.type.headline, color = Popcorn.colors.text, modifier = Modifier.semantics { heading() })
                Spacer(Modifier.height(Popcorn.spacing.xxs))
                Text(
                    format.join(
                        if (show.seasons.size > 1) format.count(show.seasons.size.toLong(), "season") else null,
                        format.count(episodeCount, "episode"),
                    ),
                    style = Popcorn.type.metadata,
                    color = Popcorn.colors.textMuted,
                )
                if (upNext != null) {
                    Spacer(Modifier.height(Popcorn.spacing.l))
                    val code = MediaFormat.episodeCode(upNext.episode.season, upNext.episode.episode)
                    PrimaryButton(
                        text = if (upNext.resume) "Resume $code" else "Play $code",
                        glyph = PopcornIcons.Play,
                        onClick = { actions.play(upNext.episode, !upNext.resume) },
                        modifier = Modifier.fillMaxWidth(),
                    )
                }
            }
        }
        if (show.seasons.size > 1 && season != null) {
            item(key = "seasons") {
                Box(Modifier.padding(start = gutter, end = gutter, top = Popcorn.spacing.xl)) {
                    SeasonPicker(show.seasons.map { it.season }, season.season, actions.selectSeason)
                }
            }
        }
        item(key = "episodes gap") { Spacer(Modifier.height(Popcorn.spacing.l)) }
        if (season != null) {
            items(season.episodes, key = { it.id }) { episode ->
                EpisodeRow(episode, upNext, glyph, format) { actions.openEpisode(episode) }
            }
        }
    }
}

/**
 * The season choice: segmented buttons for up to four seasons, a menu beyond. Season 0
 * reads "Specials".
 */
@Composable
private fun SeasonPicker(seasons: List<Int>, selected: Int, onSelect: (Int) -> Unit) {
    val colors = Popcorn.colors
    if (seasons.size <= 4) {
        SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
            seasons.forEachIndexed { index, season ->
                SegmentedButton(
                    selected = season == selected,
                    onClick = { onSelect(season) },
                    shape = SegmentedButtonDefaults.itemShape(index, seasons.size, RoundedCornerShape(8.dp)),
                    icon = {},
                    colors = SegmentedButtonDefaults.colors(
                        activeContainerColor = colors.subtle,
                        activeContentColor = colors.text,
                        activeBorderColor = colors.outline,
                        inactiveContainerColor = colors.page,
                        inactiveContentColor = colors.textMuted,
                        inactiveBorderColor = colors.outline,
                    ),
                    modifier = Modifier.heightIn(min = 48.dp),
                ) {
                    Text(
                        if (seasons.size > 3 && season != 0) "S$season" else MediaFormat.seasonName(season),
                        style = Popcorn.type.action,
                        maxLines = 1,
                        modifier = Modifier.semantics { if (seasons.size > 3) stateDescription = MediaFormat.seasonName(season) },
                    )
                }
            }
        }
    } else {
        var open by remember { mutableStateOf(false) }
        Box {
            OutlinedButton(
                onClick = { open = true },
                shape = Popcorn.shapes.control,
                border = BorderStroke(1.dp, colors.outline),
                modifier = Modifier.heightIn(min = 48.dp),
            ) {
                Text(MediaFormat.seasonName(selected), style = Popcorn.type.action, color = colors.text)
                Spacer(Modifier.width(Popcorn.spacing.xs))
                LucideIcon(PopcornIcons.Expand, contentDescription = null, tint = colors.textMuted, size = 18.dp)
            }
            DropdownMenu(
                expanded = open,
                onDismissRequest = { open = false },
                containerColor = colors.surface,
                shape = Popcorn.shapes.control,
                border = BorderStroke(1.dp, colors.divider),
            ) {
                seasons.forEach { season ->
                    DropdownMenuItem(
                        text = { Text(MediaFormat.seasonName(season), style = Popcorn.type.body) },
                        trailingIcon = { if (season == selected) LucideIcon(PopcornIcons.Chosen, contentDescription = null, size = 20.dp) },
                        onClick = {
                            open = false
                            onSelect(season)
                        },
                    )
                }
            }
        }
    }
}

/**
 * One episode: its 16:9 frame with the watched part, its name, and its state: "Watched",
 * "Up next" with what is left or its length, or just its length.
 */
@Composable
private fun EpisodeRow(episode: Item, upNext: UpNext?, glyph: LucideGlyph, format: MediaFormat, onClick: () -> Unit) {
    val colors = Popcorn.colors
    val isNext = upNext?.episode?.id == episode.id
    val state = when {
        episode.progress?.finished == true -> "Watched"
        isNext && upNext.resume -> format.join("Up next", format.timeLeft(episode))
        isNext -> format.join("Up next", format.duration(episode.durationS))
        else -> format.duration(episode.durationS)
    }
    Row(
        Modifier
            .fillMaxWidth()
            .clickable(role = Role.Button, onClickLabel = "Details", onClick = onClick)
            .padding(horizontal = Popcorn.spacing.gutter, vertical = Popcorn.spacing.xs)
            .semantics(mergeDescendants = true) { },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        MediaPicture(
            url = LocalPictures.current.thumb(episode),
            shape = TileShape.Frame,
            glyph = glyph,
            progress = format.watchedFraction(episode.progress),
            modifier = Modifier.width(Popcorn.media.episodeFrame),
        )
        Spacer(Modifier.width(Popcorn.spacing.s))
        Column(Modifier.weight(1f)) {
            // "3. The lantern keepers"; an episode without a title is already "Episode 3".
            val number = episode.episode?.takeIf { !episode.episodeTitle.isNullOrBlank() }?.let { "$it." }
            Text(
                format.join(number, MediaFormat.episodeName(episode), separator = " "),
                style = Popcorn.type.item,
                color = colors.text,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            if (state.isNotBlank()) {
                Spacer(Modifier.height(2.dp))
                Text(state, style = Popcorn.type.metadata, color = if (isNext) colors.text else colors.textMuted, maxLines = 1)
            }
        }
    }
}

@Composable
private fun ShowPlaceholder() {
    val gutter = Popcorn.spacing.gutter
    Column(Modifier.padding(top = Popcorn.spacing.xs)) {
        PlaceholderTile(TileShape.Frame, Modifier.padding(horizontal = gutter), caption = false)
        Spacer(Modifier.height(Popcorn.spacing.l))
        PlaceholderLine(Modifier.padding(horizontal = gutter).width(200.dp), height = 28.dp)
        Spacer(Modifier.height(Popcorn.spacing.s))
        PlaceholderLine(Modifier.padding(horizontal = gutter).width(120.dp))
        Spacer(Modifier.height(Popcorn.spacing.l))
        PlaceholderLine(Modifier.padding(horizontal = gutter).fillMaxWidth(), height = 48.dp)
        Spacer(Modifier.height(Popcorn.spacing.xl))
        repeat(3) {
            Row(Modifier.padding(horizontal = gutter, vertical = Popcorn.spacing.xs), verticalAlignment = Alignment.CenterVertically) {
                PlaceholderTile(TileShape.Frame, Modifier.width(Popcorn.media.episodeFrame), caption = false)
                Spacer(Modifier.width(Popcorn.spacing.s))
                Column {
                    PlaceholderLine(Modifier.width(140.dp))
                    Spacer(Modifier.height(8.dp))
                    PlaceholderLine(Modifier.width(80.dp))
                }
            }
        }
    }
}
