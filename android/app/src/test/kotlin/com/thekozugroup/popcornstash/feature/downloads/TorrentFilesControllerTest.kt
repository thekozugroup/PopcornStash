package com.thekozugroup.popcornstash.feature.downloads

import com.thekozugroup.popcornstash.data.api.ApiException
import com.thekozugroup.popcornstash.data.api.DownloadKind
import com.thekozugroup.popcornstash.data.api.DownloadState
import com.thekozugroup.popcornstash.data.api.FinishCompletedRequest
import com.thekozugroup.popcornstash.data.api.TorrentFile
import com.thekozugroup.popcornstash.data.api.TorrentFileKind
import com.thekozugroup.popcornstash.data.api.TorrentFiles
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.test.advanceTimeBy
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
        var manifest = TorrentFiles(true, files, reviewRequired = true)
        val controller = TorrentFilesController({ manifest }, { _, _ -> error("review must use start") }, this,
            start = { _, indices ->
                starts += indices
                manifest = TorrentFiles(true, files.map { it.copy(selected = it.index in indices, verifiedBytes = 0, complete = false) })
            })
        controller.open(download)
        runCurrent()
        assertTrue(controller.state.value!!.canSave)
        controller.draft(TorrentRuleDraft(enabled = true, include = "episode", minimum = "1000"))
        assertEquals(setOf(4, 12, 15), controller.state.value!!.selected)
        assertFalse(controller.state.value!!.canSave)
        controller.filter()
        assertEquals(setOf(4), controller.state.value!!.selected)
        controller.toggle(8)
        controller.submit()
        controller.submit()
        runCurrent()
        assertEquals(listOf(listOf(4, 8)), starts)
        val active = controller.state.value!!
        assertFalse(active.reviewRequired)
        assertFalse(active.filtersEdited)
        assertTrue(active.hasFileProgress)
        assertEquals(setOf(4, 8), active.selected)
    }

    @Test
    fun invalidFiltersBlockSubmitAndOnlyApplyAfterFilter() = runTest {
        var starts = 0
        val controller = TorrentFilesController({ TorrentFiles(true, files, reviewRequired = true) }, { _, _ -> }, this, start = { _, _ -> starts++ })
        controller.open(download)
        runCurrent()
        controller.draft(TorrentRuleDraft(minimum = "100", maximum = "1"))
        controller.filter()
        assertTrue(controller.state.value!!.filterError != null)
        controller.toggle(8)
        controller.submit()
        runCurrent()
        assertEquals(0, starts)
        assertFalse(controller.state.value!!.canSave)
        controller.draft(TorrentRuleDraft(minimum = "100"))
        controller.filter()
        assertEquals(setOf(4), controller.state.value!!.selected)
        assertNull(controller.state.value!!.filterError)
        controller.type(TorrentFileKind.Video)
        controller.type(TorrentFileKind.Subtitle)
        controller.filter()
        assertTrue(controller.state.value!!.selected.isEmpty())
        assertFalse(controller.state.value!!.canSave)
        controller.type(TorrentFileKind.Video)
        controller.filter()
        assertEquals(setOf(4), controller.state.value!!.selected)
        assertTrue(controller.state.value!!.canSave)
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

    @Test
    fun skipChangesOnlyTheRequestedIndexAndReloadsAuthoritativeChoices() = runTest {
        val kept = files[0].copy(verifiedBytes = files[0].size, complete = true)
        val partial = files[2].copy(verifiedBytes = 72L, complete = false)
        var manifest = TorrentFiles(true, listOf(kept, partial))
        val skipped = mutableListOf<Int>()
        var refreshed = 0
        val controller = TorrentFilesController({ manifest }, { _, _ -> error("unexpected selection replacement") }, this,
            onSaved = { refreshed++ }, skip = { _, index ->
                skipped += index
                manifest = manifest.copy(files = listOf(kept, partial.copy(selected = false)))
            })
        controller.open(download)
        runCurrent()
        controller.skipFile(999)
        controller.skipFile(partial.index)
        controller.skipFile(partial.index)
        runCurrent()
        assertEquals(listOf(partial.index), skipped)
        assertEquals(setOf(kept.index), controller.state.value!!.selected)
        assertFalse(controller.state.value!!.canSkip(kept.index))
        controller.skipFile(kept.index)
        runCurrent()
        assertEquals(1, refreshed)
        assertEquals(1, skipped.size)
    }

    @Test
    fun finishRechecksThePlanAndRequiresFreshProgressAfterAnUncertainResult() = runTest {
        val complete = files[0].copy(verifiedBytes = files[0].size, complete = true)
        val skipped = files[1].copy(verifiedBytes = files[1].size, complete = true)
        val partial = files[2].copy(verifiedBytes = 12, complete = false)
        var manifest = TorrentFiles(true, listOf(complete, skipped, partial))
        var attempts = 0
        val sent = mutableListOf<FinishCompletedRequest>()
        val controller = TorrentFilesController({ manifest }, { _, _ -> }, this, finish = { _, request ->
            sent += request
            if (++attempts == 1) throw ApiException.network()
        })
        controller.open(download)
        runCurrent()
        val first = controller.state.value!!.finishPlan!!
        assertEquals(setOf(complete.index), first.completed)
        assertEquals(setOf(complete.index, partial.index), first.selected)
        controller.toggle(partial.index)
        assertNull(controller.state.value!!.finishPlan)
        controller.toggle(partial.index)
        manifest = manifest.copy(files = listOf(complete, skipped, partial.copy(verifiedBytes = partial.size, complete = true)))
        controller.refresh()
        runCurrent()
        val latest = controller.state.value!!.finishPlan!!
        controller.finishCompleted(first) // Another selected file completed. Both can be kept.
        controller.finishCompleted(latest)
        runCurrent()
        assertEquals(1, attempts)
        assertEquals(FinishCompletedRequest(listOf(4, 12), listOf(4)), sent.single())
        assertNull(controller.state.value!!.finishPlan)
        controller.finishCompleted(latest)
        controller.refresh()
        runCurrent()
        controller.finishCompleted(controller.state.value!!.finishPlan!!)
        runCurrent()
        assertEquals(2, attempts)
        assertEquals(FinishCompletedRequest(listOf(4, 12), listOf(4, 12)), sent.last())
        assertNull(controller.state.value)
    }

    @Test
    fun progressRefreshMergesLocalEditsAndAccessLossClearsTheManifest() = runTest {
        var manifest = TorrentFiles(true, files)
        var denied = false
        val controller = TorrentFilesController({
            if (denied) throw ApiException(404, "not_found", "Download unavailable.")
            manifest
        }, { _, _ -> }, this)
        controller.open(download)
        runCurrent()
        controller.toggle(12)
        controller.draft(TorrentRuleDraft(minimum = "10"))
        manifest = manifest.copy(files = files.map { if (it.index == 8) it.copy(selected = true) else it })
        controller.refresh()
        runCurrent()
        assertEquals(setOf(4, 8, 15), controller.state.value!!.selected)
        assertEquals("10", controller.state.value!!.draft.minimum)
        assertTrue(controller.state.value!!.filtersPending)
        denied = true
        controller.refresh()
        runCurrent()
        assertTrue(controller.state.value!!.files.isEmpty())
        assertFalse(controller.state.value!!.canSave)
    }

    @Test
    fun completionRequiresBothVerificationFieldsAndNeverIncludesSkippedFiles() {
        val complete = files.first().copy(size = 100L, verifiedBytes = 100L, complete = true)
        val checking = complete.copy(index = 2, complete = false)
        val unknown = complete.copy(index = 3, verifiedBytes = null)
        val partial = complete.copy(index = 4, verifiedBytes = 50L, complete = false)
        val skipped = complete.copy(index = 5, selected = false)
        val manifest = listOf(complete.copy(index = 1), checking, unknown, partial, skipped)
        val state = TorrentFilesState(download, loading = false, ready = true, files = manifest, selected = setOf(1, 2, 3, 4))
        assertNull(state.finishPlan)
        assertNull(state.copy(reviewRequired = true).finishPlan)
        assertNull(state.copy(needsRefresh = true).finishPlan)
        assertTrue(checking.transferDescription(true).startsWith("Verifying"))
        assertTrue(unknown.transferDescription(true).startsWith("Progress unavailable"))
        assertTrue(partial.transferDescription(true).contains("50%"))
        assertTrue(skipped.transferDescription(false).startsWith("Skipped"))
    }

    @Test
    fun invalidSelectedStatusSuppressesFinishAndCannotBeSkipped() {
        val done = files[0].copy(verifiedBytes = files[0].size, complete = true)
        val partial = files[1].copy(selected = true, size = 100, verifiedBytes = 0, complete = false)
        val noStatus = listOf(
            partial.copy(verifiedBytes = null), partial.copy(complete = null),
            partial.copy(verifiedBytes = -1), partial.copy(verifiedBytes = 101),
            partial.copy(complete = true), partial.copy(verifiedBytes = 100),
            partial.copy(size = 0),
        )
        val state = TorrentFilesState(download, loading = false, ready = true, files = listOf(done, partial), selected = setOf(done.index, partial.index))
        for (file in noStatus) {
            val invalid = state.copy(files = listOf(done, file))
            assertNull(invalid.finishPlan)
            assertFalse(invalid.canSkip(file.index))
            val unselected = invalid.copy(files = listOf(done, file.copy(selected = false)), selected = setOf(done.index))
            assertEquals(setOf(done.index), unselected.finishPlan!!.selected)
            assertFalse(unselected.canSkip(file.index))
        }
        assertFalse(state.canSkip(done.index))
        assertTrue(state.canSkip(partial.index))
        assertFalse(state.copy(files = listOf(partial), selected = setOf(partial.index)).canSkip(partial.index))
    }

    @Test
    fun finalReadRejectsSwappedPendingOrCompletedFileEvenAtTheSameCounts() = runTest {
        val done = files[0].copy(verifiedBytes = files[0].size, complete = true)
        val pending = files[1].copy(selected = true, verifiedBytes = 5, complete = false)
        val original = TorrentFiles(true, listOf(done, pending))
        var manifest = original
        val controller = TorrentFilesController({ manifest }, { _, _ -> }, this, finish = { _, _ -> error("Unexpected finish") })
        for (changed in listOf(
            listOf(done, pending.copy(index = 99)),
            listOf(done.copy(complete = false, verifiedBytes = 2), pending.copy(complete = true, verifiedBytes = pending.size)),
            listOf(done, pending.copy(verifiedBytes = null)),
        )) {
            manifest = original
            controller.open(download)
            runCurrent()
            val confirmed = controller.state.value!!.finishPlan!!
            // Change only the server. The displayed confirmation still matches the last poll.
            manifest = TorrentFiles(true, changed)
            controller.finishCompleted(confirmed)
            runCurrent()
            assertEquals(changed, controller.state.value!!.files)
            assertTrue(controller.state.value!!.error!!.contains("changed"))
        }
    }

    @Test
    fun postConflictReloadsNewSelectionAndRequiresExplicitConfirmation() = runTest {
        val done = files[0].copy(verifiedBytes = files[0].size, complete = true)
        val pending = files[2].copy(verifiedBytes = 2, complete = false)
        var manifest = TorrentFiles(true, listOf(pending, done))
        val sent = mutableListOf<FinishCompletedRequest>()
        val controller = TorrentFilesController({ manifest }, { _, _ -> }, this, finish = { _, request ->
            sent += request
            if (sent.size == 1) {
                manifest = manifest.copy(files = listOf(done, pending.copy(index = 99)))
                throw ApiException(409, "conflict", "Selection changed on another device.")
            }
        })
        controller.open(download)
        runCurrent()
        val original = controller.state.value!!.finishPlan!!
        controller.finishCompleted(original)
        runCurrent()
        val refreshed = controller.state.value!!
        assertEquals("Selection changed on another device.", refreshed.error)
        assertEquals(setOf(done.index, 99), refreshed.selected)
        controller.finishCompleted(original)
        runCurrent()
        val firstRequest = FinishCompletedRequest(listOf(done.index, pending.index), listOf(done.index))
        assertEquals(listOf(firstRequest), sent)
        controller.finishCompleted(refreshed.finishPlan!!)
        runCurrent()
        assertEquals(listOf(firstRequest, FinishCompletedRequest(listOf(done.index, 99), listOf(done.index))), sent)
        assertNull(controller.state.value)
    }

    @Test
    fun lostConfirmedFileAfterTheFinalReadReloadsAndRequiresNewConfirmation() = runTest {
        val done = files[0].copy(verifiedBytes = files[0].size, complete = true)
        val pending = files[2].copy(verifiedBytes = 0, complete = false)
        var manifest = TorrentFiles(true, listOf(done, pending))
        val sent = mutableListOf<FinishCompletedRequest>()
        val controller = TorrentFilesController({ manifest }, { _, _ -> }, this, finish = { _, request ->
            sent += request
            if (sent.size == 1) {
                // The server now sees a different complete file in the exact same selection.
                manifest = manifest.copy(files = listOf(
                    done.copy(verifiedBytes = 0, complete = false),
                    pending.copy(verifiedBytes = pending.size, complete = true),
                ))
                throw ApiException(409, "conflict", "A confirmed file is no longer complete.")
            }
        })
        controller.open(download)
        runCurrent()
        val original = controller.state.value!!.finishPlan!!
        controller.finishCompleted(original)
        runCurrent()
        val refreshed = controller.state.value!!
        assertEquals("A confirmed file is no longer complete.", refreshed.error)
        assertEquals(original.selected, refreshed.selected)
        controller.finishCompleted(original)
        runCurrent()
        assertEquals(listOf(FinishCompletedRequest(listOf(4, 12), listOf(4))), sent)
        controller.finishCompleted(refreshed.finishPlan!!)
        runCurrent()
        assertEquals(FinishCompletedRequest(listOf(4, 12), listOf(12)), sent.last())
        assertEquals(2, sent.size)
        assertNull(controller.state.value)
    }

    @Test
    fun hungRefreshExpiresTheLastSnapshotUntilTheNextResponse() = runTest {
        val done = files[0].copy(verifiedBytes = files[0].size, complete = true)
        val pending = files[2].copy(verifiedBytes = 2, complete = false)
        val manifest = TorrentFiles(true, listOf(done, pending))
        val response = CompletableDeferred<TorrentFiles>()
        var reads = 0
        val controller = TorrentFilesController({ if (++reads == 1) manifest else response.await() }, { _, _ -> }, this)
        controller.open(download)
        runCurrent()
        assertTrue(controller.state.value!!.canSkip(pending.index))
        controller.refresh()
        advanceTimeBy(9_000)
        runCurrent()
        assertNull(controller.state.value!!.finishPlan)
        assertFalse(controller.state.value!!.canSkip(pending.index))
        response.complete(manifest)
        runCurrent()
        assertEquals(setOf(done.index), controller.state.value!!.finishPlan!!.completed)
        assertTrue(controller.state.value!!.canSkip(pending.index))
    }

    @Test
    fun skipRechecksWhetherTheChosenFileHasCompleted() = runTest {
        var manifest = TorrentFiles(true, files.map { it.copy(verifiedBytes = 0, complete = false) })
        val controller = TorrentFilesController({ manifest }, { _, _ -> }, this, skip = { _, _ -> error("Unexpected skip") })
        controller.open(download)
        runCurrent()
        assertTrue(controller.state.value!!.canSkip(4))
        manifest = manifest.copy(files = manifest.files.map { if (it.index == 4) it.copy(verifiedBytes = it.size, complete = true) else it })
        controller.skipFile(4)
        runCurrent()
        assertFalse(controller.state.value!!.canSkip(4))
        assertTrue(controller.state.value!!.error!!.contains("changed"))
    }

    @Test
    fun aProgressReadStartedBeforeSkipCannotRestoreTheSkippedChoice() = runTest {
        val old = files.map { it.copy(verifiedBytes = 0, complete = false) }
        val late = CompletableDeferred<TorrentFiles>()
        var reads = 0
        val controller = TorrentFilesController(load = {
            when (++reads) {
                1 -> TorrentFiles(true, old)
                2 -> withContext(NonCancellable) { late.await() }
                3 -> TorrentFiles(true, old) // The explicit skip rechecks this index before POST.
                else -> TorrentFiles(true, old.map { if (it.index == 12) it.copy(selected = false) else it })
            }
        }, save = { _, _ -> }, scope = this, skip = { _, _ -> })
        controller.open(download)
        runCurrent()
        controller.refresh()
        runCurrent()
        controller.skipFile(12)
        runCurrent()
        late.complete(TorrentFiles(true, old))
        runCurrent()
        assertFalse(12 in controller.state.value!!.selected)
        assertEquals(4, reads)
    }
}
