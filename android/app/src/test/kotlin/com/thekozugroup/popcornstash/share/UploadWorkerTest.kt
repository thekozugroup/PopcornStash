package com.thekozugroup.popcornstash.share

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import androidx.work.ListenableWorker
import androidx.work.testing.TestListenableWorkerBuilder
import androidx.work.workDataOf
import com.thekozugroup.popcornstash.PopcornStashApplication
import com.thekozugroup.popcornstash.data.session.SessionState
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/** The upload job in the app's own process: it never uploads for someone who is not signed in. */
@RunWith(RobolectricTestRunner::class)
class UploadWorkerTest {
    private val context = ApplicationProvider.getApplicationContext<Context>()
    private val app = context as PopcornStashApplication

    @Test
    fun withNobodySignedInTheJobSendsNothingAndForgetsTheBatch() = runBlocking {
        withTimeout(10_000) { app.container.sessions.state.first { it is SessionState.SignedOut } }
        val store = app.container.uploads.store
        store.save(
            UploadBatch(
                id = "batch-signed-out",
                userId = "u-ana",
                userName = "Ana Ribeiro",
                server = "https://stash.example.com/",
                files = listOf(SharedFile("content://media/external/images/1", "PXL_1.jpg", 10)),
            ),
        )
        val worker = TestListenableWorkerBuilder<UploadWorker>(context)
            .setInputData(workDataOf(UploadWorker.KEY_BATCH to "batch-signed-out"))
            .build()
        val result = worker.doWork()
        assertEquals(ListenableWorker.Result.failure(), result)
        assertNull(store.load("batch-signed-out"))
    }

    @Test
    fun aBatchThatIsGoneEndsTheJobQuietly() = runBlocking {
        val worker = TestListenableWorkerBuilder<UploadWorker>(context)
            .setInputData(workDataOf(UploadWorker.KEY_BATCH to "batch-gone"))
            .build()
        assertTrue(worker.doWork() is ListenableWorker.Result.Success)
    }
}
