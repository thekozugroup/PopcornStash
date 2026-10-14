package com.thekozugroup.popcornstash.share

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder

class UploadBatchStoreTest {
    @get:Rule
    val folder = TemporaryFolder()

    private fun batch(id: String, at: Long) =
        UploadBatch(id, "u-ana", "Ana Ribeiro", "https://stash.example.com/", listOf(SharedFile("content://m/1", "a.jpg", 3)), at)

    @Test
    fun batchesAreKeptUpdatedAndListedOldestFirst() {
        val dir = folder.newFolder("uploads")
        val store = UploadBatchStore(dir)
        store.save(batch("b-2", 20))
        store.save(batch("b-1", 10))
        assertEquals(listOf("b-1", "b-2"), store.all().map { it.id })
        store.update("b-1", 0) { it.copy(uploadId = "u9") }
        assertEquals("u9", store.load("b-1")!!.files[0].uploadId)
        store.delete("b-1")
        assertNull(store.load("b-1"))
        store.clear()
        assertFalse(dir.exists())
        assertTrue(store.all().isEmpty())
    }

    @Test(expected = IllegalArgumentException::class)
    fun anIdCannotReachOutsideTheFolder() {
        UploadBatchStore(folder.newFolder("uploads")).load("../token")
    }

    @Test
    fun progressReadsAsTheRowAndNotificationSayIt() {
        val one = UploadBatchProgress("b", listOf("Holiday.mov"), 0, 50, 200, waiting = false)
        assertEquals("Holiday.mov", one.label)
        assertEquals(0.25, one.fraction!!, 0.0001)
        assertEquals("Uploading Holiday.mov", UploadNotifications.uploadingTitle(one))
        assertEquals("25%", UploadNotifications.uploadingText(one))
        val three = UploadBatchProgress("b", listOf("a", "b", "c"), 1, 0, null, waiting = false)
        assertEquals("a and 2 more", three.label)
        assertEquals("Uploading 3 files", UploadNotifications.uploadingTitle(three))
        assertEquals("1 of 3 done", UploadNotifications.uploadingText(three))
    }

    @Test
    fun theLastNotificationSaysHowItEnded() {
        assertEquals("3 files uploaded" to "They will be sorted on their own.", UploadNotifications.endWords(UploadOutcome.Done(3)))
        assertEquals("1 file uploaded" to "It will be sorted on its own.", UploadNotifications.endWords(UploadOutcome.Done(1)))
        assertEquals("Upload stopped" to "Share the file again to finish uploading.", UploadNotifications.endWords(UploadOutcome.SourceGone("a")))
        assertNull(UploadNotifications.endWords(UploadOutcome.NotSignedIn))
        assertNull(UploadNotifications.endWords(UploadOutcome.Stopped))
    }
}
