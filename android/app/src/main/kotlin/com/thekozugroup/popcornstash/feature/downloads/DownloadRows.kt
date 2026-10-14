package com.thekozugroup.popcornstash.feature.downloads

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.IconButton
import androidx.compose.material3.MenuDefaults
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.ProgressBarRangeInfo
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.progressBarRangeInfo
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.thekozugroup.popcornstash.data.api.Download
import com.thekozugroup.popcornstash.data.api.DownloadState
import com.thekozugroup.popcornstash.share.UploadBatchProgress
import com.thekozugroup.popcornstash.ui.components.SecondaryButton
import com.thekozugroup.popcornstash.ui.icons.LibraryIcons
import com.thekozugroup.popcornstash.ui.icons.LucideGlyph
import com.thekozugroup.popcornstash.ui.icons.LucideGlyphs
import com.thekozugroup.popcornstash.ui.icons.LucideIcon
import com.thekozugroup.popcornstash.ui.icons.PopcornIcons
import com.thekozugroup.popcornstash.ui.theme.Popcorn

// The rows of the Downloads tab (android/DESIGN.md, Downloads and Rows): flat rows on the
// page, 1dp rules between them, a name, a metadata strip, trailing 48dp actions.

/** A section's heading: its title, the count beside it, and at most one quiet action. */
@Composable
fun DownloadsSectionHeader(title: String, count: Int?, modifier: Modifier = Modifier, action: (@Composable () -> Unit)? = null) {
    Row(
        modifier
            .fillMaxWidth()
            .heightIn(min = 48.dp)
            .padding(start = Popcorn.spacing.gutter, end = Popcorn.spacing.xs, top = Popcorn.spacing.l),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(title, style = Popcorn.type.title, color = Popcorn.colors.text, modifier = Modifier.semantics { heading() })
        if (count != null) {
            Spacer(Modifier.width(Popcorn.spacing.xs))
            Text(count.toString(), style = Popcorn.type.metadata, color = Popcorn.colors.textMuted)
        }
        Spacer(Modifier.weight(1f))
        action?.invoke()
    }
}

/** The 1dp rule between rows. */
@Composable
fun RowRule(start: androidx.compose.ui.unit.Dp = Popcorn.spacing.gutter) {
    Box(
        Modifier
            .padding(start = start)
            .fillMaxWidth()
            .height(1.dp)
            .background(Popcorn.colors.divider),
    )
}

/**
 * A 4dp progress bar: Action Blue on `subtle` while it moves, the muted text colour while it
 * waits or is paused. [fraction] null draws the empty track (size not known yet); nothing loops.
 */
@Composable
fun ThinProgress(fraction: Double?, moving: Boolean, label: String, modifier: Modifier = Modifier) {
    val colors = Popcorn.colors
    val value = (fraction ?: 0.0).toFloat().coerceIn(0f, 1f)
    Box(
        modifier
            .fillMaxWidth()
            .height(Popcorn.media.progressHeight)
            .background(colors.subtle, Popcorn.shapes.tag)
            .semantics {
                contentDescription = label
                progressBarRangeInfo = ProgressBarRangeInfo(value, 0f..1f)
            },
    ) {
        if (value > 0f) {
            Box(
                Modifier
                    .fillMaxWidth(value)
                    .fillMaxHeight()
                    .background(if (moving) colors.accent else colors.textMuted, Popcorn.shapes.tag),
            )
        }
    }
}

/** Facts spaced 12dp apart ("64%  2.1 MB/s  12 min left"), wrapping on a narrow phone. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun FactsStrip(facts: List<String>, modifier: Modifier = Modifier, color: Color = Popcorn.colors.textMuted) {
    FlowRow(modifier, horizontalArrangement = Arrangement.spacedBy(Popcorn.spacing.s)) {
        facts.forEach { Text(it, style = Popcorn.type.metadata, color = color) }
    }
}

/** A 48dp icon button for a row, with its name for screen readers and a tooltip. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun RowIconButton(glyph: LucideGlyph, label: String, description: String, onClick: () -> Unit, busy: Boolean = false, enabled: Boolean = true) {
    TooltipBox(
        positionProvider = TooltipDefaults.rememberTooltipPositionProvider(TooltipAnchorPosition.Above),
        tooltip = { PlainTooltip { Text(label) } },
        state = rememberTooltipState(),
    ) {
        IconButton(
            onClick = { if (!busy) onClick() },
            enabled = enabled,
            modifier = Modifier.semantics { contentDescription = description },
        ) {
            if (busy) {
                CircularProgressIndicator(color = Popcorn.colors.textMuted, strokeWidth = 2.dp, modifier = Modifier.size(18.dp))
            } else {
                LucideIcon(glyph, contentDescription = null, tint = Popcorn.colors.text, size = 22.dp)
            }
        }
    }
}

/** One choice in a row's More menu. */
data class MenuChoice(val label: String, val destructive: Boolean = false, val onClick: () -> Unit)

