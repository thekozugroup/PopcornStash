package com.thekozugroup.popcornstash.feature.library

import androidx.test.ext.junit.runners.AndroidJUnit4
import com.thekozugroup.popcornstash.data.api.Suggestion
import com.thekozugroup.popcornstash.feature.downloads.DownloadsSamples
import com.thekozugroup.popcornstash.feature.downloads.ImportTarget
import com.thekozugroup.popcornstash.feature.downloads.initialMoveForm
import com.thekozugroup.popcornstash.feature.downloads.previewMove
import com.thekozugroup.popcornstash.testing.SampleMedia
import com.thekozugroup.popcornstash.testing.inEachZone
import com.thekozugroup.popcornstash.testing.plain
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Test
import org.junit.runner.RunWith
import java.time.LocalDateTime
import java.time.ZoneOffset
import java.util.Locale

/**
 * A capture time is the camera's clock (docs/architecture.md section 4, Capture time): the
 * phone shows `taken_local` as it is, and months as the server sends them, whatever time zone
 * the phone is set to. Each test runs on Robolectric with the phone's default zone set to
 * places on both sides of UTC, and builds its formatter the way the app does (the phone's zone).
 */
@RunWith(AndroidJUnit4::class)
class CaptureTimeTest {
    /** 20:00 on 30 June in Los Angeles: already 1 July in UTC, and later still in Tokyo. */
    private val lateJune = SampleMedia.homeVideo("v-june", "Fireworks practice", daysAgo = 0, seconds = 95).copy(
        takenAt = LocalDateTime.of(2026, 6, 30, 20, 0).toEpochSecond(ZoneOffset.ofHours(-7)),
        takenLocal = "2026-06-30T20:00:00",
        takenOffset = -420,
    )

    /** A photo still named after its camera file, taken at 23:30 on New Year's Eve with no offset written. */
    private val newYearsEve = SampleMedia.photo("p-nye", daysAgo = 0).copy(
        title = "IMG_0001",
        relPath = "Photos/2026/12/IMG_0001.jpg",
        takenAt = LocalDateTime.of(2026, 12, 31, 23, 30).toEpochSecond(ZoneOffset.UTC),
        takenLocal = "2026-12-31T23:30:00",
        takenOffset = null,
    )

    @Test
    fun aCaptureTimeReadsAsTheCameraClockInEveryZone() = inEachZone { zone ->
        val format = MediaFormat(Locale.US)
        val lisbon = SampleMedia.lisbonSunset
        assertEquals(zone, "Photo from Sep 12, 2026", plain(format.displayTitle(lisbon)))
        assertEquals(zone, "Taken at 6:05 PM", plain(format.detailLine(lisbon)))
        assertEquals(zone, "Taken Sep 12, 2026, 6:05 PM", plain(format.detailLine(lisbon.copy(title = "Sunset at Belem"))))
        assertEquals(zone, "Sep 12, 2026, 6:05 PM", plain(format.dateTime(lisbon.capturedAt!!)))
        assertEquals(zone, "Jun 30, 2026 · 1 min", plain(format.tileLine(lateJune)))
        assertEquals(zone, "Recorded Jun 30, 2026, 8:00 PM · 1 min", plain(format.detailLine(lateJune)))
        assertEquals(zone, "Photo from Dec 31, 2026", plain(format.displayTitle(newYearsEve)))
        assertEquals(zone, "Taken at 11:30 PM", plain(format.detailLine(newYearsEve)))
    }

    @Test
    fun monthsAreTheServersMonthsInEveryZone() = inEachZone { zone ->
        val format = MediaFormat(Locale.US)
        assertEquals(zone, "June 2026", plain(format.month("2026-06")))
        assertEquals(zone, "July 2026", plain(format.month("2026-07")))
        assertEquals(zone, "January 2027", plain(format.month("2027-01")))
    }

    @Test
    fun aWaitingFileIsFiledByTheMonthOnItsCameraClockInEveryZone() = inEachZone { zone ->
        val photo = DownloadsSamples.photo
        val target = ImportTarget(DownloadsSamples.photos)
        // 21:30 on 30 September in Los Angeles is 1 October in UTC: still September's folder.
        assertEquals(zone, "Photos/2026/09/", previewMove(target, initialMoveForm(photo, target), photo, showFolder = true).folder)
        // The suggestion's clock comes first: it is where the server would file it.
        val suggested = photo.copy(
            suggestion = photo.suggestion!!.copy(
                takenLocal = "2026-08-31T23:59:00",
                takenAt = LocalDateTime.of(2026, 8, 31, 23, 59).toEpochSecond(ZoneOffset.ofHours(-7)),
                takenOffset = -420,
            ),
        )
        assertEquals(zone, "Photos/2026/08/", previewMove(target, initialMoveForm(suggested, target), suggested, showFolder = true).folder)
    }

    @Test
    fun anOlderServersCaptureTimeIsReadInUtcInEveryZone() = inEachZone { zone ->
        // Before taken_local, the server kept a camera's clock in taken_at as if it were UTC.
        val older = lateJune.copy(takenLocal = null, takenOffset = null, takenAt = LocalDateTime.of(2026, 6, 30, 20, 0).toEpochSecond(ZoneOffset.UTC))
        assertEquals(zone, "Recorded Jun 30, 2026, 8:00 PM · 1 min", plain(MediaFormat(Locale.US).detailLine(older)))
        assertEquals(zone, LocalDateTime.of(2026, 6, 30, 20, 0), older.capturedAt)
        val suggestion = Suggestion(takenAt = LocalDateTime.of(2026, 9, 30, 23, 0).toEpochSecond(ZoneOffset.UTC))
        assertEquals(zone, LocalDateTime.of(2026, 9, 30, 23, 0), captureClock(suggestion.takenLocal, suggestion.takenAt))
    }

    @Test
    fun anAddedDateIsAMomentInThePhonesZone() {
        // The contrast that proves the zone really changes: 02:00 UTC on 1 July is still 30 June in Los Angeles.
        val added = LocalDateTime.of(2026, 7, 1, 2, 0).toEpochSecond(ZoneOffset.UTC)
        val days = mutableMapOf<String, String>()
        inEachZone(listOf("America/Los_Angeles", "Asia/Tokyo")) { zone -> days[zone] = plain(MediaFormat(Locale.US).date(added)) }
        assertEquals("Jun 30, 2026", days["America/Los_Angeles"])
        assertEquals("Jul 1, 2026", days["Asia/Tokyo"])
        assertNotEquals(days["America/Los_Angeles"], days["Asia/Tokyo"])
        val undated = lateJune.copy(takenAt = null, takenLocal = null, takenOffset = null, addedAt = added)
        inEachZone(listOf("America/Los_Angeles")) { assertEquals("Added Jun 30, 2026 · 1 min", plain(MediaFormat(Locale.US).detailLine(undated))) }
    }
}
