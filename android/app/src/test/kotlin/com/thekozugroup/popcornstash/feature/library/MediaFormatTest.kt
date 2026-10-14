package com.thekozugroup.popcornstash.feature.library

import com.thekozugroup.popcornstash.data.api.AudioStream
import com.thekozugroup.popcornstash.data.api.Layout
import com.thekozugroup.popcornstash.data.api.Progress
import com.thekozugroup.popcornstash.data.api.SubtitleStream
import com.thekozugroup.popcornstash.data.api.VideoStream
import com.thekozugroup.popcornstash.testing.SampleMedia
import com.thekozugroup.popcornstash.ui.components.TileShape
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import java.time.ZoneId
import java.util.Locale

/** The words and numbers of the library pages, the same as the web app's. */
class MediaFormatTest {
    private val format = MediaFormat(Locale.US, ZoneId.of("UTC"))

    @Test
    fun durationsReadLikeTheWeb() {
        assertEquals("1 h 42 min", format.duration(6120.0))
        assertEquals("2 h", format.duration(7200.0))
        assertEquals("42 min", format.duration(2520.0))
        assertEquals("38 s", format.duration(38.0))
        assertEquals("", format.duration(null))
        assertEquals("1 hour 42 minutes", format.spokenDuration(6120.0))
        assertEquals("1 minute", format.spokenDuration(60.0))
        assertEquals("less than a minute", format.spokenDuration(20.0))
    }

    @Test
    fun sizesAreDecimal() {
        assertEquals("592 B", format.bytes(592))
        assertEquals("192 KB", format.bytes(192_400))
        assertEquals("1.24 GB", format.bytes(1_240_000_000))
        assertEquals("12.5 MB", format.bytes(12_500_000))
    }

    @Test
    fun clocksAndMonths() {
        assertEquals("39:31", format.clock(2371.2))
        assertEquals("1:02:05", format.clock(3725.0))
        assertEquals("0:08", format.clock(8.9))
        assertEquals("June 2024", format.month("2024-06"))
        assertEquals("not a month", format.month("not a month"))
    }

    @Test
    fun timeLeftAndTheWatchedBar() {
        assertEquals("10 min left", format.timeLeft(SampleMedia.lantern[1]))
        assertEquals("Less than a minute left", format.timeLeft(SampleMedia.lantern[1].copy(progress = Progress(1430.0, 1440.0))))
        assertEquals(0.55f, format.watchedFraction(SampleMedia.lantern[1].progress)!!, 0.001f)
        assertNull("a finished video has no bar", format.watchedFraction(Progress(1440.0, 1440.0, finished = true)))
        assertNull("a glance is not worth a bar", format.watchedFraction(Progress(5.0, 1440.0)))
    }

    @Test
    fun tileLinesByKind() {
        assertEquals("2011 · 1 h 44 min", format.tileLine(SampleMedia.paperBoats))
        assertEquals("S01E02 · 24 min", format.tileLine(SampleMedia.lantern[1]))
        assertEquals("Sep 11, 2026 · 3 min", format.tileLine(SampleMedia.beachDay))
        assertEquals("PDF · 192 KB", format.tileLine(SampleMedia.otherFiles[0]))
        assertEquals("MP3 · 3 min", format.tileLine(SampleMedia.otherFiles[1]))
    }

    @Test
    fun namesPeopleKnowItemsBy() {
        assertEquals("Fog over the pier", format.displayTitle(SampleMedia.lantern[1]))
        assertEquals("Episode 5", MediaFormat.episodeName(SampleMedia.lantern[4]))
        val camera = SampleMedia.recentPhotos[0]
        assertEquals("Photo from ${format.date(camera.takenAt!!)}", format.displayTitle(camera))
        assertEquals("Paper Boats", format.displayTitle(SampleMedia.paperBoats))
        assertEquals("S01E02", MediaFormat.episodeCode(1, 2))
        assertEquals("Specials", MediaFormat.seasonName(0))
        assertEquals("Season 3", MediaFormat.seasonName(3))
    }

    @Test
    fun detailLinesByKind() {
        assertEquals("2011 · 1 h 44 min", format.detailLine(SampleMedia.paperBoats))
        assertEquals("Season 1, episode 2 · 24 min", format.detailLine(SampleMedia.lantern[1]))
        assertEquals("Special 1 · 24 min", format.detailLine(SampleMedia.episode("x", "Show", 0, 1, null)))
        assertEquals("Taken at ${format.time(SampleMedia.recentPhotos[0].takenAt!!)}", format.detailLine(SampleMedia.recentPhotos[0]))
    }

    @Test
    fun mediaFactsInWords() {
        assertEquals("1080p, H.264", format.videoSummary(VideoStream(0, "h264", width = 1920, height = 1040)))
        assertEquals("4K HDR10, HEVC", format.videoSummary(VideoStream(0, "hevc", width = 3840, height = 2160, hdr = "hdr10")))
        assertEquals(
            "English, Dolby Digital Plus 5.1, 1 more",
            format.audioSummary(listOf(AudioStream(1, "eac3", 6, "eng"), AudioStream(2, "aac", 2, "por"))),
        )
        assertEquals("English, Spanish", format.subtitleSummary(listOf(SubtitleStream(3, "subrip", "eng"), SubtitleStream(4, "subrip", "spa"))))
        assertNull(format.languageName("und"))
    }

    @Test
    fun librariesByLayout() {
        assertEquals(TileShape.Poster, Layout.Posters.tileShape())
        assertEquals(TileShape.Poster, Layout.Series.tileShape())
        assertEquals(TileShape.Frame, Layout.Frames.tileShape())
        assertEquals(TileShape.Square, Layout.Timeline.tileShape())
        assertEquals("48 videos", format.libraryCountLine(SampleMedia.movies))
        assertEquals("6 shows", format.libraryCountLine(SampleMedia.shows))
        assertEquals("3,214 photos", format.libraryCountLine(SampleMedia.photos))
        assertEquals("New movies land here on their own.", emptyLibraryLine(SampleMedia.movies))
        assertEquals("Files you accept from Downloads into Kids show up here.", emptyLibraryLine(SampleMedia.kids))
        assertEquals("movies, TV episodes and photos", joinWords(listOf("movies", "TV episodes", "photos")))
    }
}
