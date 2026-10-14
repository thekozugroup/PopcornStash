package com.thekozugroup.popcornstash.ui.components

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import com.thekozugroup.popcornstash.ui.theme.Popcorn

/**
 * A row on the page (android/DESIGN.md, Rows): a leading glyph or picture, a name, one
 * metadata line, trailing actions. At least 56dp tall; the whole row is one target when it
 * opens something ([onClick]).
 */
@Composable
fun ListRow(
    title: String,
    modifier: Modifier = Modifier,
    supporting: String? = null,
    titleColor: Color = Popcorn.colors.text,
    onClick: (() -> Unit)? = null,
    onClickLabel: String? = null,
    leading: (@Composable () -> Unit)? = null,
    trailing: (@Composable () -> Unit)? = null,
) {
    val clickable = if (onClick != null) {
        Modifier.clickable(onClickLabel = onClickLabel, role = Role.Button, onClick = onClick)
    } else {
        Modifier
    }
    Row(
        modifier
            .fillMaxWidth()
            .heightIn(min = 56.dp)
            .then(clickable)
            .padding(horizontal = Popcorn.spacing.gutter, vertical = Popcorn.spacing.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (leading != null) {
            leading()
            Spacer(Modifier.width(Popcorn.spacing.m))
        }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.Center) {
            Text(title, style = Popcorn.type.item, color = titleColor)
            if (supporting != null) {
                Text(supporting, style = Popcorn.type.metadata, color = Popcorn.colors.textMuted)
            }
        }
        if (trailing != null) {
            Spacer(Modifier.width(Popcorn.spacing.s))
            trailing()
        }
    }
}

/** The 1dp rule between rows, starting where the row's text starts when it has a leading glyph. */
@Composable
fun RowDivider(inset: Boolean = true) {
    HorizontalDivider(
        thickness = 1.dp,
        color = Popcorn.colors.divider,
        modifier = Modifier.padding(start = if (inset) Popcorn.spacing.gutter + 24.dp + Popcorn.spacing.m else 0.dp),
    )
}

/** A heading over a group of rows. */
@Composable
fun SectionHeading(text: String, modifier: Modifier = Modifier) {
    Text(
        text,
        style = Popcorn.type.section,
        color = Popcorn.colors.text,
        modifier = modifier
            .fillMaxWidth()
            .padding(start = Popcorn.spacing.gutter, end = Popcorn.spacing.gutter, top = Popcorn.spacing.xl, bottom = Popcorn.spacing.xs)
            .semantics { heading() },
    )
}
