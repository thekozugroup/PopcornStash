package com.thekozugroup.popcornstash.feature.settings

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import com.thekozugroup.popcornstash.data.api.*
import com.thekozugroup.popcornstash.ui.components.PageTopBar
import com.thekozugroup.popcornstash.ui.components.PrimaryButton
import com.thekozugroup.popcornstash.ui.components.SecondaryButton
import com.thekozugroup.popcornstash.ui.theme.Popcorn
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

internal data class ConversionUi(val view: LibraryConversion? = null, val draft: ConversionPolicy? = null, val busy: Boolean = false, val error: String? = null, val notice: String? = null)
internal class ConversionModel(private val api: ApiClient, private val id: String) : ViewModel() {
    val state = MutableStateFlow(ConversionUi())
    suspend fun refresh() {
        try { val view = api.libraryConversion(id); state.update { it.copy(view = view, draft = it.draft ?: view.policy, error = null) } }
        catch (_: ApiException) { state.update { it.copy(error = "Could not load conversion settings. Check your connection and try again.") } }
    }
    fun change(policy: ConversionPolicy) { state.update { it.copy(draft = policy, notice = null) } }
    fun save(retry: Boolean = false) {
        val policy = state.value.draft ?: return
        if (state.value.busy) return
        state.update { it.copy(busy = true, error = null, notice = null) }
        viewModelScope.launch {
            try {
                val next = if (retry) api.retryLibraryConversion(id) else api.saveLibraryConversion(id, policy)
                state.update { it.copy(view = next, draft = if (retry) it.draft else next.policy, notice = if (retry) "Stopped conversions will be checked again." else "Changes saved.") }
            } catch (_: ApiException) { state.update { it.copy(error = "Could not save. Your changes are still here; try again.") } }
            finally { state.update { it.copy(busy = false) } }
        }
    }
}

@Composable
internal fun LibraryConversionRoute(api: ApiClient, id: String, onBack: () -> Unit, bottomClearance: Dp) {
    val model: ConversionModel = viewModel(key = "conversion $id", factory = viewModelFactory { initializer { ConversionModel(api, id) } })
    val state by model.state.collectAsStateWithLifecycle()
    LaunchedEffect(model) { while (true) { model.refresh(); delay(5000) } }
    LibraryConversionPage(state, model::change, { model.save() }, { model.save(true) }, onBack, bottomClearance)
}

@Composable
internal fun LibraryConversionPage(state: ConversionUi, onChange: (ConversionPolicy) -> Unit, onSave: () -> Unit, onRetry: () -> Unit, onBack: () -> Unit, bottomClearance: Dp = 24.dp) {
    Column(Modifier.fillMaxSize().background(Popcorn.colors.page)) {
        PageTopBar("Video conversion", onBack = onBack)
        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = Popcorn.spacing.gutter).padding(bottom = bottomClearance)) {
            state.error?.let { Text(it, color = Popcorn.colors.error, modifier = Modifier.padding(vertical = 16.dp)) }
            val p = state.draft
            val view = state.view
            if (p == null || view == null) { Text("Loading conversion settings…", modifier = Modifier.padding(vertical = 24.dp)); return@Column }
            FormSection("Automatic conversion", "Applies to existing videos and new imports after downloads finish.", first = true) {
                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                    Text("Convert this library automatically", style = Popcorn.type.body, modifier = Modifier.weight(1f))
                    Switch(modifier = Modifier.semantics { contentDescription = "Convert this library automatically" }, checked = p.enabled, onCheckedChange = { onChange(p.copy(enabled = it)) }, enabled = !state.busy)
                }
                Text("Enabling this allows originals to be replaced. Only smaller files that pass full-video, frame, audio and subtitle checks are kept.", style = Popcorn.type.metadata, color = Popcorn.colors.textMuted)
                Text("Titles, folders, favourites and watch progress stay with the video. HDR and rotated videos are kept unchanged.", style = Popcorn.type.metadata, color = Popcorn.colors.textMuted)
            }
            FormSection("Format and quality", "Conversion is lossy. Higher quality keeps more detail; a replacement must also save space.") {
                ConversionChoices("Video format", listOf("av1" to "AV1", "hevc" to "H.265 (HEVC)"), p.codec, !state.busy) { onChange(p.copy(codec = it)) }
                ConversionChoices("Quality", listOf("high" to "Higher quality", "balanced" to "Balanced", "small" to "Smaller files"), p.quality, !state.busy) { onChange(p.copy(quality = it)) }
            }
            FormSection("Processing", "One video at a time on this server. Automatic prefers a tested GPU, with CPU fallback when no compatible GPU is available.") {
                ConversionChoices("Use", listOf("auto" to "Automatic", "hardware" to "GPU only", "cpu" to "CPU only"), p.compute, !state.busy) { onChange(p.copy(compute = it)) }
                val encoder = view.encoders.firstOrNull { it.codec == p.codec && (p.compute == "auto" || it.hardware == (p.compute == "hardware")) }
                Text(encoder?.let { "Available: ${it.label}" } ?: "No working encoder for this choice. Originals will be kept.", style = Popcorn.type.metadata, color = Popcorn.colors.textMuted)
                Text("Turning conversion off stops the current conversion safely. Completed replacements remain.", style = Popcorn.type.metadata, color = Popcorn.colors.textMuted)
            }
            Spacer(Modifier.height(16.dp))
            PrimaryButton(if (state.busy) "Saving…" else "Save changes", onClick = onSave, enabled = !state.busy)
            state.notice?.let { Text(it, modifier = Modifier.padding(vertical = 16.dp)) }
            FormSection("Activity", if (view.policy.enabled) "${view.pending} videos waiting to be checked." else "Automatic conversion is off.") {
                if (view.jobs.isEmpty()) Text("No conversions yet.", color = Popcorn.colors.textMuted)
                view.jobs.forEach { job ->
                    Column(Modifier.fillMaxWidth().padding(vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                        Text(job.title, style = Popcorn.type.body)
                        val label = when (job.state) { "encoding" -> "Converting · ${(job.progress * 100).toInt()}%"; "verifying" -> "Checking video"; "publishing" -> "Replacing original"; "complete" -> "Complete"; "skipped" -> "Original kept"; "failed" -> "Needs attention"; else -> "Stopped" }
                        Text(label + if (job.savedBytes > 0) " · ${com.thekozugroup.popcornstash.feature.downloads.formatBytes(job.savedBytes)} saved" else "", style = Popcorn.type.metadata, color = Popcorn.colors.textMuted)
                        job.encoder?.let { Text(it, style = Popcorn.type.metadata, color = Popcorn.colors.textMuted) }
                        job.message?.let { Text(it, style = Popcorn.type.metadata, color = Popcorn.colors.textMuted) }
                    }
                    HorizontalDivider(color = Popcorn.colors.divider)
                }
                if (view.jobs.any { it.state == "failed" || it.state == "cancelled" }) SecondaryButton("Retry stopped conversions", onClick = onRetry, enabled = !state.busy && view.policy.enabled)
            }
        }
    }
}

@Composable
private fun ConversionChoices(label: String, options: List<Pair<String, String>>, value: String, enabled: Boolean, onChange: (String) -> Unit) {
    Column(Modifier.fillMaxWidth().selectableGroup()) {
        Text(label, style = Popcorn.type.action, modifier = Modifier.semantics { heading() })
        options.forEach { (key, title) ->
            RadioRow(title, selected = value == key, onClick = { onChange(key) }, enabled = enabled)
        }
    }
}