/** The row's More button and its menu. */
@Composable
fun RowMoreMenu(name: String, choices: List<MenuChoice>, enabled: Boolean = true) {
    var open by remember { mutableStateOf(false) }
    Box {
        RowIconButton(PopcornIcons.More, "More", "More for $name", onClick = { open = true }, enabled = enabled)
        DropdownMenu(
            expanded = open,
            onDismissRequest = { open = false },
            containerColor = Popcorn.colors.surface,
            shape = Popcorn.shapes.control,
            shadowElevation = 8.dp,
            border = androidx.compose.foundation.BorderStroke(1.dp, Popcorn.colors.divider),
        ) {
            choices.forEach { choice ->
                DropdownMenuItem(
                    text = { Text(choice.label, style = Popcorn.type.item) },
                    onClick = {
                        open = false
                        choice.onClick()
                    },
                    colors = MenuDefaults.itemColors(textColor = if (choice.destructive) Popcorn.colors.error else Popcorn.colors.text),
                    modifier = Modifier.heightIn(min = 48.dp),
                )
            }
        }
    }
}

// ------------------------------------------------------------------- VPN ----

/**
 * The VPN line under the top bar, in the web's words, with the status dot beside the label.
 * For the owner it opens the VPN settings on the web (trailing `external-link`).
 */
@Composable
fun VpnLineRow(line: VpnLine, onOpenSettings: (() -> Unit)?, modifier: Modifier = Modifier) {
    val colors = Popcorn.colors
    val (dot, words) = when (line.tone) {
        VpnTone.Connected -> colors.connectedDot to colors.connected
        VpnTone.Warning -> colors.warningDot to colors.warning
        VpnTone.Error -> colors.errorDot to colors.error
        VpnTone.Neutral -> colors.textMuted to colors.textMuted
    }
    val opens = line.opensSettings && onOpenSettings != null
    Row(
        modifier
            .fillMaxWidth()
            .heightIn(min = 56.dp)
            .then(
                if (opens) {
                    Modifier.clickable(onClickLabel = "Open VPN settings on the web", role = Role.Button) { onOpenSettings?.invoke() }
                } else {
                    Modifier
                },
            )
            .padding(start = Popcorn.spacing.gutter, end = Popcorn.spacing.gutter, top = Popcorn.spacing.xs, bottom = Popcorn.spacing.s)
            .semantics(mergeDescendants = true) { liveRegion = LiveRegionMode.Polite }
            .testTag("vpn line"),
        verticalAlignment = Alignment.Top,
    ) {
        Box(Modifier.padding(top = 6.dp).size(8.dp).background(dot, Popcorn.shapes.avatar))
        Spacer(Modifier.width(Popcorn.spacing.s))
        Column(Modifier.weight(1f)) {
            Text(line.label, style = Popcorn.type.item, color = words)
            Text(line.detail, style = Popcorn.type.metadata, color = colors.textMuted)
        }
        if (opens) {
            Spacer(Modifier.width(Popcorn.spacing.s))
            LucideIcon(PopcornIcons.OpenOnWeb, contentDescription = null, tint = colors.textMuted, size = 20.dp, modifier = Modifier.padding(top = 2.dp))
        }
    }
}

/** "Not connected to the server. Trying again." with the warning dot, while the events are cut off. */
@Composable
fun ConnectionLostLine(modifier: Modifier = Modifier) {
    Row(
        modifier
            .fillMaxWidth()
            .padding(horizontal = Popcorn.spacing.gutter, vertical = Popcorn.spacing.xs)
            .semantics(mergeDescendants = true) { liveRegion = LiveRegionMode.Polite },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.size(8.dp).background(Popcorn.colors.warningDot, Popcorn.shapes.avatar))
        Spacer(Modifier.width(Popcorn.spacing.xs))
        Text("Not connected to the server. Trying again.", style = Popcorn.type.metadata, color = Popcorn.colors.warning)
    }
}

// ---------------------------------------------------------------- transfers ----

