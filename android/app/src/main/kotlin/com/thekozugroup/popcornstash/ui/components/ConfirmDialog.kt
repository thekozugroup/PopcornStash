package com.thekozugroup.popcornstash.ui.components

import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.DialogProperties
import com.thekozugroup.popcornstash.ui.theme.Popcorn

/**
 * Every confirmation in the app (android/DESIGN.md, Choices and confirmations): a title that asks,
 * what happens, Cancel and the action as quiet 48dp buttons, the action in the error colour when it
 * removes or deletes something. The panel shape on the surface colour, 24dp from the screen's edges
 * on every phone. While the action runs ([enabled] false) both buttons wait.
 */
@Composable
fun ConfirmDialog(
    title: String,
    confirm: String,
    onConfirm: () -> Unit,
    onDismiss: () -> Unit,
    destructive: Boolean = false,
    enabled: Boolean = true,
    confirmEnabled: Boolean = enabled,
    text: @Composable () -> Unit,
) {
    val colors = Popcorn.colors
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        text = text,
        confirmButton = {
            TextButton(
                onClick = onConfirm,
                enabled = confirmEnabled,
                shape = Popcorn.shapes.control,
                colors = ButtonDefaults.textButtonColors(
                    contentColor = if (destructive) colors.error else colors.text,
                    disabledContentColor = colors.textMuted,
                ),
                modifier = Modifier.defaultMinSize(minHeight = 48.dp),
            ) { Text(confirm, style = Popcorn.type.action) }
        },
        dismissButton = {
            TextButton(
                onClick = onDismiss,
                enabled = enabled,
                shape = Popcorn.shapes.control,
                colors = ButtonDefaults.textButtonColors(contentColor = colors.text, disabledContentColor = colors.textMuted),
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

/** A [ConfirmDialog] whose body is one sentence. */
@Composable
fun ConfirmDialog(
    title: String,
    text: String,
    confirm: String,
    onConfirm: () -> Unit,
    onDismiss: () -> Unit,
    destructive: Boolean = false,
    enabled: Boolean = true,
) = ConfirmDialog(title, confirm, onConfirm, onDismiss, destructive, enabled) { Text(text) }
