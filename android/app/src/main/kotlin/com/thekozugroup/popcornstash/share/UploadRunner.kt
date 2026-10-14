package com.thekozugroup.popcornstash.share

import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.ApiException
import kotlinx.coroutines.CancellationException

/** How a batch ended. */
sealed interface UploadOutcome {
    /** Every file arrived; the server sorts them on its own. */
    data class Done(val count: Int) : UploadOutcome

    /** Nobody, or someone else, is signed in now: nothing was sent and the batch is dropped. */
    data object NotSignedIn : UploadOutcome

    /** The batch was stopped (by the person, or by signing out). */
    data object Stopped : UploadOutcome

    /** The sharing app's read grant is gone: "Share the file again to finish uploading." */
    data class SourceGone(val name: String) : UploadOutcome

    /** The server refused a file, with its sentence. */
    data class Failed(val message: String) : UploadOutcome

    /** The connection dropped: try the whole batch again later; done files are skipped. */
    data class RetryLater(val message: String) : UploadOutcome
}

/**
 * Uploads one confirmed batch, file by file, keeping each file's upload id and whether it is
 * done in [store], so a job that is stopped and started again continues where it was.
 *
 * [apiFor] answers the API of the signed-in session only when it is the person who confirmed
 * the batch, on the same server; otherwise nothing is sent. The batch's file in [store] is
 * also the switch that keeps it going: once it is gone (signed out, or stopped), no further
 * request is made.
 */
class UploadRunner(
    private val store: UploadBatchStore,
    private val apiFor: suspend (UploadBatch) -> ApiClient?,
    private val open: (SharedFile) -> ChunkSource,
    private val measure: (SharedFile) -> Long,
    private val uploader: (ApiClient) -> Uploader = { Uploader(it) },
) {
    suspend fun run(batchId: String, onProgress: suspend (UploadBatchProgress) -> Unit): UploadOutcome {
        val first = store.load(batchId) ?: return UploadOutcome.Stopped
        val api = apiFor(first) ?: run {
            store.delete(batchId)
            return UploadOutcome.NotSignedIn
        }
        val sizes = first.files.map { it.size }.toMutableList()
        val sent = LongArray(first.files.size) { index -> if (first.files[index].done) sizes[index] ?: 0 else 0 }
        suspend fun report(batch: UploadBatch) {
            val total = if (sizes.all { it != null }) sizes.sumOf { it ?: 0 } else null
            onProgress(
                UploadBatchProgress(
                    id = batch.id,
                    names = batch.files.map { it.name },
                    doneFiles = batch.files.count { it.done },
                    sentBytes = sent.sum(),
                    totalBytes = total,
                    waiting = false,
                ),
            )
        }
        report(first)
        for (index in first.files.indices) {
            val batch = store.load(batchId) ?: return UploadOutcome.Stopped
            val file = batch.files[index]
            if (file.done) continue
            val source = try {
                open(file)
            } catch (_: SourceUnreadableException) {
                return gone(api, batchId, index, file)
            }
            try {
                val size = sizes[index] ?: measure(file).also { sizes[index] = it }
                uploader(api).upload(
                    file = file,
                    size = size,
                    source = source,
                    onCreated = { id -> store.update(batchId, index) { it.copy(uploadId = id, size = size) } },
                    onSent = { bytes -> sent[index] = bytes },
                    stillWanted = { store.load(batchId) != null },
                )
                sent[index] = size
                val updated = store.update(batchId, index) { it.copy(done = true) } ?: return UploadOutcome.Stopped
                report(updated)
            } catch (e: CancellationException) {
                throw e
            } catch (_: UploadStoppedException) {
                return UploadOutcome.Stopped
            } catch (_: SourceUnreadableException) {
                return gone(api, batchId, index, file)
            } catch (e: ApiException) {
                if (e.isNetwork || e.status in RETRY_STATUSES) return UploadOutcome.RetryLater(e.message)
                drop(api, batchId, index)
                return UploadOutcome.Failed(e.message)
            } finally {
                runCatching { source.close() }
            }
        }
        val finished = store.load(batchId) ?: return UploadOutcome.Stopped
        store.delete(batchId)
        return UploadOutcome.Done(finished.files.size)
    }

    private suspend fun gone(api: ApiClient, batchId: String, index: Int, file: SharedFile): UploadOutcome {
        drop(api, batchId, index)
        return UploadOutcome.SourceGone(file.name)
    }

    /** Ends the batch: the server deletes the part of the file it already has. */
    private suspend fun drop(api: ApiClient, batchId: String, index: Int) {
        val uploadId = store.load(batchId)?.files?.getOrNull(index)?.uploadId
        store.delete(batchId)
        if (uploadId != null) {
            try {
                api.cancelUpload(uploadId)
            } catch (_: ApiException) {
                // The server deletes partial uploads after a week anyway.
            }
        }
    }

    private companion object {
        val RETRY_STATUSES = setOf(408, 429, 500, 502, 503, 504)
    }
}
