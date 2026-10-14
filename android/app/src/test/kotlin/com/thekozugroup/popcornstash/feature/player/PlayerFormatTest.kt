package com.thekozugroup.popcornstash.feature.player

import com.thekozugroup.popcornstash.data.api.Progress
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import java.util.Locale

class PlayerFormatTest {
    @Test
    fun clockReadsLikeTheWebPlayers() {
        assertEquals("0:00", PlayerFormat.clock(0.0))
        assertEquals("0:05", PlayerFormat.clock(5.9))
        assertEquals("12:04", PlayerFormat.clock(724.0))
        assertEquals("52:16", PlayerFormat.clock(3136.0))
        assertEquals("1:02:03", PlayerFormat.clock(3723.0))
        assertEquals("0:00", PlayerFormat.clock(Double.NaN))
        assertEquals("0:00", PlayerFormat.clock(-4.0))
    }

    @Test
    fun talkBackHearsDurationsInWords() {
        assertEquals("12 minutes 4 seconds", PlayerFormat.spokenDuration(724.0))
        assertEquals("1 hour 2 minutes 3 seconds", PlayerFormat.spokenDuration(3723.0))
        assertEquals("1 minute", PlayerFormat.spokenDuration(60.0))
        assertEquals("0 seconds", PlayerFormat.spokenDuration(0.0))
    }

    @Test
    fun resumeFollowsTheWebsRule() {
        val item = PlayerItems.episodeOne
        assertEquals(724.0, PlayerFormat.resumePosition(item), 0.0)
        assertEquals("the first 15 seconds start again", 0.0, PlayerFormat.resumePosition(item.copy(progress = Progress(14.0, 3136.0))), 0.0)
        assertEquals("the last 2 seconds start again", 0.0, PlayerFormat.resumePosition(item.copy(progress = Progress(3134.5, 3136.0))), 0.0)
        assertEquals(0.0, PlayerFormat.resumePosition(item.copy(progress = Progress(900.0, 3136.0, finished = true))), 0.0)
        assertEquals(0.0, PlayerFormat.resumePosition(item.copy(progress = null)), 0.0)
        assertEquals("the item's own length when progress has none", 0.0, PlayerFormat.resumePosition(item.copy(progress = Progress(3135.0, 0.0))), 0.0)
    }

    @Test
    fun theTitleBesideCloseNamesTheShowThenTheEpisode() {
        assertEquals("The Lantern Keepers" to "S01E01 · Pilot", PlayerFormat.titleLines(PlayerItems.episodeOne))
        assertEquals("The Lantern Keepers" to "S01E03", PlayerFormat.titleLines(PlayerItems.episodeOne.copy(episode = 3, episodeTitle = null)))
        assertEquals("Paper Boats" to null, PlayerFormat.titleLines(PlayerItems.film))
        assertEquals("S01E02 · The lantern keepers", PlayerFormat.nextLabel(PlayerItems.episodeTwo))
        assertEquals("Paper Boats", PlayerFormat.nextLabel(PlayerItems.film))
        assertNull(PlayerFormat.episodeCode(PlayerItems.film))
    }

    @Test
    fun languagesAreNamedInThePhonesLanguage() {
        assertEquals("English", PlayerFormat.languageName("eng", Locale.ENGLISH))
        assertEquals("German", PlayerFormat.languageName("ger", Locale.ENGLISH))
        assertEquals("Deutsch", PlayerFormat.languageName("deu", Locale.GERMAN))
        assertEquals("XYZ", PlayerFormat.languageName("xyz", Locale.ENGLISH))
        assertNull(PlayerFormat.languageName("und", Locale.ENGLISH))
        assertNull(PlayerFormat.languageName(null, Locale.ENGLISH))
    }

    @Test
    fun resolutionNamesMatchTheWeb() {
        assertEquals("4K", PlayerFormat.resolutionName(3840, 1608))
        assertEquals("1080p", PlayerFormat.resolutionName(1920, 800))
        assertEquals("720p", PlayerFormat.resolutionName(1280, 720))
        assertEquals("180p", PlayerFormat.resolutionName(320, 180))
        assertNull(PlayerFormat.resolutionName(null, 180))
    }
}
