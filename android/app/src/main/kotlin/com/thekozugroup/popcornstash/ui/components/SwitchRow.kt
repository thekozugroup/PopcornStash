package com.thekozugroup.popcornstash.ui.components

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.selection.toggleable
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import com.thekozugroup.popcornstash.ui.theme.Popcorn

/**
 * A setting that is on or off (android/DESIGN.md, Switches): the label in `item`, one line under
 * it in `metadata`, and Material's switch at the end. The whole row is the target. On is the
 * Action Blue track with a white knob; off is a `subtle` track inside a 2dp `fieldOutline` ring
 * with a knob in the ring's colour (`textMuted` in dark, lighter than its track), so off reads on
 * its own, at 3:1 or more, without the blue to compare with.
 */
@Composable
fun SwitchRow(
    label: String,
    checked: Boolean,
    onCheckedChange: (Boolean) -> Unit,
    modifier: Modifier = Modifier,
    supporting: String? = null,
    enabled: Boolean = true,
) {
    val colors = Popcorn.colors
    val offKnob = if (colors.isDark) colors.textMuted else colors.fieldOutline
    Row(
        modifier
            .fillMaxWidth()
            .heightIn(min = 56.dp)
            .toggleable(value = checked, enabled = enabled, role = Role.Switch, onValueChange = onCheckedChange)
            .padding(horizontal = Popcorn.spacing.gutter, vertical = Popcorn.spacing.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(label, style = Popcorn.type.item, color = if (enabled) colors.text else colors.textMuted)
            if (supporting != null) Text(supporting, style = Popcorn.type.metadata, color = colors.textMuted)
        }
        Spacer(Modifier.width(Popcorn.spacing.m))
        Switch(
            checked = checked,
            // The row takes the tap, so TalkBack hears one switch with its label.
            onCheckedChange = null,
            enabled = enabled,
            colors = SwitchDefaults.colors(
                checkedTrackColor = colors.accent,
                checkedThumbColor = colors.onAccent,
                checkedBorderColor = colors.accent,
                checkedIconColor = colors.accent,
                uncheckedTrackColor = colors.subtle,
                uncheckedThumbColor = offKnob,
                uncheckedBorderColor = colors.fieldOutline,
                uncheckedIconColor = colors.subtle,
            ),
        )
    }
}
