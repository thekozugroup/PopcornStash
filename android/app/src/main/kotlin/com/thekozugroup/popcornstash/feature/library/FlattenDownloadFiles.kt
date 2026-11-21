package com.thekozugroup.popcornstash.feature.library

import androidx.compose.foundation.layout.*
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import com.thekozugroup.popcornstash.data.api.*
import com.thekozugroup.popcornstash.ui.components.*
import com.thekozugroup.popcornstash.ui.theme.Popcorn
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch

/** Opening the preview never moves files. Only the explicit Move files action does. */
@Composable
internal fun FlattenDownloadFiles(id: String, groupKey: String, api: ApiClient, onMoved: () -> Unit, onBusy: (Boolean) -> Unit, onClose: () -> Unit) {
    var preview by remember(id, groupKey) { mutableStateOf<FlattenPreview?>(null) }
    var busy by remember { mutableStateOf(true) }
    var error by remember(id, groupKey) { mutableStateOf<String?>(null) }
    var result by remember(id, groupKey) { mutableStateOf<String?>(null) }
    var expanded by remember(id, groupKey) { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    fun message(e: Exception): String {
        if (e is CancellationException) throw e
        return (e as? ApiException)?.message ?: "PopcornStash did not answer. Try again."
    }
    fun working(value: Boolean) { busy = value; onBusy(value) }
    var attempt by remember { mutableIntStateOf(0) }
    LaunchedEffect(id, groupKey, attempt) {
        busy = true; error = null; result = null; preview = null; expanded = false
        try { preview = api.previewFlatten(id, groupKey) }
        catch (e: Exception) { error = message(e) }
        finally { busy = false }
    }
    fun load() { attempt++ }
    fun move(plan: FlattenPreview) {
        working(true); error = null
        scope.launch {
            try {
                val value = api.flattenDownload(id, groupKey, plan.token)
                preview = null
                result = "${value.completed.size} ${if (value.completed.size == 1) "video" else "videos"} organized."
                if (value.failed.isNotEmpty()) error = value.failed.joinToString(" ") { it.message }
                onMoved()
            } catch (e: Exception) { preview = null; error = message(e) }
            finally { working(false) }
        }
    }
    Column(verticalArrangement = Arrangement.spacedBy(Popcorn.spacing.s)) {
        val plan = preview
        if (plan == null) {
            if (busy) Text("Preparing preview…", style = Popcorn.type.metadata, color = Popcorn.colors.textMuted)
            Row(horizontalArrangement = Arrangement.spacedBy(Popcorn.spacing.s)) {
                if (error != null && result == null) QuietButton("Retry preview", ::load, enabled = !busy)
                QuietButton(if (result != null) "Done" else "Cancel", onClose, enabled = !busy)
            }
        } else {
            val moves = plan.files.filter { it.from != it.to }
            Text("Move files into this download folder", style = Popcorn.type.section)
            Text("Destination: ${plan.folder}", style = Popcorn.type.metadata, color = Popcorn.colors.textMuted)
            Text("Videos and matching sidecars move here, with filenames unchanged. Other files and folders stay in place. Nothing is overwritten.", style = Popcorn.type.metadata, color = Popcorn.colors.textMuted)
            Text(if (moves.isEmpty()) (if (plan.blocked.isEmpty()) "Files are already in this folder." else "No files are ready to move.") else "${moves.size} ${if (moves.size == 1) "file" else "files"} to move.", style = Popcorn.type.body)
            plan.blocked.forEach { Text(it, color = Popcorn.colors.error, style = Popcorn.type.body) }
            (if (expanded) moves else moves.take(6)).forEach { file ->
                Column {
                    Text("From: ${file.from}", style = Popcorn.type.metadata, color = Popcorn.colors.textMuted)
                    Text("To: ${file.to}", style = Popcorn.type.metadata)
                }
            }
            if (!expanded && moves.size > 6) QuietButton("Show all ${moves.size} moves", { expanded = true }, enabled = !busy)
            PrimaryButton("Move files", { move(plan) }, enabled = !busy && plan.blocked.isEmpty() && moves.isNotEmpty(), busy = busy)
            Row(horizontalArrangement = Arrangement.spacedBy(Popcorn.spacing.s)) {
                QuietButton("Refresh preview", ::load, enabled = !busy)
                QuietButton("Cancel", onClose, enabled = !busy)
            }
        }
        result?.let { Text(it, style = Popcorn.type.body) }
        error?.let { Text(it, style = Popcorn.type.body, color = Popcorn.colors.error) }
    }
}
