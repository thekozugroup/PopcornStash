package com.thekozugroup.popcornstash.feature.downloads

import com.thekozugroup.popcornstash.data.api.ApiException
import com.thekozugroup.popcornstash.data.api.DownloadKind
import com.thekozugroup.popcornstash.data.api.DownloadState
import com.thekozugroup.popcornstash.data.api.TorrentFile
import com.thekozugroup.popcornstash.data.api.TorrentFileKind
import com.thekozugroup.popcornstash.data.api.TorrentFiles
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.withContext
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class TorrentFilesControllerTest {
    private val download = DownloadsSamples.downloading
    private val files = listOf(
        TorrentFile(4, "Episode/film.mkv", 8_000_000_000L, TorrentFileKind.Video, true),
        TorrentFile(8, "Episode/en.srt", 54_000, TorrentFileKind.Subtitle, false),
        TorrentFile(12, "Episode/poster.jpg", 430_000, TorrentFileKind.Image, true),
        TorrentFile(15, "Episode/readme.txt", 800, TorrentFileKind.Other, true),
    )

    @Test
    fun reviewFiltersThenManualOverrideStartExactlyTheFinalSelection() = runTest {
        val starts = mutableListOf<List<Int>>()
        val controller = TorrentFilesController({ TorrentFiles(true, files, reviewRequired = true) }, { _, _ -> error("review must use start") }, this,
            start = { _, indices -> starts += indices })
        controller.open(download)
        runCurrent()
        assertTrue(controller.state.value!!.canSave)
        controller.draft(TorrentRuleDraft(enabled = true, include = "episode", minimum = "1000"))
        controller.applyFilters()
        assertEquals(setOf(4), controller.state.value!!.selected)
        controller.toggle(8)
        controller.submit()
        controller.submit()
        runCurrent()
        assertEquals(listOf(listOf(4, 8)), starts)
        assertNull(controller.state.value)
    }

    @Test
    fun openingPreservesExistingChoicesIncludingImagesAndOtherFilesAndCancelSendsNothing() = runTest {
        val writes = mutableListOf<List<Int>>()
        val controller = TorrentFilesController({ TorrentFiles(true, files) }, { _, indices -> writes += indices }, this)
        controller.open(download)
        assertTrue(controller.state.value!!.loading)
        runCurrent()
        assertEquals(setOf(4, 12, 15), controller.state.value!!.selected)
        assertFalse(controller.state.value!!.canSave)
        controller.toggle(4)
        controller.toggle(8)
        assertEquals(setOf(8, 12, 15), controller.state.value!!.selected)
        controller.close()
        assertNull(controller.state.value)
        assertTrue(writes.isEmpty())
    }

    @Test
    fun saveSendsActualManifestIndicesAndOnlyAfterAnExplicitChange() = runTest {
        val writes = mutableListOf<Pair<String, List<Int>>>()
        var saved = 0
        val controller = TorrentFilesController({ TorrentFiles(true, files) }, { id, indices -> writes += id to indices }, this, { saved++ })
        controller.open(download.copy(state = DownloadState.Paused))
        runCurrent()
        controller.submit()
        assertTrue(writes.isEmpty())
        controller.toggle(12)
        controller.submit()
        controller.submit()
        controller.toggle(8)
        controller.close()
        assertTrue(controller.state.value!!.busy)
        runCurrent()
        assertEquals(listOf(download.id to listOf(4, 15)), writes)
        assertEquals(1, saved)
        assertNull(controller.state.value)
    }

    @Test
    fun anEmptySelectionCannotBeSavedAndUnknownIndicesAreIgnored() = runTest {
        var writes = 0
        val controller = TorrentFilesController({ TorrentFiles(true, files) }, { _, _ -> writes++ }, this)
        controller.open(download)
        controller.toggle(8) // Nothing can change before the manifest is loaded.
        runCurrent()
        listOf(4, 12, 15).forEach(controller::toggle)
        controller.toggle(900)
        assertTrue(controller.state.value!!.selected.isEmpty())
        assertFalse(controller.state.value!!.canSave)
        controller.submit()
        runCurrent()
        assertEquals(0, writes)
        controller.toggle(8)
        assertTrue(controller.state.value!!.canSave)
    }

    @Test
    fun pendingMetadataCanBeCheckedAgainWithoutApplyingDefaults() = runTest {
        var ready = false
        val controller = TorrentFilesController({ TorrentFiles(ready, if (ready) files else emptyList()) }, { _, _ -> }, this)
        controller.open(download)
        runCurrent()
        assertFalse(controller.state.value!!.ready)
        assertFalse(controller.state.value!!.canSave)
        ready = true
        controller.refresh()
        runCurrent()
        assertEquals(setOf(4, 12, 15), controller.state.value!!.selected)
    }

    @Test
    fun failedSaveKeepsChoicesForRetryAndFailedLoadCanRecover() = runTest {
        var failLoad = true
        var failSave = true
        val controller = TorrentFilesController(
            load = { if (failLoad) throw ApiException.network() else TorrentFiles(true, files) },
            save = { _, _ -> if (failSave) throw ApiException(409, "conflict", "That torrent is no longer active.") },
            scope = this,
        )
        controller.open(download)
        runCurrent()
        assertEquals(ApiException.network().message, controller.state.value!!.error)
        failLoad = false
        controller.refresh()
        runCurrent()
        controller.toggle(15)
        controller.submit()
        runCurrent()
        assertEquals("That torrent is no longer active.", controller.state.value!!.error)
        assertEquals(setOf(4, 12), controller.state.value!!.selected)
        assertTrue(controller.state.value!!.canSave)
        failSave = false
        controller.submit()
        runCurrent()
        assertNull(controller.state.value)
    }

    @Test
    fun aLateAnswerCannotReplaceAnotherTorrentsChoices() = runTest {
        val first = CompletableDeferred<TorrentFiles>()
        val controller = TorrentFilesController(
            load = { id -> if (id == download.id) withContext(NonCancellable) { first.await() } else TorrentFiles(true, files.take(1)) },
            save = { _, _ -> },
            scope = this,
        )
        controller.open(download)
        runCurrent()
        controller.open(download.copy(id = "second"))
        runCurrent()
        first.complete(TorrentFiles(true, files))
        runCurrent()
        assertEquals("second", controller.state.value!!.download.id)
        assertEquals(files.take(1), controller.state.value!!.files)
    }

    @Test
    fun webLinksAndFinishedTorrentsHaveNoFileChoice() = runTest {
        var reads = 0
        val controller = TorrentFilesController({ reads++; TorrentFiles(true, files) }, { _, _ -> }, this)
        controller.open(download.copy(kind = DownloadKind.Http))
        controller.open(download.copy(state = DownloadState.Sorted))
        runCurrent()
        assertNull(controller.state.value)
        assertEquals(0, reads)
    }
}
