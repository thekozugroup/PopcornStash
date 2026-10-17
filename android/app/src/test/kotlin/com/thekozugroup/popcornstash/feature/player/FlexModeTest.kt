package com.thekozugroup.popcornstash.feature.player

import android.graphics.Rect
import androidx.activity.ComponentActivity
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.getUnclippedBoundsInRoot
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.doubleClick
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.window.layout.FoldingFeature
import androidx.window.layout.WindowLayoutInfo
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

@RunWith(AndroidJUnit4::class)
@Config(qualifiers = "w673dp-h841dp-mdpi")
class FlexModeTest {
    @get:Rule val compose = createAndroidComposeRule<ComponentActivity>()

    @Test fun tabletopKeepsMediaAboveTheHingeAndWorkingControlsBelow() {
        var hinge by mutableStateOf<FlexHinge?>(FlexHinge(410, 430))
        var toggles = 0
        var seek = 0L
        var visible = false
        compose.setContent {
            PlayerTheme {
                PlayerScreen(PlayerSamples.episode.copy(playWhenReady = true), { PlayerSamples.clock },
                    PlayerActions(onTogglePlay = { toggles++ }, onSeek = { seek = it }),
                    chromeVisible = visible, onChromeVisibleChange = { visible = it }, flexHinge = hinge,
                ) { SampleFrame(it) }
            }
        }
        val video = compose.onNodeWithTag("flex video").getUnclippedBoundsInRoot()
        val controls = compose.onNodeWithTag("flex controls").getUnclippedBoundsInRoot()
        assertEquals(410.dp, video.bottom)
        assertEquals(430.dp, controls.top)
        assertTrue(compose.onNodeWithTag("play pause").getUnclippedBoundsInRoot().top >= controls.top)
        compose.onNodeWithContentDescription("Pause").performClick()
        compose.onNodeWithTag("seek bar").performSemanticsAction(SemanticsActions.SetProgress) { it(0.5f) }
        assertEquals(1, toggles)
        assertEquals(1568000L, seek)
        compose.onNodeWithTag("player tap area").performTouchInput { doubleClick(Offset(width * .9f, height * .5f)) }
        assertEquals(PlayerSamples.clock.positionMs + 15_000L, seek)
        compose.mainClock.advanceTimeBy(10000)
        compose.onNodeWithTag("play pause").assertExists()
        compose.runOnIdle { hinge = null }
        compose.onNodeWithTag("flex video").assertDoesNotExist()
        compose.onNodeWithTag("flex controls").assertDoesNotExist()
    }

    @Test fun pictureInPictureAlwaysUsesTheWholePicture() {
        compose.setContent {
            PlayerTheme {
                PlayerScreen(PlayerSamples.episode, { PlayerSamples.clock }, PlayerActions(), true, {},
                    inPictureInPicture = true, flexHinge = FlexHinge(410, 430),
                ) { SampleFrame(it) }
            }
        }
        compose.onNodeWithTag("flex controls").assertDoesNotExist()
        compose.onNodeWithTag("flex video").assertDoesNotExist()
    }

    @Test fun flatAndBookPosturesDoNotSplitPlayback() {
        fun feature(state: FoldingFeature.State, orientation: FoldingFeature.Orientation) = object : FoldingFeature {
            override val bounds = Rect(0, 410, 673, 430)
            override val state = state
            override val orientation = orientation
            override val isSeparating = true
            override val occlusionType = FoldingFeature.OcclusionType.FULL
        }
        assertNull(WindowLayoutInfo(emptyList()).flexHinge())
        assertNull(WindowLayoutInfo(listOf(feature(FoldingFeature.State.FLAT, FoldingFeature.Orientation.HORIZONTAL))).flexHinge())
        assertNull(WindowLayoutInfo(listOf(feature(FoldingFeature.State.HALF_OPENED, FoldingFeature.Orientation.VERTICAL))).flexHinge())
        assertEquals(FlexHinge(410, 430), WindowLayoutInfo(listOf(feature(FoldingFeature.State.HALF_OPENED, FoldingFeature.Orientation.HORIZONTAL))).flexHinge())
    }
}
