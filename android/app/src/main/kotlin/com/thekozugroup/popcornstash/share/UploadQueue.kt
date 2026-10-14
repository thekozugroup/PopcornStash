package com.thekozugroup.popcornstash.share

import android.content.Context
import android.content.Intent
import androidx.core.net.toUri
import androidx.work.BackoffPolicy
import androidx.work.Constraints
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.OutOfQuotaPolicy
import androidx.work.WorkInfo
import androidx.work.WorkManager
import androidx.work.workDataOf
import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.ApiException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.withTimeoutOrNull
import java.io.File
import java.util.UUID
import java.util.concurrent.TimeUnit

/**
 * The share sheet's uploads (docs/architecture.md section 10, Share sheet): each confirmed
 * batch is a WorkManager job ([UploadWorker]) with a progress notification. A batch belongs
 * to the person who confirmed it: signing out stops every job and deletes what the server
 * holds of them ([stopAll]), and nothing of it is kept ([forget]).
 */
class UploadQueue(
    private val context: Context,
    val store: UploadBatchStore,
    scope: CoroutineScope,
) {
    private val work by lazy { WorkManager.getInstance(context) }

    /** Batches waiting or running, for the Downloads tab. */
    val progress: StateFlow<List<UploadBatchProgress>> by lazy {
        work.getWorkInfosByTagFlow(TAG)
            .map { infos -> infos.mapNotNull(::progressOf) }
            .stateIn(scope, SharingStarted.Eagerly, emptyList())
    }

    /** Keeps [batch] and starts its job. The share sheet has already kept its read grants. */
    fun enqueue(batch: UploadBatch) {
        store.save(batch)
        val request = OneTimeWorkRequestBuilder<UploadWorker>()
            .setInputData(workDataOf(UploadWorker.KEY_BATCH to batch.id))
            .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
            .setExpedited(OutOfQuotaPolicy.RUN_AS_NON_EXPEDITED_WORK_REQUEST)
            .setBackoffCriteria(BackoffPolicy.EXPONENTIAL, 30, TimeUnit.SECONDS)
            .addTag(TAG)
            .addTag(BATCH_TAG_PREFIX + batch.id)
            .build()
        work.enqueueUniqueWork(workName(batch.id), ExistingWorkPolicy.KEEP, request)
    }

    /** The person stopped one batch: the job ends and the server deletes what arrived. */
    suspend fun stop(batchId: String, api: ApiClient?) {
        val batch = store.load(batchId)
        store.delete(batchId)
        runCatching { work.cancelUniqueWork(workName(batchId)) }
        batch?.let { release(it) }
        if (api != null) batch?.let { deleteOnServer(api, listOf(it)) }
    }

    /**
     * Signing out, while the session still works: every job stops and the server deletes the
     * partial uploads. The batch files go first, so a job between two chunks sends nothing more.
     */
    suspend fun stopAll(api: ApiClient) {
        val batches = store.all()
        store.clear()
        runCatching { work.cancelAllWorkByTag(TAG) }
        batches.forEach(::release)
        withTimeoutOrNull(3_000) { deleteOnServer(api, batches) }
    }

    /** The session ended (sign-out, a 401, someone else signing in): nothing of it stays. */
    fun forget() {
        store.all().forEach(::release)
        store.clear()
        // WorkManager may not be set up yet in a process that never uploaded anything.
        runCatching { work.cancelAllWorkByTag(TAG) }
        UploadNotifications(context).cancelAll()
    }

    /** Gives back the read grants the share sheet kept for [batch]. */
    fun release(batch: UploadBatch) {
        batch.files.forEach { file ->
            runCatching {
                context.revokeUriPermission(context.packageName, file.uri.toUri(), Intent.FLAG_GRANT_READ_URI_PERMISSION)
            }
        }
    }

    private suspend fun deleteOnServer(api: ApiClient, batches: List<UploadBatch>) {
        for (file in batches.flatMap { it.files }) {
            val id = file.uploadId ?: continue
            if (file.done) continue
            try {
                api.cancelUpload(id)
            } catch (_: ApiException) {
                // The server deletes partial uploads after a week anyway.
            }
        }
    }

    private fun progressOf(info: WorkInfo): UploadBatchProgress? {
        if (info.state.isFinished) return null
        val batchId = info.tags.firstOrNull { it.startsWith(BATCH_TAG_PREFIX) }?.removePrefix(BATCH_TAG_PREFIX)
            ?: info.progress.getString(UploadWorker.KEY_BATCH)
            ?: return null
        val batch = store.load(batchId) ?: return null
        val running = info.state == WorkInfo.State.RUNNING
        val sent = info.progress.getLong(UploadWorker.KEY_SENT, -1)
        val total = info.progress.getLong(UploadWorker.KEY_TOTAL, -1)
        val sizes = batch.files.map { it.size }
        return UploadBatchProgress(
            id = batch.id,
            names = batch.files.map { it.name },
            doneFiles = if (running) info.progress.getInt(UploadWorker.KEY_DONE, batch.files.count { it.done }) else batch.files.count { it.done },
            sentBytes = sent.coerceAtLeast(0),
            totalBytes = total.takeIf { it >= 0 } ?: if (sizes.all { it != null }) sizes.sumOf { it ?: 0 } else null,
            waiting = !running,
        )
    }

    companion object {
        const val TAG = "popcornstash-upload"
        const val BATCH_TAG_PREFIX = "batch:"

        fun workName(batchId: String) = "upload-$batchId"

        fun newBatchId(): String = UUID.randomUUID().toString()

        /** Where batches are kept: never backed up, deleted on sign-out. */
        fun storeDir(context: Context) = File(context.noBackupFilesDir, "uploads")
    }
}
