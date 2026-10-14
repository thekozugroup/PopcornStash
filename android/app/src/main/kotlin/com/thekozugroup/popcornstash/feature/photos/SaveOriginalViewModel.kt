package com.thekozugroup.popcornstash.feature.photos

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.ApiException
import com.thekozugroup.popcornstash.feature.library.FileFetcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.BufferOverflow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.IOException
import java.io.OutputStream

/**
 * Saves a photo's original file where the person chose (the system's "Save to" picker), with
 * the session's own client: `GET /api/items/:id/file?download=1` streamed into the chosen
 * document. No other app or service is given the token (docs/architecture.md section 10).
 */
class SaveOriginalViewModel(
    private val api: ApiClient,
    private val files: FileFetcher,
    /** Opens the chosen document for writing; null when it cannot be written. */
    private val openOutput: (String) -> OutputStream?,
) : ViewModel() {
    /** The item whose original is being saved, or null. */
    private val _saving = MutableStateFlow<String?>(null)
    val saving: StateFlow<String?> = _saving.asStateFlow()

    private val _messages = MutableSharedFlow<String>(extraBufferCapacity = 4, onBufferOverflow = BufferOverflow.DROP_OLDEST)
    val messages: SharedFlow<String> = _messages.asSharedFlow()

    /** Copies the original of [itemId] into the document at [destination] (a content URI). */
    fun save(itemId: String, destination: String) {
        if (_saving.value != null) return
        _saving.value = itemId
        viewModelScope.launch {
            try {
                val out = withContext(Dispatchers.IO) { runCatching { openOutput(destination) }.getOrNull() }
                    ?: throw IOException("Cannot write there")
                out.use { files.copy(api.fileUrl(itemId, download = true), it) }
                _messages.tryEmit("Original saved.")
            } catch (e: ApiException) {
                _messages.tryEmit(if (e.isNotFound) "That photo is not available any more." else e.message)
            } catch (_: IOException) {
                _messages.tryEmit("The original could not be saved there. Choose another place.")
            } finally {
                _saving.value = null
            }
        }
    }
}
