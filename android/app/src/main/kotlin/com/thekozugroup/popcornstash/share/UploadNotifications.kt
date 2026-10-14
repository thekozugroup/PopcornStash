package com.thekozugroup.popcornstash.share

import android.Manifest
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import androidx.work.ForegroundInfo
import com.thekozugroup.popcornstash.R
import com.thekozugroup.popcornstash.navigation.OpenTab
import com.thekozugroup.popcornstash.navigation.Tab

/**
 * The upload notifications (android/DESIGN.md, Share sheet): "Uploading 3 files" with
 * progress while the job runs, then "3 files uploaded" with "They will be sorted on their
 * own.", or why it stopped. Tapping one opens Downloads. Every one carries [TAG], so signing
 * out removes them all ([cancelAll]).
 */
class UploadNotifications(private val context: Context) {
    private val manager = NotificationManagerCompat.from(context)

    fun ensureChannel() {
        val channel = NotificationChannel(CHANNEL, "Uploads", NotificationManager.IMPORTANCE_LOW).apply {
            description = "Files you share to PopcornStash while they upload."
            setShowBadge(false)
        }
        context.getSystemService(NotificationManager::class.java)?.createNotificationChannel(channel)
    }

    /** The running job's notification, as a foreground service of type dataSync. */
    fun foreground(batchId: String, progress: UploadBatchProgress?, stop: PendingIntent?): ForegroundInfo {
        val notification = progressNotification(progress, stop)
        val id = progressId(batchId)
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            ForegroundInfo(id, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
        } else {
            ForegroundInfo(id, notification)
        }
    }

    fun progressNotification(progress: UploadBatchProgress?, stop: PendingIntent?): Notification {
        ensureChannel()
        val fraction = progress?.fraction
        val builder = base()
            .setContentTitle(progress?.let { uploadingTitle(it) } ?: "Uploading")
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setCategory(NotificationCompat.CATEGORY_PROGRESS)
            .setProgress(100, ((fraction ?: 0.0) * 100).toInt(), fraction == null)
        progress?.let { builder.setContentText(uploadingText(it)) }
        stop?.let { builder.addAction(0, "Stop", it) }
        return builder.build()
    }

    /** Replaces the progress with how the batch ended. */
    fun finished(batchId: String, outcome: UploadOutcome) {
        val (title, text) = endWords(outcome) ?: run {
            cancel(batchId)
            return
        }
        ensureChannel()
        manager.cancel(TAG, progressId(batchId))
        notify(endId(batchId), base().setContentTitle(title).setContentText(text).setStyle(NotificationCompat.BigTextStyle().bigText(text)).build())
    }

    fun update(batchId: String, progress: UploadBatchProgress, stop: PendingIntent?) {
        notify(progressId(batchId), progressNotification(progress, stop))
    }

    fun cancel(batchId: String) {
        manager.cancel(TAG, progressId(batchId))
        manager.cancel(TAG, endId(batchId))
    }

    /** Signing out: every upload notification goes, whatever batch it was for. */
    fun cancelAll() {
        val system = context.getSystemService(NotificationManager::class.java) ?: return
        runCatching { system.activeNotifications }.getOrNull().orEmpty()
            .filter { it.tag == TAG }
            .forEach { system.cancel(it.tag, it.id) }
    }

    private fun base(): NotificationCompat.Builder =
        NotificationCompat.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_stat_upload)
            .setContentIntent(openDownloads())
            .setVisibility(NotificationCompat.VISIBILITY_PRIVATE)
            .setPublicVersion(
                NotificationCompat.Builder(context, CHANNEL)
                    .setSmallIcon(R.drawable.ic_stat_upload)
                    .setContentTitle("PopcornStash")
                    .setContentText("Uploads")
                    .build(),
            )
            .setAutoCancel(true)
            .setSilent(true)

    private fun openDownloads(): PendingIntent = PendingIntent.getActivity(
        context,
        0,
        OpenTab.intent(context, Tab.Downloads),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

    private fun notify(id: Int, notification: Notification) {
        // Without the permission (Android 13 and later) the upload still runs; it just stays quiet.
        val allowed = Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED
        if (allowed) manager.notify(TAG, id, notification)
    }

    companion object {
        const val CHANNEL = "uploads"
        const val TAG = "upload"

        fun progressId(batchId: String) = batchId.hashCode() and 0x3fffffff

        fun endId(batchId: String) = progressId(batchId) or 0x40000000

        /** "Uploading 3 files", "Uploading Holiday.mov". */
        fun uploadingTitle(progress: UploadBatchProgress): String =
            if (progress.totalFiles == 1) "Uploading ${progress.names.first()}" else "Uploading ${progress.totalFiles} files"

        /** "1 of 3 done · 42%", "42%". */
        fun uploadingText(progress: UploadBatchProgress): String {
            val percent = progress.fraction?.let { "${(it * 100).toInt()}%" }
            val files = if (progress.totalFiles > 1) "${progress.doneFiles} of ${progress.totalFiles} done" else null
            return listOfNotNull(files, percent).joinToString(" · ").ifEmpty { "Starting" }
        }

        /** The last notification's title and text, or null when it should just go away. */
        fun endWords(outcome: UploadOutcome): Pair<String, String>? = when (outcome) {
            is UploadOutcome.Done -> if (outcome.count == 1) {
                "1 file uploaded" to "It will be sorted on its own."
            } else {
                "${outcome.count} files uploaded" to "They will be sorted on their own."
            }
            is UploadOutcome.SourceGone -> "Upload stopped" to "Share the file again to finish uploading."
            is UploadOutcome.Failed -> "Upload stopped" to outcome.message
            is UploadOutcome.RetryLater -> null
            UploadOutcome.NotSignedIn, UploadOutcome.Stopped -> null
        }
    }
}
