package com.thekozugroup.popcornstash.feature.downloads

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.ButtonDefaults
import com.thekozugroup.popcornstash.ui.components.PopcornMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExposedDropdownMenuAnchorType
import androidx.compose.material3.ExposedDropdownMenuBox
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MenuDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.thekozugroup.popcornstash.data.api.MediaKind
import com.thekozugroup.popcornstash.ui.components.PrimaryButton
import com.thekozugroup.popcornstash.ui.components.SecondaryButton
import com.thekozugroup.popcornstash.ui.icons.LibraryIcons
import com.thekozugroup.popcornstash.ui.icons.LucideGlyph
import com.thekozugroup.popcornstash.ui.icons.LucideIcon
import com.thekozugroup.popcornstash.ui.icons.PopcornIcons
import com.thekozugroup.popcornstash.ui.theme.Popcorn

/** The glyph for a target: the library's own icon, or Other files. */
fun ImportTarget.glyph(): LucideGlyph = library?.let { LibraryIcons.glyph(it.icon) } ?: PopcornIcons.OtherFiles

/**
 * One file ready to import (android/DESIGN.md, Downloads, Ready to import): the picture or
 * the file's glyph, the name, why it waits, the library picker, and Accept, Edit and More.
 * Only [first] shows Accept as the primary button, so the page keeps one primary action.
 */
@Composable
fun ImportRowView(
    row: ImportRow,
    first: Boolean,
    locked: Boolean,
    onChoose: (ImportTarget) -> Unit,
    onAccept: () -> Unit,
    onEdit: () -> Unit,
    onIgnore: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val item = row.item
    val facts = listOfNotNull(
        item.size.takeIf { it > 0 }?.let { formatBytes(it) },
        item.durationS?.takeIf { it > 0 }?.let { formatRuntime(it) },
    )
    Row(
        modifier
            .fillMaxWidth()
            .padding(start = Popcorn.spacing.gutter, end = Popcorn.spacing.xxs, top = Popcorn.spacing.m, bottom = Popcorn.spacing.m)
            .testTag("import ${item.id}"),
        verticalAlignment = Alignment.Top,
    ) {
        MediaThumb(item.thumb, glyphForKind(item.kind), size = THUMB)
        Spacer(Modifier.width(Popcorn.spacing.m))
        Column(Modifier.weight(1f)) {
            Text(
                breakableName(row.fileName),
                style = Popcorn.type.item,
                color = Popcorn.colors.text,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.padding(end = Popcorn.spacing.m),
            )
            if (row.reason != null) {
                Text(row.reason, style = Popcorn.type.metadata, color = Popcorn.colors.text, modifier = Modifier.padding(end = Popcorn.spacing.m))
            }
            if (facts.isNotEmpty()) FactsStrip(facts)
            Spacer(Modifier.height(Popcorn.spacing.s))
            LibraryPicker(
                choices = row.choices,
                selected = row.target,
                onChoose = onChoose,
                enabled = !locked,
                fileName = row.fileName,
                modifier = Modifier.padding(end = Popcorn.spacing.m),
            )
            Spacer(Modifier.height(Popcorn.spacing.xs))
            Row(verticalAlignment = Alignment.CenterVertically) {
                val accept = if (row.wantsDetails) "Add details" else "Accept"
                val canAccept = row.target != null && !locked
                if (first) {
                    PrimaryButton(accept, onClick = onAccept, enabled = canAccept || row.busy == RowBusy.Accept, busy = row.busy == RowBusy.Accept)
                } else {
                    TonalButton(accept, onClick = onAccept, enabled = canAccept, busy = row.busy == RowBusy.Accept)
                }
                Spacer(Modifier.width(Popcorn.spacing.xs))
                SecondaryButton("Edit", onClick = onEdit, enabled = !locked)
                Spacer(Modifier.weight(1f))
                if (!row.ignored) {
                    RowMoreMenu(
                        row.fileName,
                        listOf(MenuChoice("Ignore", onClick = onIgnore)),
                        enabled = !locked,
                    )
                }
            }
        }
    }
}

private val THUMB = 64.dp

/** "1 h 52 min", "42 min", "38 s". */
fun formatRuntime(seconds: Double): String {
    val total = seconds.toLong()
    val hours = total / 3600
    val minutes = (total % 3600) / 60
    return when {
        hours > 0 && minutes > 0 -> "$hours h $minutes min"
        hours > 0 -> "$hours h"
        minutes > 0 -> "$minutes min"
        else -> "$total s"
    }
}

/** A second choice on the rows after the first: `subtle` fill, text colour. */
@Composable
fun TonalButton(text: String, onClick: () -> Unit, enabled: Boolean = true, busy: Boolean = false) {
    FilledTonalButton(
        onClick = { if (!busy) onClick() },
        enabled = enabled || busy,
        shape = Popcorn.shapes.control,
        colors = ButtonDefaults.filledTonalButtonColors(
            containerColor = Popcorn.colors.subtle,
            contentColor = Popcorn.colors.text,
            disabledContainerColor = Popcorn.colors.subtle,
            disabledContentColor = Popcorn.colors.textMuted,
        ),
        contentPadding = PaddingValues(horizontal = 20.dp, vertical = 12.dp),
        modifier = Modifier.defaultMinSize(minHeight = 48.dp),
    ) {
        if (busy) {
            androidx.compose.material3.CircularProgressIndicator(
                color = Popcorn.colors.text,
                strokeWidth = 2.dp,
                modifier = Modifier.padding(end = 10.dp).width(16.dp).height(16.dp),
            )
        }
        Text(text, style = Popcorn.type.action)
    }
}

