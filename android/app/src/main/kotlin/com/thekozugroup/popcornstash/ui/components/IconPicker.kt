package com.thekozugroup.popcornstash.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
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
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.CollectionInfo
import androidx.compose.ui.semantics.CollectionItemInfo
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.collectionInfo
import androidx.compose.ui.semantics.collectionItemInfo
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.max
import com.thekozugroup.popcornstash.ui.icons.LibraryIcons
import com.thekozugroup.popcornstash.ui.icons.LucideIcon
import com.thekozugroup.popcornstash.ui.theme.Popcorn

/** Sizes of the icon picker (android/DESIGN.md, Icon picker). */
object IconPickerTokens {
    /** Cells are square and never narrower than this; they grow to fill the row. */
    val minCell: Dp = 48.dp
    val gap: Dp = 8.dp
    val glyph: Dp = 24.dp
    val ring: Dp = 1.5.dp

    /** How many cells fit in a row [width] wide, and how wide each is. */
    fun layout(width: Dp): Pair<Int, Dp> {
        val columns = maxOf(1, ((width + gap) / (minCell + gap)).toInt())
        return columns to max(minCell, (width - gap * (columns - 1)) / columns)
    }
}

/**
 * The library icon picker: the server's curated list ([icons], in its order) as a grid of
 * square cells that fill the width, at least 48dp each with 8dp between them (six across on a
 * phone). The chosen cell gets the `subtle` fill, a 1.5dp `text` ring and the glyph in `text`;
 * neutral, not Action Blue, because it is a choice and not an action. One selectable group;
 * each cell is a radio button named by the icon's plain name, with that name as a tooltip on a
 * long press. A name the app does not know draws the folder glyph.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun IconPicker(
    icons: List<String>,
    selected: String,
    onSelect: (String) -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
) {
    val colors = Popcorn.colors
    val gap = IconPickerTokens.gap
    BoxWithConstraints(modifier.fillMaxWidth()) {
        val (columns, cell) = IconPickerTokens.layout(maxWidth)
        val rows = icons.chunked(columns)
        Column(
            verticalArrangement = Arrangement.spacedBy(gap),
            modifier = Modifier
                .testTag("icon picker")
                .selectableGroup()
                .semantics { collectionInfo = CollectionInfo(rowCount = rows.size, columnCount = columns) }
                .alpha(if (enabled) 1f else 0.45f),
        ) {
            rows.forEachIndexed { rowIndex, row ->
                Row(horizontalArrangement = Arrangement.spacedBy(gap)) {
                    row.forEachIndexed { columnIndex, name ->
                        val entry = LibraryIcons.forName(name)
                        val chosen = name == selected
                        TooltipBox(
                            positionProvider = TooltipDefaults.rememberTooltipPositionProvider(TooltipAnchorPosition.Above),
                            tooltip = { PlainTooltip { Text(entry.label) } },
                            state = rememberTooltipState(),
                        ) {
                            Box(
                                Modifier
                                    .testTag("icon $name")
                                    .size(cell)
                                    .clip(Popcorn.shapes.control)
                                    .background(if (chosen) colors.subtle else Color.Transparent)
                                    .border(IconPickerTokens.ring, if (chosen) colors.text else Color.Transparent, Popcorn.shapes.control)
                                    .selectable(selected = chosen, enabled = enabled, role = Role.RadioButton) { onSelect(name) }
                                    .semantics {
                                        contentDescription = entry.label
                                        collectionItemInfo = CollectionItemInfo(rowIndex, 1, columnIndex, 1)
                                    },
                                contentAlignment = Alignment.Center,
                            ) {
                                LucideIcon(
                                    entry.glyph,
                                    contentDescription = null,
                                    tint = if (chosen) colors.text else colors.textMuted,
                                    size = IconPickerTokens.glyph,
                                )
                            }
                        }
                    }
                }
            }
        }
    }
}
