package com.thekozugroup.popcornstash.navigation

import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Text
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.unit.DpRect
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.getUnclippedBoundsInRoot
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import com.thekozugroup.popcornstash.ui.theme.ToolbarTokens
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

@RunWith(AndroidJUnit4::class)
@Config(qualifiers = "w411dp-h891dp-xxhdpi")
class AdaptiveNavigationTest {
    private fun DpRect.rect() = Rect(left.value, top.value, right.value, bottom.value)
    @get:Rule val compose = createAndroidComposeRule<ComponentActivity>()
    private val tokens = ToolbarTokens()

    private fun show(direction: LayoutDirection = LayoutDirection.Ltr, reduceMotion: Boolean = true) {
        compose.setContent {
            CompositionLocalProvider(LocalLayoutDirection provides direction) {
                PopcornTheme(darkTheme = false, reduceMotion = reduceMotion) {
                    AppShell { page -> Box(Modifier.fillMaxSize().testTag("content ${page.tab.name}")) { Text(page.tab.label) } }
                }
            }
        }
    }

    private fun assertBottom() {
        val bar = compose.onNodeWithTag("toolbar").getUnclippedBoundsInRoot().rect()
        val root = compose.onRoot().getUnclippedBoundsInRoot().rect()
        assertEquals(tokens.thickness.value, bar.height, .5f)
        assertEquals(tokens.length(3).value, bar.width, .5f)
        assertEquals(root.center.x, bar.center.x, .5f)
        val items = Tab.entries.map { compose.onNodeWithTag("tab ${it.name}").getUnclippedBoundsInRoot().rect() }
        assertEquals(items[0].top, items[1].top, .5f)
        assertEquals(items[1].top, items[2].top, .5f)
        assertEquals(items[1].left - items[0].left, items[2].left - items[1].left, .5f)
        items.forEach { assertTrue(it.width >= 48f && it.height >= 48f) }
        compose.onNodeWithTag("toolbar").assert(SemanticsMatcher("one row of three tabs") {
            val info = it.config[SemanticsProperties.CollectionInfo]
            info.rowCount == 1 && info.columnCount == 3
        })
    }

    @Test fun phoneUsesEvenlySpacedBottomNavigation() { show(); assertBottom() }

    @Test fun bottomNavigationKeepsIndicatorAlignedInRtl() {
        show(LayoutDirection.Rtl)
        assertBottom()
        compose.onNodeWithTag("tab Settings").performClick().assertIsSelected()
        val item = compose.onNodeWithTag("tab Settings").getUnclippedBoundsInRoot().rect()
        val indicator = compose.onNodeWithTag("toolbar indicator", useUnmergedTree = true).getUnclippedBoundsInRoot().rect()
        assertEquals(item.left, indicator.left, .5f)
    }

    @Test fun bottomPagesFollowTheTappedSideInRtl() {
        show(LayoutDirection.Rtl, reduceMotion = false)
        val origin = compose.onNodeWithTag("content Home").getUnclippedBoundsInRoot().rect()
        compose.mainClock.autoAdvance = false
        compose.onNodeWithTag("tab Settings").performClick()
        repeat(6) { compose.mainClock.advanceTimeByFrame(); compose.waitForIdle() }
        val entering = compose.onNodeWithTag("content Settings").getUnclippedBoundsInRoot().rect()
        val leaving = compose.onNodeWithTag("content Home").getUnclippedBoundsInRoot().rect()
        assertTrue("Settings enters from its square on the left", entering.left < origin.left - 1f)
        assertTrue("Home leaves to the right", leaving.left > origin.left + 1f)
        assertEquals(origin.top, entering.top, .5f)
        compose.mainClock.autoAdvance = true
        compose.waitForIdle()
        assertEquals(origin.left, compose.onNodeWithTag("content Settings").getUnclippedBoundsInRoot().rect().left, .5f)
    }

    @Test @Config(qualifiers = "w891dp-h411dp-land-xxhdpi")
    fun landscapePhoneKeepsBottomNavigation() { show(); assertBottom() }

    @Test @Config(qualifiers = "w540dp-h840dp-xxhdpi")
    fun tabletSplitScreenUsesBottomNavigation() { show(); assertBottom() }

    @Test @Config(qualifiers = "w673dp-h841dp-xxhdpi")
    fun unfoldedDisplayUsesRightRailAndReservesContentSpace() {
        show()
        val bar = compose.onNodeWithTag("toolbar").getUnclippedBoundsInRoot().rect()
        val content = compose.onNodeWithTag("content Home").getUnclippedBoundsInRoot().rect()
        assertEquals(tokens.thickness.value, bar.width, .5f)
        assertEquals(tokens.length(3).value, bar.height, .5f)
        assertTrue(content.right <= bar.left - tokens.margin.value)
    }

    @Test @Config(qualifiers = "w1000dp-h1200dp-xxhdpi")
    fun resizingKeepsSelectedTabAndSavedPageState() {
        val compact = mutableStateOf(false)
        compose.setContent {
            PopcornTheme(darkTheme = false, reduceMotion = true) {
                Box(Modifier.width(if (compact.value) 411.dp else 840.dp).height(900.dp)) {
                    AppShell { page ->
                        val count = rememberSaveable { mutableStateOf(0) }
                        androidx.compose.material3.Button(onClick = { count.value++ }) { Text("${page.tab.label}: ${count.value}") }
                    }
                }
            }
        }
        compose.onNodeWithTag("tab Settings").performClick()
        compose.onNodeWithText("Settings: 0").performClick()
        compose.runOnIdle { compact.value = true }
        compose.onNodeWithTag("tab Settings").assertIsSelected()
        compose.onNodeWithText("Settings: 1").assertExists()
        assertEquals(tokens.thickness.value, compose.onNodeWithTag("toolbar").getUnclippedBoundsInRoot().rect().height, .5f)
    }

    @Test fun breakpointsUseAvailableWidthAndHeight() {
        assertEquals(NavigationPlacement.Bottom, navigationPlacement(599.dp, 1000.dp))
        assertEquals(NavigationPlacement.Right, navigationPlacement(600.dp, 480.dp))
        assertEquals(NavigationPlacement.Bottom, navigationPlacement(900.dp, 479.dp))
    }
}
