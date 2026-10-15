package com.thekozugroup.popcornstash.feature.downloads

import com.thekozugroup.popcornstash.data.api.Destination
import com.thekozugroup.popcornstash.data.api.Download
import com.thekozugroup.popcornstash.data.api.DownloadKind
import com.thekozugroup.popcornstash.data.api.DownloadState
import com.thekozugroup.popcornstash.data.api.Place
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.Locale

/** Downloads in the web app's words (web/src/components/downloads/downloadText.ts). */
class DownloadsTextTest {
    private val us = Locale.US
    private val s = DownloadsSamples

    @Test
    fun aDownloadingTorrentShowsPercentSpeedAndTimeLeft() {
        assertEquals(listOf("64%", "2.14 MB/s", "12 min left"), s.downloading.progressFacts(us))
    }

    @Test
    fun aTorrentWithoutPeersSaysItIsLookingForThem() {
        val lonely = s.downloading.copy(peers = 0, downBps = 0, etaS = null)
        assertEquals(listOf("64%", "Looking for peers"), lonely.progressFacts(us))
    }

    @Test
    fun aMagnetWithoutMetadataExplainsTheWaitInsteadOfSayingIdle() {
        val magnet = s.downloading.copy(totalBytes = null, doneBytes = 0, progress = 0.0, peers = null, downBps = 0, etaS = null)
        assertEquals(listOf("Fetching torrent details. Waiting for a peer."), magnet.progressFacts(us))
        assertEquals("Fetching torrent details", magnet.activeStatus())
        assertEquals("Paused", magnet.copy(state = DownloadState.Paused).activeStatus())
        assertEquals("Waiting for the VPN.", magnet.copy(state = DownloadState.Queued, error = "Waiting for the VPN.").activeStatus())
        assertEquals("Looking for peers", magnet.copy(totalBytes = 1024).activeStatus())
        assertEquals("Downloading", magnet.copy(totalBytes = 1024, downBps = 100).activeStatus())
        assertEquals(listOf("Paused"), magnet.copy(state = DownloadState.Paused).progressFacts(us))
        assertEquals(listOf("64%", "Looking for peers"), s.downloading.copy(peers = null, downBps = 0, etaS = null).progressFacts(us))
    }

    @Test
    fun aPausedDownloadSaysPausedWithWhatArrived() {
        assertEquals(listOf("Paused", "31%", "128 MB of 412 MB"), s.paused.progressFacts(us))
    }

    @Test
    fun aQueuedDownloadSaysWhyItWaitsInTheServersWords() {
        assertEquals(listOf("Waiting for a free slot (5 at once)."), s.queued.progressFacts(us))
        assertTrue(s.queued.isWaiting())
        assertEquals(listOf("Waiting to start."), s.queued.copy(error = null).progressFacts(us))
    }

    @Test
    fun aSeedingTorrentSaysItSharesBeforeSorting() {
        val seeding = s.downloading.copy(state = DownloadState.Seeding, progress = 1.0, upBps = 512_000)
        assertEquals(listOf("Downloaded. Sharing with peers before sorting.", "Sharing at 512 KB/s"), seeding.progressFacts(us))
    }

    @Test
    fun anUnknownSizeShowsWhatArrivedSoFarAndAnEmptyBar() {
        val unknown = Download(id = "x", kind = DownloadKind.Http, name = "x", state = DownloadState.Paused, doneBytes = 3_400_000)
        assertNull(unknown.knownProgress())
        assertEquals(listOf("Paused", "3.4 MB so far"), unknown.progressFacts(us))
    }

    @Test
    fun timesLeftReadAsTheWebSaysThemShorter() {
        assertNull(formatEta(null))
        assertEquals("Less than a minute left", formatEta(42))
        assertEquals("12 min left", formatEta(712))
        assertEquals("2 h 5 min left", formatEta(7_500))
        assertEquals("3 h left", formatEta(10_800))
        assertEquals("3 days left", formatEta(260_000))
    }

    @Test
    fun sizesUseDecimalUnits() {
        assertEquals("0 B", formatBytes(0, us))
        assertEquals("812 KB", formatBytes(812_000, us))
        assertEquals("4.7 GB", formatBytes(4_700_000_000, us))
        assertEquals("1.21 TB", formatBytes(1_210_000_000_000, us))
        assertEquals("Idle", formatSpeed(0, us))
    }

    @Test
    fun percentNeverRoundsUpToTheEnd() {
        assertEquals("99%", formatPercent(0.999))
        assertEquals("100%", formatPercent(1.0))
    }

    @Test
    fun finishedTimesAreRelative() {
        val now = s.NOW
        assertEquals("Just now", formatAgo(now - 10, now, us))
        assertEquals("3 min ago", formatAgo(now - 180, now, us))
        assertEquals("2 h ago", formatAgo(now - 7_300, now, us))
        assertEquals("Yesterday", formatAgo(now - 100_000, now, us))
        assertEquals("4 days ago", formatAgo(now - 4 * 86_400 - 5, now, us))
        assertEquals("Finished 3 min ago", s.sorted.finishedWhen(now, us))
        assertEquals("Started 3 days ago", s.failed.finishedWhen(now, us))
    }

