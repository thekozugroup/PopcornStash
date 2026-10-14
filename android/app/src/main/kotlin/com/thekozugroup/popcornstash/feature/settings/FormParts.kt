package com.thekozugroup.popcornstash.feature.settings

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CheckboxDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.RadioButton
import androidx.compose.material3.RadioButtonDefaults
import androidx.compose.material3.Snackbar
import androidx.compose.material3.SnackbarData
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.DialogProperties
import com.thekozugroup.popcornstash.ui.theme.Popcorn
import com.thekozugroup.popcornstash.ui.theme.PopcornPalette

// The form parts of Settings and the library editor (android/DESIGN.md, Library editor):
// grouped sections, radio and check rows, field labels and errors, a destructive button,
// confirmations and the snackbar. Choices are drawn in the text colour, not Action Blue:
// a choice is not an action (the icon picker follows the same rule).

/** Disabled controls and their words fade to this. */
private const val DisabledAlpha = 0.45f

/**
 * One group of the form: a heading, the sentence that says what it decides, then its fields.
 * Groups after the first are parted by a rule.
 */
@Composable
fun FormSection(
    title: String,
    description: String?,
    modifier: Modifier = Modifier,
    first: Boolean = false,
    content: @Composable ColumnScope.() -> Unit,
) {
    Column(modifier.fillMaxWidth()) {
        if (!first) {
            Spacer(Modifier.height(Popcorn.spacing.l))
            Box(Modifier.fillMaxWidth().height(1.dp).background(Popcorn.colors.divider))
        }
        Spacer(Modifier.height(Popcorn.spacing.l))
        Text(title, style = Popcorn.type.section, color = Popcorn.colors.text, modifier = Modifier.semantics { heading() })
        if (description != null) {
            Spacer(Modifier.height(Popcorn.spacing.xxs))
            Text(description, style = Popcorn.type.metadata, color = Popcorn.colors.textMuted)
        }
        Spacer(Modifier.height(Popcorn.spacing.m))
        content()
    }
}

/** The name over a group of choices or a picker ("Arranged as", "Icon"). */
@Composable
fun FieldLabel(text: String, modifier: Modifier = Modifier) {
    Text(text, style = Popcorn.type.item, color = Popcorn.colors.text, modifier = modifier.padding(bottom = Popcorn.spacing.xxs))
}

/** A sentence under a field: the server's message when it names this field. */
@Composable
fun FieldError(message: String?, modifier: Modifier = Modifier) {
    if (message == null) return
    Text(
        message,
        style = Popcorn.type.metadata,
        color = Popcorn.colors.error,
        modifier = modifier
            .padding(top = Popcorn.spacing.xs)
            .semantics { liveRegion = LiveRegionMode.Polite },
    )
}

/** A quiet sentence that explains a choice ("Files stay in Holidays/. Nothing is moved."). */
@Composable
fun FieldNote(text: String, modifier: Modifier = Modifier, color: Color = Popcorn.colors.textMuted) {
    Text(text, style = Popcorn.type.metadata, color = color, modifier = modifier.padding(top = Popcorn.spacing.xs))
}

/** One radio choice, the whole row its target: the label, and a line under it when it needs one. */
@Composable
fun RadioRow(
    label: String,
    selected: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    description: String? = null,
    enabled: Boolean = true,
) {
    val colors = Popcorn.colors
    ChoiceRow(
        label = label,
        description = description,
        enabled = enabled,
        modifier = modifier.selectable(selected = selected, enabled = enabled, role = Role.RadioButton, onClick = onClick),
    ) {
        RadioButton(
            selected = selected,
            onClick = null,
            enabled = enabled,
            colors = RadioButtonDefaults.colors(
                selectedColor = colors.text,
                unselectedColor = colors.fieldOutline,
                disabledSelectedColor = colors.textMuted,
                disabledUnselectedColor = colors.fieldOutline.copy(alpha = DisabledAlpha),
            ),
        )
    }
}

/** One checkbox choice, the whole row its target. */
@Composable
fun CheckRow(
    label: String,
    checked: Boolean,
    onToggle: () -> Unit,
    modifier: Modifier = Modifier,
    description: String? = null,
    enabled: Boolean = true,
) {
    val colors = Popcorn.colors
    ChoiceRow(
        label = label,
        description = description,
        enabled = enabled,
        modifier = modifier.toggleable(value = checked, enabled = enabled, role = Role.Checkbox, onValueChange = { onToggle() }),
    ) {
        Checkbox(
            checked = checked,
            onCheckedChange = null,
            enabled = enabled,
            colors = CheckboxDefaults.colors(
                checkedColor = colors.text,
                uncheckedColor = colors.fieldOutline,
                checkmarkColor = colors.page,
                disabledCheckedColor = colors.textMuted,
                disabledUncheckedColor = colors.fieldOutline.copy(alpha = DisabledAlpha),
            ),
        )
    }
}

@Composable
private fun ChoiceRow(
    label: String,
    description: String?,
    enabled: Boolean,
    modifier: Modifier,
    control: @Composable () -> Unit,
) {
    val colors = Popcorn.colors
    Row(
        modifier
            .fillMaxWidth()
            .heightIn(min = if (description != null) 56.dp else 48.dp)
            .padding(vertical = Popcorn.spacing.xs),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.size(24.dp), contentAlignment = Alignment.Center) { control() }
        Spacer(Modifier.width(Popcorn.spacing.s))
        Column(Modifier.weight(1f).alpha(if (enabled) 1f else 0.7f)) {
            Text(label, style = Popcorn.type.item, color = if (enabled) colors.text else colors.textMuted)
            if (description != null) {
                Text(description, style = Popcorn.type.metadata, color = colors.textMuted)
            }
        }
    }
}