/**
 * Where the file goes: an exposed dropdown, 48dp, showing the library's icon and name, or
 * "Choose a library". It lists only [choices] (the libraries this person can see that fit
 * the file, then Other files). With one place to go it shows it, without a menu.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun LibraryPicker(
    choices: List<ImportTarget>,
    selected: ImportTarget?,
    onChoose: (ImportTarget) -> Unit,
    enabled: Boolean,
    fileName: String,
    modifier: Modifier = Modifier,
    error: Boolean = false,
) {
    var open by remember { mutableStateOf(false) }
    val fixed = choices.size <= 1
    val colors = Popcorn.colors
    val described = "Library for $fileName"
    ExposedDropdownMenuBox(
        expanded = open && !fixed,
        onExpandedChange = { if (enabled && !fixed) open = it },
        modifier = modifier,
    ) {
        Row(
            Modifier
                .menuAnchor(ExposedDropdownMenuAnchorType.PrimaryNotEditable, enabled = enabled && !fixed)
                .fillMaxWidth()
                .heightIn(min = 48.dp)
                .border(
                    BorderStroke(1.dp, if (error) colors.error else if (fixed) colors.divider else colors.fieldOutline),
                    Popcorn.shapes.control,
                )
                .padding(horizontal = Popcorn.spacing.s)
                .semantics(mergeDescendants = true) {
                    contentDescription = described
                    stateDescription = selected?.name ?: "No library chosen"
                    if (!fixed) role = Role.DropdownList
                }
                .testTag("picker $fileName"),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (selected != null) {
                LucideIcon(selected.glyph(), contentDescription = null, tint = colors.textMuted, size = 20.dp)
                Spacer(Modifier.width(Popcorn.spacing.s))
            }
            Text(
                selected?.name ?: "Choose a library",
                style = Popcorn.type.item,
                color = if (selected != null) colors.text else colors.textMuted,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            if (!fixed) {
                LucideIcon(
                    PopcornIcons.Expand,
                    contentDescription = null,
                    tint = colors.textMuted,
                    size = 20.dp,
                    modifier = Modifier.rotate(if (open) 180f else 0f),
                )
            }
        }
        if (!fixed) {
            ExposedDropdownMenu(
                expanded = open,
                onDismissRequest = { open = false },
                containerColor = colors.surface,
                shape = Popcorn.shapes.panel,
                shadowElevation = 8.dp,
                border = BorderStroke(1.dp, colors.divider),
            ) {
                choices.forEachIndexed { index, choice ->
                    if (choice.library == null && index > 0) HorizontalDivider(color = colors.divider)
                    val chosen = choice.key == selected?.key
                    PopcornMenuItem(
                        text = { Text(choice.name, style = Popcorn.type.item) },
                        leadingIcon = { LucideIcon(choice.glyph(), contentDescription = null, tint = colors.textMuted, size = 20.dp) },
                        trailingIcon = if (chosen) {
                            { LucideIcon(PopcornIcons.Chosen, contentDescription = "Chosen", tint = colors.text, size = 20.dp) }
                        } else {
                            null
                        },
                        onClick = {
                            open = false
                            onChoose(choice)
                        },
                        colors = MenuDefaults.itemColors(textColor = colors.text),
                        modifier = Modifier.heightIn(min = 48.dp),
                    )
                }
            }
        }
    }
}

/** "Left where they are (3)": the closed disclosure over the files someone ignored. */
@Composable
fun IgnoredDisclosure(count: Int, open: Boolean, onToggle: () -> Unit, modifier: Modifier = Modifier) {
    Row(
        modifier
            .fillMaxWidth()
            .heightIn(min = 56.dp)
            .clickable(onClickLabel = if (open) "Hide them" else "Show them", role = Role.Button, onClick = onToggle)
            .padding(horizontal = Popcorn.spacing.gutter)
            .semantics(mergeDescendants = true) { stateDescription = if (open) "Shown" else "Hidden" },
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.Start,
    ) {
        LucideIcon(
            PopcornIcons.Open,
            contentDescription = null,
            tint = Popcorn.colors.textMuted,
            size = 20.dp,
            modifier = Modifier.rotate(if (open) 90f else 0f),
        )
        Spacer(Modifier.width(Popcorn.spacing.s))
        Text("Left where they are ($count)", style = Popcorn.type.section, color = Popcorn.colors.text)
    }
}

/** A file's kind in words, for the edit sheet's heading ("Video", "Photo"). */
fun kindWord(kind: MediaKind): String = when (kind) {
    MediaKind.Video -> "Video"
    MediaKind.Photo -> "Photo"
    MediaKind.Audio -> "Song"
    else -> "File"
}