    @Test
    fun aSortedDownloadNamesTheLibrariesItWentInto() {
        val outcome = s.sorted.finishedOutcome(s.libraries, isOwner = false)
        assertEquals(FinishedOutcome.Sorted(listOf(OutcomePlace("Movies", "lib-movies", "film"))), outcome)
        assertEquals("In Movies", outcome.text())
        assertEquals("In Home Videos and Other files", s.party.finishedOutcome(s.libraries, isOwner = true).text())
    }

    @Test
    fun filesStillWaitingAreCountedNotNamed() {
        assertEquals("2 waiting to import", s.waitingDownload.finishedOutcome(s.libraries, isOwner = true).text())
    }

    @Test
    fun aLibraryThePersonCannotSeeIsNeverNamed() {
        val hidden = s.sorted.copy(destinations = listOf(Destination(Place.Library, "lib-hidden", 1)))
        assertEquals("Sorted", hidden.finishedOutcome(s.libraries, isOwner = false).text())
        // The server already leaves out what a member cannot see: no destination reads "Sorted".
        assertEquals("Sorted", s.sorted.copy(destinations = emptyList()).finishedOutcome(s.libraries, isOwner = false).text())
    }

    @Test
    fun anOwnersEmptyDownloadSaysNothingCameIn() {
        val nothing = s.sorted.copy(destinations = emptyList(), itemIds = emptyList())
        assertEquals("No videos or photos inside. The files stay in Downloads/Completed.", nothing.finishedOutcome(s.libraries, true).text())
    }

    @Test
    fun failuresAndWaitsSayWhatHappened() {
        assertEquals(
            FinishedOutcome.Note("Failed. The server answered 404: the file is not there.", NoteTone.Error),
            s.failed.finishedOutcome(s.libraries, isOwner = true),
        )
        val completed = s.sorted.copy(state = DownloadState.Completed)
        assertEquals("Finished. Waiting in Downloads/Completed to be sorted.", completed.finishedOutcome(s.libraries, true).text())
        assertEquals("Finished. Waiting to be sorted.", completed.finishedOutcome(s.libraries, false).text())
        assertEquals("Sorting into your library.", s.sorted.copy(state = DownloadState.Sorting).finishedOutcome(s.libraries, true).text())
        assertEquals("Cancelled before it finished.", s.sorted.copy(state = DownloadState.Cancelled).finishedOutcome(s.libraries, true).text())
    }

    @Test
    fun onlyLinksAndMagnetsThatCanBeFetchedAgainAreRetried() {
        assertTrue(s.failed.canRetry())
        assertFalse(s.failed.copy(kind = DownloadKind.Upload).canRetry())
        assertFalse(s.failed.copy(source = "https://example.org/x#redacted").canRetry())
        assertFalse(s.sorted.canRetry())
    }

    @Test
    fun removingInProgressDeletesWhatArrivedButNeverSortedFiles() {
        assertTrue(removeDeletesFiles(s.downloading))
        assertTrue(removeDeletesFiles(s.failed))
        assertFalse(removeDeletesFiles(s.sorted))
        assertEquals("What was downloaded so far is deleted.", removeExplanation(s.paused))
        assertEquals("It leaves this list. The sorted files stay in your library.", removeExplanation(s.sorted))
        assertFalse(canRemove(s.sorted.copy(state = DownloadState.Completed)))
        assertFalse(canRemove(s.sorted.copy(state = DownloadState.Sorting)))
        assertTrue(canRemove(s.sorted))
    }

    @Test
    fun namesBreakAfterDotsUnderscoresAndHyphens() {
        assertEquals("a.​b_​c-​d", breakableName("a.b_c-d"))
        assertEquals("Harbor.​Lights.​2019.​1080p.​BluRay.x264", breakableName("Harbor.Lights.2019.1080p.BluRay.x264"))
    }

    @Test
    fun anExtensionStaysWithTheNameBeforeIt() {
        // Never "zip" or "mp4" alone on a line.
        assertEquals("Grandma 80th birthday.zip", breakableName("Grandma 80th birthday.zip"))
        assertEquals("harbor-​lights-​trailer.mp4", breakableName("harbor-lights-trailer.mp4"))
        assertEquals("PXL_​20261012_​181502.mp4 and 2 more", breakableName("PXL_20261012_181502.mp4 and 2 more"))
    }

    @Test
    fun anOlderFinishedDateStaysOnOneLine() {
        com.thekozugroup.popcornstash.testing.inEachZone(listOf("UTC")) {
            assertEquals("1 Oct", formatAgo(s.NOW - 13 * 86_400, s.NOW, us))
        }
    }
}