/** The common shape of an in-progress row: name, bar, facts, trailing actions. */
@Composable
private fun TransferRowLayout(
    name: String,
    fraction: Double?,
    moving: Boolean,
    facts: List<String>,
    factsAreSentence: Boolean,
    modifier: Modifier = Modifier,
    actions: @Composable RowScope.() -> Unit,
) {
    Row(
        modifier
            .fillMaxWidth()
            .padding(start = Popcorn.spacing.gutter, end = Popcorn.spacing.xxs, top = Popcorn.spacing.s, bottom = Popcorn.spacing.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(
                breakableName(name),
                style = Popcorn.type.item,
                color = Popcorn.colors.text,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            Spacer(Modifier.height(Popcorn.spacing.xs))
            ThinProgress(fraction, moving, label = "$name progress")
            Spacer(Modifier.height(6.dp))
            if (factsAreSentence) {
                Text(facts.joinToString(" "), style = Popcorn.type.metadata, color = Popcorn.colors.textMuted)
            } else {
                FactsStrip(facts)
            }
        }
        Spacer(Modifier.width(Popcorn.spacing.xxs))
        actions()
    }
}

/** A torrent or web download on its way in, live from `download` events. */
@Composable
fun DownloadRow(
    download: Download,
    busy: Boolean,
    onPauseOrResume: () -> Unit,
    onRemove: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val resumable = download.canResume()
    TransferRowLayout(
        name = download.name,
        fraction = download.knownProgress(),
        moving = download.state == DownloadState.Downloading || download.state == DownloadState.Seeding,
        facts = download.progressFacts(),
        factsAreSentence = download.isWaiting(),
        modifier = modifier.testTag("download ${download.id}"),
    ) {
        if (resumable || download.canPause()) {
            RowIconButton(
                glyph = if (resumable) PopcornIcons.Play else PopcornIcons.Pause,
                label = if (resumable) "Resume" else "Pause",
                description = "${if (resumable) "Resume" else "Pause"} ${download.name}",
                onClick = onPauseOrResume,
                busy = busy,
            )
        }
        RowMoreMenu(download.name, listOf(MenuChoice("Remove", destructive = true, onClick = onRemove)), enabled = !busy)
    }
}

/** Files shared from this phone while they upload (the WorkManager job's progress). */
@Composable
fun UploadRow(upload: UploadBatchProgress, onStop: () -> Unit, modifier: Modifier = Modifier) {
    val facts = if (upload.waiting) {
        listOf("Waiting for a connection to upload.")
    } else {
        listOfNotNull(
            "Uploading",
            upload.fraction?.let(::formatPercent),
            if (upload.totalFiles > 1) "${upload.doneFiles} of ${upload.totalFiles} done" else null,
        )
    }
    TransferRowLayout(
        name = upload.label,
        fraction = upload.fraction,
        moving = !upload.waiting,
        facts = facts,
        factsAreSentence = upload.waiting,
        modifier = modifier.testTag("upload ${upload.id}"),
    ) {
        RowMoreMenu(upload.label, listOf(MenuChoice("Stop uploading", destructive = true, onClick = onStop)))
    }
}

// ----------------------------------------------------------------- finished ----

/**
 * A finished download: "Finished 3 min ago", then where it went as a tappable line with the
 * library's 16dp icon ("In Kids", "In Other files", "2 waiting to import", or "Sorted").
 */
@Composable
fun FinishedRowView(
    row: FinishedRow,
    nowSeconds: Long,
    busy: Boolean,
    onOpenPlace: ((OutcomePlace) -> Unit)?,
    onShowWaiting: () -> Unit,
    onRetry: () -> Unit,
    onRemove: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val download = row.download
    Row(
        modifier
            .fillMaxWidth()
            .padding(start = Popcorn.spacing.gutter, end = Popcorn.spacing.xxs, top = Popcorn.spacing.s, bottom = Popcorn.spacing.xs)
            .testTag("finished ${download.id}"),
        verticalAlignment = Alignment.Top,
    ) {
        Column(Modifier.weight(1f)) {
            Text(
                breakableName(download.name),
                style = Popcorn.type.item,
                color = Popcorn.colors.text,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            FactsStrip(listOfNotNull(download.finishedWhen(nowSeconds), download.totalBytes?.takeIf { it > 0 }?.let { formatBytes(it) }))
            OutcomeLine(row.outcome, onOpenPlace, onShowWaiting)
        }
        Spacer(Modifier.width(Popcorn.spacing.xxs))
        val choices = buildList {
            if (download.canRetry()) add(MenuChoice("Try again", onClick = onRetry))
            if (canRemove(download)) add(MenuChoice("Remove", destructive = true, onClick = onRemove))
        }
        if (busy) {
            Box(Modifier.size(48.dp), contentAlignment = Alignment.Center) {
                CircularProgressIndicator(color = Popcorn.colors.textMuted, strokeWidth = 2.dp, modifier = Modifier.size(18.dp))
            }
        } else if (choices.isNotEmpty()) {
            RowMoreMenu(download.name, choices)
        }
    }
}

@Composable
private fun OutcomeLine(outcome: FinishedOutcome, onOpenPlace: ((OutcomePlace) -> Unit)?, onShowWaiting: () -> Unit) {
    val colors = Popcorn.colors
    when (outcome) {
        is FinishedOutcome.Note -> Text(
            outcome.text,
            style = Popcorn.type.metadata,
            color = if (outcome.tone == NoteTone.Error) colors.error else colors.textMuted,
            modifier = Modifier.padding(vertical = Popcorn.spacing.xxs),
        )
        is FinishedOutcome.Waiting -> OutcomeTarget(PopcornIcons.ReadyToImport, outcome.text(), "Show the files ready to import", onShowWaiting)
        is FinishedOutcome.Sorted -> {
            if (outcome.places.isEmpty()) {
                OutcomeTarget(LucideGlyphs.Check, "Sorted", null, null)
            } else {
                outcome.places.forEach { place ->
                    val glyph = if (place.libraryId == null) PopcornIcons.OtherFiles else LibraryIcons.glyph(place.icon)
                    OutcomeTarget(
                        glyph = glyph,
                        text = "In ${place.name}",
                        clickLabel = "Open ${place.name}",
                        onClick = onOpenPlace?.let { open -> { open(place) } },
                    )
                }
            }
        }
    }
}

/** The "In Kids" line: a 48dp target when it opens something, the 16dp icon and the words. */
@Composable
private fun OutcomeTarget(glyph: LucideGlyph, text: String, clickLabel: String?, onClick: (() -> Unit)?) {
    Row(
        Modifier
            .heightIn(min = if (onClick != null) 48.dp else 32.dp)
            .widthIn(min = 48.dp)
            .then(if (onClick != null) Modifier.clickable(onClickLabel = clickLabel, role = Role.Button, onClick = onClick) else Modifier)
            .padding(end = Popcorn.spacing.xs),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        LucideIcon(glyph, contentDescription = null, tint = Popcorn.colors.textMuted, size = 16.dp)
        Spacer(Modifier.width(Popcorn.spacing.xs))
        Text(text, style = Popcorn.type.item, color = Popcorn.colors.text)
        if (onClick != null) {
            Spacer(Modifier.width(Popcorn.spacing.xxs))
            LucideIcon(PopcornIcons.Open, contentDescription = null, tint = Popcorn.colors.textMuted, size = 16.dp)
        }
    }
}

// --------------------------------------------------------- loading and errors ----

/** Placeholder rows at their final sizes while nothing is loaded (no spinner on the page). */
@Composable
fun DownloadsPlaceholder(modifier: Modifier = Modifier) {
    Column(
        modifier
            .fillMaxWidth()
            .clearAndSetSemantics { contentDescription = "Loading downloads" },
    ) {
        Row(Modifier.padding(start = Popcorn.spacing.gutter, top = Popcorn.spacing.l, bottom = Popcorn.spacing.s)) {
            Bar(widthFraction = 0.32f, height = 20.dp)
        }
        repeat(3) { index ->
            Column(Modifier.padding(horizontal = Popcorn.spacing.gutter, vertical = Popcorn.spacing.s)) {
                Bar(widthFraction = if (index == 1) 0.5f else 0.72f, height = 14.dp)
                Spacer(Modifier.height(Popcorn.spacing.s))
                Bar(widthFraction = 1f, height = Popcorn.media.progressHeight)
                Spacer(Modifier.height(Popcorn.spacing.xs))
                Bar(widthFraction = 0.4f, height = 12.dp)
            }
            if (index < 2) RowRule()
        }
    }
}

@Composable
private fun Bar(widthFraction: Float, height: androidx.compose.ui.unit.Dp) {
    Box(
        Modifier
            .fillMaxWidth(widthFraction)
            .height(height)
            .background(Popcorn.colors.subtle, Popcorn.shapes.tag),
    )
}

/** A load that did not work: one sentence and "Try again", where the content would be. */
@Composable
fun LoadFailedBlock(title: String, message: String, onRetry: () -> Unit, modifier: Modifier = Modifier) {
    Column(
        modifier
            .fillMaxWidth()
            .padding(horizontal = Popcorn.spacing.gutter, vertical = Popcorn.spacing.xl)
            .semantics(mergeDescendants = true) { liveRegion = LiveRegionMode.Polite },
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(title, style = Popcorn.type.section, color = Popcorn.colors.text, textAlign = TextAlign.Center)
        Spacer(Modifier.height(Popcorn.spacing.xxs))
        Text(
            message,
            style = Popcorn.type.body,
            color = Popcorn.colors.textMuted,
            textAlign = TextAlign.Center,
            modifier = Modifier.widthIn(max = 360.dp),
        )
        Spacer(Modifier.height(Popcorn.spacing.m))
        SecondaryButton("Try again", onClick = onRetry)
    }
}
