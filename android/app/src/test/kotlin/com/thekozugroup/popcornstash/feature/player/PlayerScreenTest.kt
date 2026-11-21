package com.thekozugroup.popcornstash.feature.player

import android.graphics.Bitmap
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
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.doubleClick
import androidx.compose.ui.test.click
import androidx.compose.ui.test.swipe
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performSemanticsAction
import androidx.lifecycle.Lifecycle
import androidx.test.ext.junit.runners.AndroidJUnit4
import coil3.ImageLoader
import coil3.asImage
import coil3.intercept.Interceptor
import coil3.request.ImageResult
import coil3.request.SuccessResult
import com.thekozugroup.popcornstash.privacy.PrivateContent
import com.thekozugroup.popcornstash.ui.media.LocalStashImages
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import java.util.concurrent.CopyOnWriteArrayList
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/** The player's chrome on Robolectric: what it says to TalkBack, what each control does, when it hides. */
@RunWith(AndroidJUnit4::class)
@Config(qualifiers = "w891dp-h411dp-land-mdpi")
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
    fun doubleTappingThePictureSeeksFifteenSecondsInEitherDirection() {
        show(PlayerSamples.episode)
        compose.onNodeWithTag("player tap area").performTouchInput { doubleClick(Offset(width * .1f, height * .5f)) }
        compose.onNodeWithText("−15 seconds").assertIsDisplayed()
        compose.onNodeWithTag("player tap area").performTouchInput { doubleClick(Offset(width * .9f, height * .5f)) }
        assertEquals(listOf("seek ${PlayerSamples.clock.positionMs - 15_000}", "seek ${PlayerSamples.clock.positionMs + 15_000}"), done)
    }

    @Test
    fun doubleTapStopsAtTheBeginningAndEndWithoutChangingPlayState() {
        compose.setContent {
            PlayerTheme {
                PlayerScreen(PlayerSamples.episode.copy(durationMs = 10_000), { PlayerClock(5_000, 10_000) }, actions, true, {}) { SampleFrame(it) }
            }
        }
        compose.onNodeWithTag("player tap area").performTouchInput { doubleClick(Offset(width * .1f, height * .5f)) }
        compose.onNodeWithTag("player tap area").performTouchInput { doubleClick(Offset(width * .9f, height * .5f)) }
        assertEquals(listOf("seek 0", "seek 10000"), done)
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

    @Test fun tappingTheTrackJumpsImmediatelyWithoutTogglingPlayback() {
        show(PlayerSamples.film)
        compose.onNodeWithTag("seek bar").performTouchInput { click(Offset(width * .75f, centerY)) }
        compose.runOnIdle {
            val target = done.single().removePrefix("seek ").toLong()
            assertTrue(target in 4_700_000..5_000_000)
        }
    }

    @Test fun holdingTheTrackPreviewsBeforeMovingAndCommitsOnceOnRelease() {
        show(PlayerSamples.film.copy(itemId = "seek-fixture"))
        val bar = compose.onNodeWithTag("seek bar")
        bar.performTouchInput { down(Offset(width * .75f, centerY)) }
        compose.onNodeWithTag("seek preview", useUnmergedTree = true).assertIsDisplayed()
        compose.runOnIdle { assertTrue(done.isEmpty()) }

        bar.performTouchInput { up() }
        compose.runOnIdle {
            assertTrue(done.single().removePrefix("seek ").toLong() in 4_700_000..5_000_000)
        }
        compose.onNodeWithTag("seek preview", useUnmergedTree = true).assertDoesNotExist()
    }

    @Test fun cancellingAHeldPressDismissesThePreviewWithoutSeeking() {
        show(PlayerSamples.film.copy(itemId = "seek-fixture"))
        val bar = compose.onNodeWithTag("seek bar")
        bar.performTouchInput { down(center) }
        compose.onNodeWithTag("seek preview", useUnmergedTree = true).assertIsDisplayed()
        bar.performTouchInput { cancel() }
        compose.onNodeWithTag("seek preview", useUnmergedTree = true).assertDoesNotExist()
        compose.runOnIdle { assertTrue(done.isEmpty()) }
    }

    @Test fun cancellingAMovingDragDiscardsTheDraftAndAllowsTheNextSeek() {
        show(PlayerSamples.film.copy(itemId = "seek-fixture"))
        val bar = compose.onNodeWithTag("seek bar")
        val original = bar.fetchSemanticsNode().config[SemanticsProperties.ProgressBarRangeInfo].current
        bar.performTouchInput {
            down(Offset(width * .25f, centerY))
            moveTo(Offset(width * .75f, centerY))
        }
        compose.onNodeWithTag("seek preview", useUnmergedTree = true).assertIsDisplayed()
        assertTrue(bar.fetchSemanticsNode().config[SemanticsProperties.ProgressBarRangeInfo].current > original)
        bar.performTouchInput { cancel() }
        compose.onNodeWithTag("seek preview", useUnmergedTree = true).assertDoesNotExist()
        assertEquals(original, bar.fetchSemanticsNode().config[SemanticsProperties.ProgressBarRangeInfo].current, 0f)
        compose.runOnIdle { assertTrue(done.isEmpty()) }
        bar.performSemanticsAction(SemanticsActions.SetProgress) { it(.5f) }
        compose.runOnIdle { assertEquals(listOf("seek 3266000"), done) }
    }

    @Test fun draggingShowsAPreviewAndDoesNotSeekUntilRelease() {
        show(PlayerSamples.film.copy(itemId = "seek-fixture"))
        compose.onNodeWithTag("seek bar").performTouchInput {
            down(Offset(width * .3f, centerY))
            moveTo(Offset(width * .6f, centerY))
        }
        compose.onNodeWithTag("seek preview", useUnmergedTree = true).assertIsDisplayed()
        compose.runOnIdle { assertTrue(done.isEmpty()) }
        compose.onNodeWithTag("seek bar").performTouchInput { up() }
        compose.runOnIdle { assertEquals(1, done.size); assertTrue(done.single().startsWith("seek ")) }
        assertTrue(compose.onAllNodesWithTag("seek preview").fetchSemanticsNodes().isEmpty())
    }

    private fun previewImages(onRequest: suspend (String) -> Unit): ImageLoader {
        val image = Bitmap.createBitmap(16, 9, Bitmap.Config.ARGB_8888).asImage()
        return ImageLoader.Builder(compose.activity).coroutineContext(Dispatchers.Unconfined)
            .components {
                add(object : Interceptor {
                    override suspend fun intercept(chain: Interceptor.Chain): ImageResult {
                        onRequest(chain.request.data.toString())
                        return SuccessResult(image, chain.request)
                    }
                })
            }.build()
    }

    @Test
    @Config(qualifiers = "w393dp-h851dp-mdpi", fontScale = 1.5f)
    fun rapidScrubbingOnlyRequestsTheFirstAndLatestFrameInRtlPortraitWithLargeText() {
        val samples = CopyOnWriteArrayList<Long>()
        val firstFrame = CompletableDeferred<Unit>()
        val images = previewImages { url ->
            samples += url.substringAfter("time_s=").toLong()
            firstFrame.await()
        }
        val duration = 60_000L
        try {
            compose.setContent {
                CompositionLocalProvider(LocalLayoutDirection provides LayoutDirection.Rtl, LocalStashImages provides images) {
                    PlayerTheme {
                        PlayerScreen(PlayerSamples.film.copy(itemId = "seek-fixture", durationMs = duration),
                            { PlayerClock(10_000, duration) }, actions, true, {}) { SampleFrame(it) }
                    }
                }
            }
            val bar = compose.onNodeWithTag("seek bar")
            val preview = compose.onNodeWithTag("seek preview", useUnmergedTree = true)
            bar.performTouchInput { down(Offset(width * .25f, centerY)) }
            compose.waitForIdle()
            preview.assertIsDisplayed()
            compose.runOnIdle { assertTrue("samples=$samples", samples.single() in 42L..50L) }

            for (fraction in listOf(.5f, .65f, .85f)) {
                bar.performTouchInput { moveTo(Offset(width * fraction, centerY)) }
                compose.waitForIdle()
            }
            val lastFraction = bar.fetchSemanticsNode().config[SemanticsProperties.ProgressBarRangeInfo].current
            val lastSample = previewSecond((lastFraction * duration).toLong(), duration)
            compose.runOnIdle {
                assertTrue(done.isEmpty())
                assertEquals(1, samples.size)
                assertTrue(lastSample < samples.first())
                firstFrame.complete(Unit)
            }
            compose.waitForIdle()
            compose.runOnIdle { assertEquals(listOf(samples.first(), lastSample), samples.toList()) }
            bar.performTouchInput { up() }
            compose.waitForIdle()
            preview.assertDoesNotExist()
            compose.runOnIdle { assertEquals(listOf("seek ${(lastFraction * duration).toLong()}"), done) }
        } finally {
            firstFrame.complete(Unit)
            images.shutdown()
        }
    }

    @Test fun revisitingAFrameReusesItOnlyWithinTheSameUnlockedForegroundItem() {
        val requests = CopyOnWriteArrayList<String>()
        val images = previewImages { requests += it.substringAfter("/api/items/") }
        var itemId by mutableStateOf("clip-a")
        var unlocked by mutableStateOf(true)
        try {
            compose.setContent {
                PrivateContent(unlocked = unlocked, locked = {}) {
                    CompositionLocalProvider(LocalStashImages provides images) {
                        PlayerTheme {
                            PlayerScreen(PlayerSamples.film.copy(itemId = itemId, durationMs = 60_000),
                                { PlayerClock(10_000, 60_000) }, actions, true, {}) { SampleFrame(it) }
                        }
                    }
                }
            }
            fun press(fraction: Float) {
                val bar = compose.onNodeWithTag("seek bar")
                bar.performTouchInput { down(Offset(width * fraction, centerY)) }
                compose.onNodeWithTag("seek preview", useUnmergedTree = true).assertIsDisplayed()
                bar.performTouchInput { up() }
                compose.onNodeWithTag("seek preview", useUnmergedTree = true).assertDoesNotExist()
            }

            press(.25f)
            val first = requests.single()
            press(.75f)
            val second = requests.last()
            assertTrue(first != second)
            press(.25f)
            assertEquals(listOf(first, second), requests.toList())

            compose.runOnIdle { itemId = "clip-b" }
            press(.25f)
            assertEquals(first.replace("clip-a/", "clip-b/"), requests.last())
            compose.runOnIdle { itemId = "clip-a" }
            press(.25f)

            compose.activityRule.scenario.moveToState(Lifecycle.State.STARTED)
            compose.activityRule.scenario.moveToState(Lifecycle.State.RESUMED)
            press(.25f)
            compose.runOnIdle { unlocked = false }
            compose.onNodeWithTag("seek bar").assertDoesNotExist()
            compose.runOnIdle { unlocked = true }
            press(.25f)
            assertEquals(listOf(first, second, first.replace("clip-a/", "clip-b/"), first, first, first), requests.toList())
        } finally {
            images.shutdown()
        }
    }

    @Test fun tappingTheThumbKeepsItsPositionAndDraggingStillSeeks() {
        show(PlayerSamples.film)
        compose.onNodeWithTag("seek thumb", useUnmergedTree = true).performTouchInput { click(center) }
        compose.runOnIdle { assertTrue("actions=$done", done.single().removePrefix("seek ").toLong() in 700_000..750_000) }
        compose.onNodeWithTag("seek bar").performTouchInput { swipe(Offset(width * .2f, centerY), Offset(width * .6f, centerY), 300) }
        compose.runOnIdle { assertTrue("actions=$done", done.last().removePrefix("seek ").toLong() in 3_700_000..4_100_000) }
    }

    @Test fun unknownDurationDoesNotSeek() {
        show(PlayerSamples.film.copy(durationMs = 0))
        compose.onNodeWithTag("seek bar").performTouchInput { click(Offset(width * .75f, centerY)) }
        compose.runOnIdle { assertTrue(done.isEmpty()) }
    }

    @Test fun rightToLeftTrackTapsMatchTheThumbDirection() {
        compose.setContent {
            CompositionLocalProvider(LocalLayoutDirection provides LayoutDirection.Rtl) {
                PlayerTheme { PlayerScreen(PlayerSamples.film, { PlayerSamples.clock }, actions, true, {}) { SampleFrame(it) } }
            }
        }
        compose.onNodeWithTag("seek bar").performTouchInput { click(Offset(width * .25f, centerY)) }
        compose.runOnIdle { assertTrue(done.single().removePrefix("seek ").toLong() in 4_700_000..5_000_000) }
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
    @Test fun browseDrawerOpensWithSwipeAndSelectsVideo() {
        var selected: String? = null
        compose.setContent { PlayerTheme {
            PlayerScreen(PlayerSamples.episode.copy(recommendations = listOf(PlayerItems.film.item)), { PlayerSamples.clock },
                PlayerActions(onPlayVideo = { selected = it }), true, {}) { SampleFrame(it) }
        } }
        compose.onNodeWithTag("more videos").performTouchInput { swipe(Offset(centerX, bottom - 2), Offset(centerX, top + 2), 300) }
        compose.onNodeWithText("Paper Boats").assertIsDisplayed().performClick()
        compose.runOnIdle { assertEquals(PlayerItems.film.id, selected) }
    }

}
