package com.thekozugroup.popcornstash.screenshots

import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.Box
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onRoot
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.github.takahirom.roborazzi.captureRoboImage
import com.github.takahirom.roborazzi.captureScreenRoboImage
import com.thekozugroup.popcornstash.feature.player.PlayerActions
import com.thekozugroup.popcornstash.feature.player.PlayerClock
import com.thekozugroup.popcornstash.feature.player.PlayerController
import com.thekozugroup.popcornstash.feature.player.PlayerPhase
import com.thekozugroup.popcornstash.feature.player.PlayerSamples
import com.thekozugroup.popcornstash.feature.player.PlayerScreen
import com.thekozugroup.popcornstash.feature.player.PlayerTheme
import com.thekozugroup.popcornstash.feature.player.PlayerUiState
import com.thekozugroup.popcornstash.feature.player.SampleFrame
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/** The same phone held sideways, as the player shows it. */
const val PHONE_LANDSCAPE = "w891dp-h411dp-land-xxhdpi"

/**
 * The player as a phone shows it, sideways. The player is always dark (android/DESIGN.md,
 * Player), so the system's light and dark give the same picture; `player-paused-system-dark`
 * is there to show it.
 */
@RunWith(AndroidJUnit4::class)
@Config(qualifiers = PHONE_LANDSCAPE)
class PlayerScreenshotTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private fun player(
        state: PlayerUiState,
        chromeVisible: Boolean = true,
        menuOpen: Boolean = false,
        frame: Boolean = true,
        ratio: Float = 16f / 9f,
        clock: PlayerClock = PlayerSamples.clock,
        inPictureInPicture: Boolean = false,
    ) {
        // The waiting ring turns for ever: hold the clock so it is caught at the same place every run.
        compose.mainClock.autoAdvance = false
        compose.setContent {
            PlayerTheme {
                PlayerScreen(
                    state = state,
                    clock = { clock },
                    actions = PlayerActions(),
                    chromeVisible = chromeVisible,
                    onChromeVisibleChange = {},
                    menuInitiallyOpen = menuOpen,
                    inPictureInPicture = inPictureInPicture,
                ) { modifier -> if (frame) SampleFrame(modifier, ratio) else Box(modifier) }
            }
        }
        compose.mainClock.advanceTimeBy(SETTLE_MS)
        compose.waitForIdle()
    }

    private fun shoot(name: String) = compose.onRoot().captureRoboImage(screenshot(name))

    @Test
    fun loading() {
        player(PlayerSamples.loading, frame = false)
        shoot("player-loading")
    }

    @Test
    fun paused() {
        player(PlayerSamples.episode)
        shoot("player-paused")
    }

    @Test
    @Config(qualifiers = "+night")
    fun pausedWithTheSystemDark() {
        player(PlayerSamples.episode)
        shoot("player-paused-system-dark")
    }

    @Test
    fun audioAndQualityMenu() {
        player(PlayerSamples.episode, menuOpen = true)
        // The menu is a popup window of its own: capture the whole screen.
        captureScreenRoboImage(screenshot("player-menu"))
    }

    @Test
    fun bufferingWithTheChromeAway() {
        player(PlayerSamples.episode.copy(playWhenReady = true, buffering = true), chromeVisible = false)
        shoot("player-buffering")
    }

    @Test
    fun filmPlaying() {
        player(PlayerSamples.film.copy(playWhenReady = true), ratio = 2.4f)
        shoot("player-film-playing")
    }

    @Test
    fun endWithTheNextEpisode() {
        player(PlayerSamples.episode.copy(phase = PlayerPhase.Ended))
        shoot("player-ended-next")
    }

    @Test
    fun endOfAFilm() {
        player(PlayerSamples.film.copy(phase = PlayerPhase.Ended), ratio = 2.4f)
        shoot("player-ended")
    }

    @Test
    fun videoGone() {
        player(PlayerSamples.episode.copy(phase = PlayerPhase.Failed(PlayerController.GONE, gone = true)))
        shoot("player-gone")
    }

    @Test
    fun streamStopped() {
        player(
            PlayerSamples.episode.copy(
                phase = PlayerPhase.Failed("The stream stopped. Check the connection and try again.", gone = false),
            ),
        )
        shoot("player-error")
    }

    @Test
    @Config(qualifiers = PHONE)
    fun uprightPhoneVideo() {
        player(PlayerSamples.upright, ratio = 9f / 16f, clock = PlayerSamples.uprightClock)
        shoot("player-upright")
    }

    @Test
    @Config(qualifiers = "w320dp-h180dp-land-xxhdpi")
    fun pictureInPicture() {
        player(PlayerSamples.episode.copy(playWhenReady = true, buffering = true), inPictureInPicture = true)
        shoot("player-picture-in-picture")
    }

    private companion object {
        /** Long enough for fades to finish and the ring to show a clear arc. */
        const val SETTLE_MS = 700L
    }
}
