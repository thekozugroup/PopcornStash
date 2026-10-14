package com.thekozugroup.popcornstash.feature.player

import androidx.activity.ComponentActivity
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performSemanticsAction
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/** The player's chrome on Robolectric: what it says to TalkBack, what each control does, when it hides. */
@RunWith(AndroidJUnit4::class)
class PlayerScreenTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val done = mutableListOf<String>()
    private var chromeVisible by mutableStateOf(true)

    private val actions = PlayerActions(
        onClose = { done += "close" },
        onTogglePlay = { done += "toggle" },
        onSeek = { done += "seek $it" },
        onChooseAudio = { done += "audio $it" },
        onChooseQuality = { done += "quality $it" },
        onPlayNext = { done += "next" },
        onPlayAgain = { done += "again" },
        onRetry = { done += "retry" },
    )

    private fun show(state: PlayerUiState, inPictureInPicture: Boolean = false) {
        compose.setContent {
            PlayerTheme {
                PlayerScreen(
                    state = state,
                    clock = { PlayerSamples.clock },
                    actions = actions,
                    chromeVisible = chromeVisible,
                    onChromeVisibleChange = { chromeVisible = it },
                    inPictureInPicture = inPictureInPicture,
                ) { SampleFrame(it) }
            }
        }
        compose.waitForIdle()
    }

    @Test
    fun theControlsSayWhatTheyDo() {
        show(PlayerSamples.episode)
        compose.onNodeWithContentDescription("Close player").performClick()
        compose.onNodeWithContentDescription("Play").performClick()
        compose.onNodeWithText("The Lantern Keepers").assertIsDisplayed()
        compose.onNodeWithText("S01E01 · Pilot").assertIsDisplayed()
        compose.onNodeWithContentDescription("Position").assert(
            SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, "12 minutes 4 seconds of 52 minutes 16 seconds"),
        )
        compose.onNodeWithContentDescription("Next episode: S01E02 · The lantern keepers").performClick()
        assertEquals(listOf("close", "toggle", "next"), done)
    }

    @Test
    fun playingShowsPauseAndSeekingGoesToThePlaceChosen() {
        show(PlayerSamples.episode.copy(playWhenReady = true))
        compose.onNodeWithContentDescription("Pause").assertIsDisplayed()
        compose.onNodeWithTag("seek bar").performSemanticsAction(SemanticsActions.SetProgress) { it(0.5f) }
        compose.waitForIdle()
        assertEquals(listOf("seek 1568000"), done)
    }

    @Test
    fun theMenuOffersAudioAndQualityAndSaysWhenTheVideoIsConverted() {
        show(PlayerSamples.episode)
        compose.onNodeWithContentDescription("Audio and quality").performClick()
        compose.onNodeWithText("Audio").assertIsDisplayed()
        compose.onNodeWithText("Commentary (English)").assertIsDisplayed()
        compose.onNodeWithText("Quality").assertIsDisplayed()
        compose.onNodeWithText("Original").assertIsDisplayed()
        compose.onNodeWithText("Converted for this phone while you watch").assertIsDisplayed()
        compose.onNodeWithText("720p").performClick()
        compose.onNodeWithContentDescription("Audio and quality").performClick()
        compose.onNodeWithText("Commentary (English)").performClick()
        assertEquals(listOf("quality 720", "audio 2"), done)
    }

    @Test
    fun aVideoWithOneTrackAndNothingSmallerHasNoMenu() {
        show(PlayerSamples.film)
        compose.onNodeWithContentDescription("Close player").assertIsDisplayed()
        assertTrue(compose.onAllNodesWithDescription("Audio and quality").isEmpty())
        assertTrue(compose.onAllNodesWithDescription("Quality").isEmpty())
    }

    @Test
    fun loadingShowsTheTitleAndAWaitingDiscThatDoesNothing() {
        show(PlayerSamples.loading)
        compose.onNodeWithContentDescription("Loading video").performClick()
        compose.onNodeWithText("The Lantern Keepers").assertIsDisplayed()
        assertTrue("no seek bar before the stream", compose.onAllNodesWithDescription("Position").isEmpty())
        assertTrue(done.isEmpty())
    }

    @Test
    fun theEndOffersTheNextEpisodeAndBackButStartsNothing() {
        show(PlayerSamples.episode.copy(phase = PlayerPhase.Ended))
        compose.onNodeWithText("Next: S01E02 · The lantern keepers").assertIsDisplayed()
        compose.onNodeWithText("Play next episode").performClick()
        compose.onNodeWithText("Back").performClick()
        assertEquals(listOf("next", "close"), done)
    }

    @Test
    fun theEndOfAFilmOffersToPlayItAgain() {
        show(PlayerSamples.film.copy(phase = PlayerPhase.Ended))
        compose.onNodeWithText("Play again").performClick()
        assertEquals(listOf("again"), done)
    }

    @Test
    fun aVideoThatIsGoneOffersOnlyBack() {
        show(PlayerSamples.episode.copy(phase = PlayerPhase.Failed(PlayerController.GONE, gone = true)))
        compose.onNodeWithText("This video is no longer available.").assertIsDisplayed()
        assertTrue(compose.onAllNodesWithText("Try again").isEmpty())
        compose.onNodeWithText("Back").performClick()
        assertEquals(listOf("close"), done)
    }

    @Test
    fun aStoppedStreamSaysWhyAndOffersTryAgain() {
        show(PlayerSamples.episode.copy(phase = PlayerPhase.Failed("The stream stopped. Check the connection and try again.", gone = false)))
        compose.onNodeWithText("Playback stopped.").assertIsDisplayed()
        compose.onNodeWithText("The stream stopped. Check the connection and try again.").assertIsDisplayed()
        compose.onNodeWithText("Try again").performClick()
        assertEquals(listOf("retry"), done)
    }

    @Test
    fun whilePlayingTheChromeFadesAfterThreeSecondsAndATapBringsItBack() {
        compose.mainClock.autoAdvance = false
        show(PlayerSamples.episode.copy(playWhenReady = true))
        compose.mainClock.advanceTimeBy(2_000)
        assertTrue(chromeVisible)
        compose.mainClock.advanceTimeBy(1_200)
        assertFalse(chromeVisible)
        compose.mainClock.advanceTimeBy(500)
        assertTrue("the close button left with the chrome", compose.onAllNodesWithDescription("Close player").isEmpty())

        // A tap on the picture.
        compose.onNodeWithTag("player").performClick()
        compose.mainClock.advanceTimeBy(500)
        assertTrue(chromeVisible)
        compose.onNodeWithContentDescription("Close player").assertIsDisplayed()
    }

    @Test
    fun pausedTheChromeStays() {
        compose.mainClock.autoAdvance = false
        show(PlayerSamples.episode)
        compose.mainClock.advanceTimeBy(10_000)
        assertTrue(chromeVisible)
    }

    @Test
    fun bufferingWithTheChromeAwayStillShowsTheWaitingDisc() {
        chromeVisible = false
        show(PlayerSamples.episode.copy(playWhenReady = true, buffering = true))
        compose.onNodeWithTag("play pause").assert(SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, "Loading"))
        assertTrue(compose.onAllNodesWithDescription("Close player").isEmpty())
    }

    @Test
    fun pictureInPictureShowsOnlyThePicture() {
        show(PlayerSamples.episode.copy(playWhenReady = true), inPictureInPicture = true)
        assertTrue(compose.onAllNodesWithDescription("Close player").isEmpty())
        assertTrue(compose.onAllNodesWithDescription("Pause").isEmpty())
        assertTrue(compose.onAllNodesWithDescription("Position").isEmpty())
    }

    private fun androidx.compose.ui.test.junit4.ComposeTestRule.onAllNodesWithDescription(label: String) =
        onAllNodes(SemanticsMatcher.expectValue(SemanticsProperties.ContentDescription, listOf(label))).fetchSemanticsNodes()

    private fun androidx.compose.ui.test.junit4.ComposeTestRule.onAllNodesWithText(text: String) =
        onAllNodes(androidx.compose.ui.test.hasText(text)).fetchSemanticsNodes()
}
