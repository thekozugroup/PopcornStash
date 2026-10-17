package com.thekozugroup.popcornstash.feature.settings

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CheckboxDefaults
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import com.thekozugroup.popcornstash.data.api.Library
import com.thekozugroup.popcornstash.ui.components.ConfirmDialog
import com.thekozugroup.popcornstash.ui.theme.Popcorn

/** Files stay by default. The folder choice needs its exact name and is recoverable. */
@Composable
fun LibraryRemovalDialog(library: Library, onConfirm: (Boolean) -> Unit, onDismiss: () -> Unit) {
    var removeFolder by remember(library.id) { mutableStateOf(false) }
    var confirmation by remember(library.id) { mutableStateOf("") }
    ConfirmDialog(
        title = "Delete ${library.name}?",
        confirm = if (removeFolder) "Delete library and folder" else "Delete library",
        onConfirm = { onConfirm(removeFolder) },
        onDismiss = onDismiss,
        destructive = true,
        confirmEnabled = !removeFolder || confirmation == library.folder,
    ) {
        Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(Popcorn.spacing.m)) {
            Text(if (removeFolder) "Takes ${library.name} out of the app." else deleteSentence(library))
            Row(
                Modifier.fillMaxWidth().defaultMinSize(minHeight = 48.dp)
                    .toggleable(removeFolder, enabled = library.folder != null, role = Role.Checkbox, onValueChange = { removeFolder = it; confirmation = "" }),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(Popcorn.spacing.s),
            ) {
                Checkbox(removeFolder, onCheckedChange = null, colors = CheckboxDefaults.colors(checkedColor = Popcorn.colors.accent))
                Text("Also remove the folder and all its files", color = Popcorn.colors.text)
            }
            if (removeFolder) {
                Text("Moves ${library.folder}/ to Trash, including files the library does not show. Owners can restore it from Sorting and trash on the web.")
                OutlinedTextField(
                    value = confirmation, onValueChange = { confirmation = it }, label = { Text("Type ${library.folder} to confirm") },
                    singleLine = true, modifier = Modifier.fillMaxWidth(), shape = Popcorn.shapes.control, colors = stashFieldColors(),
                )
            }
        }
    }
}