/** Text fields in the Popcorn colours: the zinc-500 edge, Action Blue only while focused. */
@Composable
fun stashFieldColors() = OutlinedTextFieldDefaults.colors(
    focusedBorderColor = Popcorn.colors.accent,
    unfocusedBorderColor = Popcorn.colors.fieldOutline,
    disabledBorderColor = Popcorn.colors.divider,
    focusedLabelColor = Popcorn.colors.text,
    unfocusedLabelColor = Popcorn.colors.textMuted,
    cursorColor = Popcorn.colors.text,
    focusedTextColor = Popcorn.colors.text,
    unfocusedTextColor = Popcorn.colors.text,
    disabledTextColor = Popcorn.colors.textMuted,
    focusedPlaceholderColor = Popcorn.colors.textMuted,
    unfocusedPlaceholderColor = Popcorn.colors.textMuted,
    focusedSupportingTextColor = Popcorn.colors.textMuted,
    unfocusedSupportingTextColor = Popcorn.colors.textMuted,
    focusedTrailingIconColor = Popcorn.colors.text,
    unfocusedTrailingIconColor = Popcorn.colors.textMuted,
    errorBorderColor = Popcorn.colors.error,
    errorLabelColor = Popcorn.colors.error,
    errorSupportingTextColor = Popcorn.colors.error,
    errorCursorColor = Popcorn.colors.error,
)

/** Folder names are shown as they are on the disk, in the monospace face. */
val FolderNameStyle: TextStyle
    @Composable get() = Popcorn.type.item.copy(fontFamily = FontFamily.Monospace)

/** An action that cannot be undone from here ("Delete library"): outlined, its words in the error colour. */
@Composable
fun DangerButton(text: String, onClick: () -> Unit, modifier: Modifier = Modifier, enabled: Boolean = true, busy: Boolean = false) {
    val colors = Popcorn.colors
    OutlinedButton(
        onClick = { if (!busy) onClick() },
        enabled = enabled,
        shape = Popcorn.shapes.control,
        border = BorderStroke(1.dp, colors.outline),
        colors = ButtonDefaults.outlinedButtonColors(
            contentColor = colors.error,
            disabledContentColor = colors.textMuted,
        ),
        contentPadding = PaddingValues(horizontal = 20.dp, vertical = 12.dp),
        modifier = modifier
            .defaultMinSize(minHeight = 48.dp)
            .semantics { if (busy) stateDescription = "Working" },
    ) {
        if (busy) {
            CircularProgressIndicator(color = colors.error, strokeWidth = 2.dp, modifier = Modifier.size(16.dp))
            Spacer(Modifier.width(10.dp))
        }
        Text(text, style = Popcorn.type.action)
    }
}

/**
 * A decision that must interrupt: what will happen in plain words, the action named by what it
 * does, and Cancel. [destructive] draws the action in the error colour.
 */
@Composable
fun ConfirmDialog(
    title: String,
    text: String,
    confirm: String,
    onConfirm: () -> Unit,
    onDismiss: () -> Unit,
    destructive: Boolean = false,
) {
    val colors = Popcorn.colors
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        text = { Text(text) },
        confirmButton = {
            TextButton(
                onClick = onConfirm,
                shape = Popcorn.shapes.control,
                colors = ButtonDefaults.textButtonColors(contentColor = if (destructive) colors.error else colors.text),
                modifier = Modifier.defaultMinSize(minHeight = 48.dp),
            ) { Text(confirm, style = Popcorn.type.action) }
        },
        dismissButton = {
            TextButton(
                onClick = onDismiss,
                shape = Popcorn.shapes.control,
                colors = ButtonDefaults.textButtonColors(contentColor = colors.text),
                modifier = Modifier.defaultMinSize(minHeight = 48.dp),
            ) { Text("Cancel", style = Popcorn.type.action) }
        },
        containerColor = colors.surface,
        titleContentColor = colors.text,
        textContentColor = colors.textMuted,
        shape = Popcorn.shapes.panel,
        // The dialog keeps 24dp from the screen's edges on every phone, whatever the window theme does.
        properties = DialogProperties(usePlatformDefaultWidth = false),
        modifier = Modifier.padding(horizontal = Popcorn.spacing.l),
    )
}

/** A status in words with the dot beside them ("Folder missing"); never colour alone. */
@Composable
fun StatusWords(text: String, modifier: Modifier = Modifier) {
    val colors = Popcorn.colors
    Row(modifier, verticalAlignment = Alignment.CenterVertically) {
        Box(Modifier.size(6.dp).background(colors.warningDot, Popcorn.shapes.avatar))
        Spacer(Modifier.width(6.dp))
        Text(text, style = Popcorn.type.metadata, color = colors.warning)
    }
}

/** A placeholder at the size of what is loading (android/DESIGN.md, Loading and errors). */
@Composable
fun PlaceholderBlock(width: Dp?, height: Dp, modifier: Modifier = Modifier, corner: Dp = 6.dp) {
    val sized = if (width != null) modifier.size(width, height) else modifier.fillMaxWidth().height(height)
    Box(sized.background(Popcorn.colors.subtle, RoundedCornerShape(corner)))
}

/** Snackbars in the inverse surface with 8dp corners, just above the floating toolbar. */
@Composable
fun PopcornSnackbar(data: SnackbarData) {
    val dark = Popcorn.colors.isDark
    Snackbar(
        snackbarData = data,
        shape = Popcorn.shapes.control,
        containerColor = if (dark) PopcornPalette.Zinc50 else PopcornPalette.Zinc900,
        contentColor = if (dark) PopcornPalette.Zinc900 else PopcornPalette.Zinc50,
        modifier = Modifier.padding(horizontal = Popcorn.spacing.m),
    )
}
