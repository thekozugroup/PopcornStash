package com.thekozugroup.popcornstash.update

import android.Manifest
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import com.thekozugroup.popcornstash.R
import com.thekozugroup.popcornstash.navigation.OpenTab
import com.thekozugroup.popcornstash.navigation.Tab

/**
 * The updater's notifications, on the "App updates" channel: one when the daily check finds a
 * newer build ("PopcornStash build 415 is ready to install"), and the download's progress.
 * Both open Settings.
 */
class UpdateNotifications(private val context: Context) : UpdateNotifier {
    private val manager = NotificationManagerCompat.from(context)

    private fun ensureChannel() {
        val channel = NotificationChannel(CHANNEL, "App updates", NotificationManager.IMPORTANCE_LOW).apply {
            description = "A newer build of PopcornStash, and its download."
            setShowBadge(false)
        }
        context.getSystemService(NotificationManager::class.java)?.createNotificationChannel(channel)
    }

    override fun newBuild(build: AppBuild) {
        ensureChannel()
        notify(
            NEW_BUILD_ID,
            base()
                .setContentTitle(newBuildTitle(build.number))
                .setContentText("Open Settings to update.")
                .setAutoCancel(true)
                .build(),
        )
    }

    override fun cancel() = manager.cancel(NEW_BUILD_ID)

    fun downloadingNotification(number: Int, percent: Int?): Notification {
        ensureChannel()
        return base()
            .setContentTitle("Downloading build $number")
            .apply { if (percent != null) setContentText("$percent%") }
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setCategory(NotificationCompat.CATEGORY_PROGRESS)
            .setProgress(100, percent ?: 0, percent == null)
            .build()
    }

    fun downloading(number: Int, percent: Int?) = notify(DOWNLOAD_ID, downloadingNotification(number, percent))

    fun cancelDownloading() = manager.cancel(DOWNLOAD_ID)

    private fun base(): NotificationCompat.Builder =
        NotificationCompat.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_stat_update)
            .setContentIntent(openSettings())
            .setSilent(true)

    private fun openSettings(): PendingIntent = PendingIntent.getActivity(
        context,
        REQUEST_CODE,
        OpenTab.intent(context, Tab.Settings),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

    private fun notify(id: Int, notification: Notification) {
        // Without the permission (Android 13 and later) the updater still works; it just stays quiet.
        val allowed = Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED
        if (allowed) manager.notify(id, notification)
    }

    companion object {
        const val CHANNEL = "app-updates"
        const val NEW_BUILD_ID = 0x5550_0001
        const val DOWNLOAD_ID = 0x5550_0002
        private const val REQUEST_CODE = 416

        fun newBuildTitle(number: Int) = "PopcornStash build $number is ready to install"
    }
}
