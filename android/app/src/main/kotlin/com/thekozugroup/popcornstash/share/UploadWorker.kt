package com.thekozugroup.popcornstash.share

import android.content.Context
import androidx.core.net.toUri
import androidx.work.CoroutineWorker
import androidx.work.ForegroundInfo
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import androidx.work.workDataOf
import com.thekozugroup.popcornstash.PopcornStashApplication
import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.session.SessionState
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull

/**
 * Uploads one confirmed share-sheet batch ([UploadRunner]) as a foreground job with a
 * progress notification. It runs only for the person who confirmed the batch, on the same
 * server; for anyone else, or nobody, it sends nothing and drops the batch.
 */
class UploadWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    private val notifications = UploadNotifications(context)
    private val batchId: String? get() = inputData.getString(KEY_BATCH)

    override suspend fun getForegroundInfo(): ForegroundInfo =
        notifications.foreground(batchId ?: "unknown", startingProgress(), stopIntent())

    override suspend fun doWork(): Result {
        val id = batchId ?: return Result.failure()
        val app = applicationContext as? PopcornStashApplication ?: return Result.failure()
        val queue = app.container.uploads
        // Kept for the read grants: the runner forgets the batch itself once it ends.
        val batch = queue.store.load(id) ?: return Result.success()
        // A long upload outlives the app on screen; without a foreground service it would be cut off.
        runCatching { setForeground(notifications.foreground(id, startingProgress(), stopIntent())) }
        var lastNotified = 0L
        val runner = UploadRunner(
            store = queue.store,
            apiFor = { batch -> apiFor(app, batch) },
            open = { file -> ContentChunkSource(applicationContext.contentResolver, file.uri.toUri()) },
            measure = { file -> ContentChunkSource(applicationContext.contentResolver, file.uri.toUri()).use { it.measure() } },
        )
        val outcome = try {
            runner.run(id) { progress ->
                setProgress(
                    workDataOf(
                        KEY_BATCH to id,
                        KEY_SENT to progress.sentBytes,
                        KEY_TOTAL to (progress.totalBytes ?: -1L),
                        KEY_DONE to progress.doneFiles,
                    ),
                )
                val now = System.currentTimeMillis()
                if (now - lastNotified >= NOTIFY_EVERY_MS) {
                    lastNotified = now
                    notifications.update(id, progress, stopIntent())
                }
            }
        } catch (e: CancellationException) {
            notifications.cancel(id)
            // Stopped from the notification: nothing of the batch stays, here or on the server.
            // (Signing out has already removed the batch and done the same.)
            withContext(NonCancellable) {
                withTimeoutOrNull(5_000) { queue.stop(id, queue.store.load(id)?.let { apiFor(app, it) }) }
            }
            throw e
        }
        if (outcome is UploadOutcome.RetryLater && runAttemptCount < MAX_ATTEMPTS) return Result.retry()
        val final = if (outcome is UploadOutcome.RetryLater) UploadOutcome.Failed(outcome.message) else outcome
        queue.store.delete(id)
        queue.release(batch)
        notifications.finished(id, final)
        return if (final is UploadOutcome.Done) Result.success() else Result.failure()
    }

    private fun startingProgress(): UploadBatchProgress? {
        val id = batchId ?: return null
        val app = applicationContext as? PopcornStashApplication ?: return null
        val batch = app.container.uploads.store.load(id) ?: return null
        val sizes = batch.files.map { it.size }
        return UploadBatchProgress(
            id = id,
            names = batch.files.map { it.name },
            doneFiles = batch.files.count { it.done },
            sentBytes = 0,
            totalBytes = if (sizes.all { it != null }) sizes.sumOf { it ?: 0 } else null,
            waiting = false,
        )
    }

    private fun stopIntent() = WorkManager.getInstance(applicationContext).createCancelPendingIntent(id)

    companion object {
        const val KEY_BATCH = "batch"
        const val KEY_SENT = "sent"
        const val KEY_TOTAL = "total"
        const val KEY_DONE = "done"
        private const val NOTIFY_EVERY_MS = 1_000L
        private const val MAX_ATTEMPTS = 8

        /** The signed-in session's API, only when it is the person who confirmed [batch]. */
        suspend fun apiFor(app: PopcornStashApplication, batch: UploadBatch): ApiClient? {
            val sessions = app.container.sessions
            val state = withTimeoutOrNull(15_000) { sessions.state.first { it !is SessionState.Restoring } } ?: return null
            val session = (state as? SessionState.SignedIn)?.session ?: return null
            if (session.user.value.id != batch.userId || session.server.toString() != batch.server) return null
            return session.api
        }
    }
}
