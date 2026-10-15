package com.thekozugroup.popcornstash.ui.components

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExposedDropdownMenuAnchorType
import androidx.compose.material3.ExposedDropdownMenuBox
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.thekozugroup.popcornstash.data.api.Library
import com.thekozugroup.popcornstash.ui.icons.LibraryIcons
import com.thekozugroup.popcornstash.ui.icons.LucideIcon
import com.thekozugroup.popcornstash.ui.icons.PopcornIcons
import com.thekozugroup.popcornstash.ui.theme.Popcorn

/** Uses the same field treatment and 48dp targets as the Inbox library picker. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DownloadDestinationPicker(libraries: List<Library>, selected: String?, onChoose: (String?) -> Unit, enabled: Boolean = true) {
    var open by remember { mutableStateOf(false) }
    val library = libraries.find { it.id == selected }
    val name = if (selected == null) "Sorted automatically" else library?.name ?: "Choose another library"
    val colors = Popcorn.colors
    Column(verticalArrangement = Arrangement.spacedBy(Popcorn.spacing.xs)) {
        Text("Where it goes", style = Popcorn.type.metadata, color = colors.textMuted)
        ExposedDropdownMenuBox(expanded = open && enabled, onExpandedChange = { if (enabled) open = it }) {
            Row(
                Modifier.menuAnchor(ExposedDropdownMenuAnchorType.PrimaryNotEditable, enabled = enabled)
                    .fillMaxWidth().heightIn(min = 48.dp)
                    .border(BorderStroke(1.dp, colors.fieldOutline), Popcorn.shapes.control)
                    .padding(horizontal = Popcorn.spacing.s)
                    .semantics(mergeDescendants = true) {
                        contentDescription = "Where it goes"
                        stateDescription = name
                        role = Role.DropdownList
                    },
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(Popcorn.spacing.s),
            ) {
                if (library != null) LucideIcon(LibraryIcons.glyph(library.icon), contentDescription = null, tint = colors.textMuted, size = 20.dp)
                Text(name, style = Popcorn.type.item, color = if (enabled) colors.text else colors.textMuted,
                    maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
                LucideIcon(PopcornIcons.Expand, contentDescription = null, tint = colors.textMuted, size = 20.dp)
            }
            ExposedDropdownMenu(expanded = open && enabled, onDismissRequest = { open = false },
                containerColor = colors.surface, shape = Popcorn.shapes.control,
                shadowElevation = 8.dp, border = BorderStroke(1.dp, colors.divider),
                modifier = Modifier.heightIn(max = 320.dp)) {
                DropdownMenuItem(text = { Text("Sorted automatically") }, onClick = { open = false; onChoose(null) })
                libraries.forEach { item ->
                    DropdownMenuItem(text = { Text(item.name) }, onClick = { open = false; onChoose(item.id) },
                        leadingIcon = { LucideIcon(LibraryIcons.glyph(item.icon), contentDescription = null) })
                }
            }
        }
    }
}
