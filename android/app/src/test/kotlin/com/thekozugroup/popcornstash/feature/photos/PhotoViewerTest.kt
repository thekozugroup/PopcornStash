package com.thekozugroup.popcornstash.feature.photos

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.getUnclippedBoundsInRoot
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.unit.DpRect
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.thekozugroup.popcornstash.testing.SampleMedia
import com.thekozugroup.popcornstash.testing.WithSampleImages
import com.thekozugroup.popcornstash.ui.theme.PlayerTokens
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

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
}
