package com.thekozugroup.popcornstash.navigation

import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Text
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.getUnclippedBoundsInRoot
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.unit.LayoutDirection
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/** A page opening inside a tab comes in from the end side: the right, or the left in a right-to-left layout. */
@RunWith(AndroidJUnit4::class)
@Config(qualifiers = "w411dp-h891dp-xxhdpi")
class PageStackHostTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private fun enteringLeft(direction: LayoutDirection): Float {
        val stack = PageStack.of("first")
        compose.setContent {
            CompositionLocalProvider(LocalLayoutDirection provides direction) {
                PopcornTheme(darkTheme = false, reduceMotion = false) {
                    PageStackHost(stack, ViewModelStores(), "test") { entry ->
                        Box(Modifier.fillMaxSize().testTag("page ${entry.route}")) { Text(entry.route) }
                    }
                }
            }
        }
        compose.mainClock.autoAdvance = false
        compose.runOnIdle { stack.push("second") }
        repeat(3) {
            compose.mainClock.advanceTimeByFrame()
            compose.waitForIdle()
        }
        val left = compose.onNodeWithTag("page second").getUnclippedBoundsInRoot().left.value
        compose.mainClock.autoAdvance = true
        compose.waitForIdle()
        assertEquals(0f, compose.onNodeWithTag("page second").getUnclippedBoundsInRoot().left.value, 0.5f)
        return left
    }

    @Test
    fun aPageOpensFromTheRight() {
        val left = enteringLeft(LayoutDirection.Ltr)
        assertTrue("enters from the right, at $left", left > 0.5f)
    }

    @Test
    fun aPageOpensFromTheLeftInARightToLeftLayout() {
        val left = enteringLeft(LayoutDirection.Rtl)
        assertTrue("enters from the left, at $left", left < -0.5f)
    }
}
