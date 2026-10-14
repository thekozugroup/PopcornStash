package com.thekozugroup.popcornstash.feature.photos

import androidx.activity.ComponentActivity
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHostState
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.getUnclippedBoundsInRoot
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.unit.DpRect
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.thekozugroup.popcornstash.testing.SampleMedia
import com.thekozugroup.popcornstash.testing.WithSampleImages
import com.thekozugroup.popcornstash.testing.plain
import com.thekozugroup.popcornstash.ui.theme.PlayerTokens
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import kotlin.math.roundToInt
import org.robolectric.annotation.Config
import java.util.TimeZone

/**
 * The viewer's chrome follows the player's rules (android/DESIGN.md, Player and photo viewer):
 * the close disc is the player's disc, and every control keeps the same edge from the screen.
 */
@RunWith(AndroidJUnit4::class)
@Config(qualifiers = "w411dp-h891dp-xxhdpi")
class PhotoViewerTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val tokens = PlayerTokens()

    private fun bounds(description: String): DpRect =
        compose.onNodeWithContentDescription(description).getUnclippedBoundsInRoot()

    @Test
    fun everyControlKeepsThePlayersEdge() {
        compose.setContent {
            PopcornTheme(darkTheme = true, reduceMotion = true) {
                WithSampleImages {
                    PhotoViewer(
                        photos = SampleMedia.timeline.flatMap { it.items },
                        startId = "s2",
                        onClose = {},
                        onDetails = {},
                        onPlay = {},
                        onSaveOriginal = {},
                    )
                }
            }
        }
        compose.waitForIdle()
        val chrome = compose.onNodeWithTag("viewer chrome").getUnclippedBoundsInRoot()
        val close = bounds("Close")
        // The close disc is the player's 52dp disc, 16dp in from the top and the start.
        assertEquals(tokens.disc.value, (close.right - close.left).value, 0.5f)
        assertEquals(tokens.disc.value, (close.bottom - close.top).value, 0.5f)
        assertEquals((chrome.left + tokens.edge).value, close.left.value, 0.5f)
        assertEquals((chrome.top + tokens.edge).value, close.top.value, 0.5f)
        // The plate with the date, Save the original and Details: the same 16dp from the sides and the bottom.
        val plate = compose.onNodeWithTag("viewer plate").getUnclippedBoundsInRoot()
        assertEquals((chrome.left + tokens.edge).value, plate.left.value, 0.5f)
        assertEquals((chrome.right - tokens.edge).value, plate.right.value, 0.5f)
        assertEquals((chrome.bottom - tokens.edge).value, plate.bottom.value, 0.5f)
    }

    /** The viewer's own messages sit on top of its plate, not over Save the original or Details. */
    @Test
    fun aMessageSitsOnTopOfThePlate() {
        val snackbar = SnackbarHostState()
        compose.setContent {
            PopcornTheme(darkTheme = false, reduceMotion = true) {
                WithSampleImages {
                    PhotoViewer(
                        photos = SampleMedia.timeline.flatMap { it.items },
                        startId = "s2",
                        onClose = {},
                        onDetails = {},
                        onPlay = {},
                        onSaveOriginal = {},
                        snackbar = snackbar,
                    )
                    LaunchedEffect(Unit) { snackbar.showSnackbar("Original saved.", duration = SnackbarDuration.Indefinite) }
                }
            }
        }
        compose.waitForIdle()
        compose.onNodeWithText("Original saved.").assertExists()
        // The visible snackbar, not its host: as wide as the plate and 8dp above it.
        val note = compose.onNodeWithTag("viewer snackbar").getUnclippedBoundsInRoot()
        val plate = compose.onNodeWithTag("viewer plate").getUnclippedBoundsInRoot()
        assertEquals("The snackbar ends 8dp above the plate", plate.top.value - 8f, note.bottom.value, 0.5f)
        assertEquals("Its left edge is the plate's", plate.left.value, note.left.value, 0.5f)
        assertEquals("Its right edge is the plate's", plate.right.value, note.right.value, 0.5f)
        // Those bounds are the visible surface: 2dp inside its left and right edges is the light
        // snackbar's zinc-50, where Material's own 12dp of padding would show the black stage.
        val pixels = compose.onRoot().captureToImage().toPixelMap()
        val density = compose.density.density
        val y = ((note.top.value + note.bottom.value) / 2 * density).roundToInt()
        for (x in listOf(note.left.value + 2f, note.right.value - 2f)) {
            val pixel = pixels[(x * density).roundToInt(), y]
            assertTrue("The snackbar is zinc-50 at ${x}dp, not $pixel", pixel.red > 0.95f && pixel.green > 0.95f && pixel.blue > 0.95f)
        }
    }

    @Test
    fun thePlateShowsTheCameraClockWhateverThePhonesZone() {
        // 18:05 in Lisbon is 07:05 the next morning on Kiritimati (UTC+14), where this phone is.
        val before = TimeZone.getDefault()
        TimeZone.setDefault(TimeZone.getTimeZone("Pacific/Kiritimati"))
        try {
            compose.setContent {
                PopcornTheme(darkTheme = true, reduceMotion = true) {
                    WithSampleImages {
                        PhotoViewer(
                            photos = listOf(SampleMedia.lisbonSunset),
                            startId = SampleMedia.lisbonSunset.id,
                            onClose = {},
                            onDetails = {},
                            onPlay = {},
                            onSaveOriginal = {},
                        )
                    }
                }
            }
            compose.waitForIdle()
            val line = compose.onNode(hasText("6:05", substring = true), useUnmergedTree = true)
                .fetchSemanticsNode().config[SemanticsProperties.Text].joinToString()
            assertEquals("Sep 12, 2026, 6:05 PM", plain(line))
        } finally {
            TimeZone.setDefault(before)
        }
    }
}
