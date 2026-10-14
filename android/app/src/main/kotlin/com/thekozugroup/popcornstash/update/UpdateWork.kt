package com.thekozugroup.popcornstash.update

import android.content.Context
import android.content.pm.ServiceInfo
import android.os.Build
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.ExistingWorkPolicy
import androidx.work.ForegroundInfo
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.OutOfQuotaPolicy
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import androidx.work.workDataOf
import com.thekozugroup.popcornstash.PopcornStashApplication
import com.thekozugroup.popcornstash.data.api.StashJson
import kotlinx.serialization.SerializationException
import java.util.concurrent.TimeUnit

/**
 * The updater's WorkManager jobs: the daily check ([UpdateCheckWorker], once a day on any
 * network, kept while the setting is on) and a download ([UpdateDownloadWorker], one at a time).
 */
class WorkManagerUpdateJobs(private val context: Context) : UpdateJobs {
    private val work by lazy { WorkManager.getInstance(context) }

    override fun scheduleDailyCheck(enabled: Boolean) {
        if (!enabled) {
            work.cancelUniqueWork(DAILY_CHECK)
            return
        }
        work.enqueueUniquePeriodicWork(DAILY_CHECK, ExistingPeriodicWorkPolicy.KEEP, dailyCheckRequest())
    }

    override fun startDownload(build: AppBuild) {
        val request = OneTimeWorkRequestBuilder<UpdateDownloadWorker>()
            .setInputData(workDataOf(UpdateDownloadWorker.KEY_BUILD to StashJson.encodeToString(AppBuild.serializer(), build)))
            .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
            .setExpedited(OutOfQuotaPolicy.RUN_AS_NON_EXPEDITED_WORK_REQUEST)
            .build()
        work.enqueueUniqueWork(DOWNLOAD, ExistingWorkPolicy.REPLACE, request)
    }

    companion object {
        const val DAILY_CHECK = "app-update-check"
        const val DOWNLOAD = "app-update-download"

        fun dailyCheckRequest() = PeriodicWorkRequestBuilder<UpdateCheckWorker>(1, TimeUnit.DAYS)
            .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
            .build()
    }
}

/** The daily check: asks GitHub once and posts one notification for a newer build ([AppUpdates.backgroundCheck]). */
class UpdateCheckWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        val app = applicationContext as? PopcornStashApplication ?: return Result.failure()
        app.container.updates.backgroundCheck()
        // A failed check waits for tomorrow; retrying sooner would only spend GitHub's hourly limit.
        return Result.success()
    }
}

/** Downloads and checks one build with a progress notification ([AppUpdates.download]). */
class UpdateDownloadWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    private val notifications = UpdateNotifications(context)

    private val build: AppBuild?
        get() = inputData.getString(KEY_BUILD)?.let {
            try {
                StashJson.decodeFromString(AppBuild.serializer(), it)
            } catch (_: SerializationException) {
                null
            } catch (_: IllegalArgumentException) {
                null
            }
        }

    override suspend fun getForegroundInfo(): ForegroundInfo = foreground(build?.number ?: 0, null)

    override suspend fun doWork(): Result {
        val build = build ?: return Result.failure()
        val app = applicationContext as? PopcornStashApplication ?: return Result.failure()
        // A large download outlives the app on screen; without a foreground service it would be cut off.
        runCatching { setForeground(foreground(build.number, null)) }
        try {
            val verified = app.container.updates.download(build) { percent ->
                notifications.downloading(build.number, percent)
            }
            return if (verified) Result.success() else Result.failure()
        } finally {
            notifications.cancelDownloading()
        }
    }

    private fun foreground(number: Int, percent: Int?): ForegroundInfo {
        val notification = notifications.downloadingNotification(number, percent)
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            ForegroundInfo(UpdateNotifications.DOWNLOAD_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
        } else {
            ForegroundInfo(UpdateNotifications.DOWNLOAD_ID, notification)
        }
    }

    companion object {
        const val KEY_BUILD = "build"
    }
}
