package com.thekozugroup.popcornstash.update

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import androidx.work.Configuration
import androidx.work.NetworkType
import androidx.work.WorkInfo
import androidx.work.WorkManager
import androidx.work.testing.SynchronousExecutor
import androidx.work.testing.WorkManagerTestInitHelper
import com.thekozugroup.popcornstash.data.api.StashJson
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import java.util.concurrent.TimeUnit

/** The updater's WorkManager jobs: the daily check follows the setting, and one download at a time. */
@RunWith(RobolectricTestRunner::class)
class UpdateWorkTest {
    private val context = ApplicationProvider.getApplicationContext<Context>()
    private lateinit var work: WorkManager
    private lateinit var jobs: WorkManagerUpdateJobs

    @Before
    fun setUp() {
        WorkManagerTestInitHelper.initializeTestWorkManager(
            context,
            Configuration.Builder().setExecutor(SynchronousExecutor()).build(),
        )
        work = WorkManager.getInstance(context)
        jobs = WorkManagerUpdateJobs(context)
    }

    private fun daily() = work.getWorkInfosForUniqueWork(WorkManagerUpdateJobs.DAILY_CHECK).get()

    @Test
    fun theDailyCheckRunsOnceADayOnAnyNetwork() {
        jobs.scheduleDailyCheck(true)
        val info = daily().single()
        assertEquals(WorkInfo.State.ENQUEUED, info.state)
        assertEquals(TimeUnit.DAYS.toMillis(1), info.periodicityInfo?.repeatIntervalMillis)
        assertEquals(NetworkType.CONNECTED, info.constraints.requiredNetworkType)
        assertTrue(info.tags.contains(UpdateCheckWorker::class.java.name))
    }

    @Test
    fun schedulingAgainKeepsTheSameJob() {
        jobs.scheduleDailyCheck(true)
        val first = daily().single().id
        jobs.scheduleDailyCheck(true)
        assertEquals(first, daily().single().id)
    }

    @Test
    fun turningItOffCancelsTheJob() {
        jobs.scheduleDailyCheck(true)
        jobs.scheduleDailyCheck(false)
        assertEquals(WorkInfo.State.CANCELLED, daily().single().state)
        jobs.scheduleDailyCheck(true)
        assertEquals(WorkInfo.State.ENQUEUED, daily().single { it.state != WorkInfo.State.CANCELLED }.state)
    }

    @Test
    fun aDownloadIsOneJobThatCarriesItsBuild() {
        val build = UpdateFixtures.build(415)
        // Both wait for a network, which the test never gives them, so neither runs.
        jobs.startDownload(build)
        jobs.startDownload(build.copy(number = 416))
        val infos = work.getWorkInfosForUniqueWork(WorkManagerUpdateJobs.DOWNLOAD).get()
        val live = infos.filter { !it.state.isFinished }
        assertEquals(1, live.size)
        assertEquals(NetworkType.CONNECTED, live.single().constraints.requiredNetworkType)
        assertTrue(live.single().tags.contains(UpdateDownloadWorker::class.java.name))
    }

    @Test
    fun theDownloadJobsInputIsTheBuild() {
        val build = UpdateFixtures.build(415)
        val json = StashJson.encodeToString(AppBuild.serializer(), build)
        assertEquals(build, StashJson.decodeFromString(AppBuild.serializer(), json))
    }
}
