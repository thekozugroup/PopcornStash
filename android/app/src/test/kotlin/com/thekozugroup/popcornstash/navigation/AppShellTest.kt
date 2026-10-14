package com.thekozugroup.popcornstash.navigation

import androidx.activity.ComponentActivity
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsNotSelected
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.getBoundsInRoot
import androidx.compose.ui.test.getUnclippedBoundsInRoot
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.longClick
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollToIndex
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

@RunWith(AndroidJUnit4::class)
@Config(qualifiers = "w411dp-h891dp-xxhdpi")
class AppShellTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private fun showShell(reduceMotion: Boolean = false, readyToImport: Int = 0) {
        compose.setContent {
            PopcornTheme(darkTheme = false, reduceMotion = reduceMotion) {
                AppShell(readyToImport = readyToImport) { page -> TestPage(page) }
            }
        }
    }

    /** Runs [count] frames of 16 ms with the clock held, so a transition can be looked at mid-way. */
    private fun advanceFrames(count: Int) = repeat(count) {
        compose.mainClock.advanceTimeByFrame()
        compose.waitForIdle()
    }

    private fun tab(tab: Tab) = compose.onNodeWithTag("tab ${tab.name}")
    private fun page(tab: Tab) = compose.onNodeWithTag("page ${tab.name}")
    /** Where a node is, in dp, including any part slid off screen. */
    private fun bounds(tag: String): Rect = compose.onNodeWithTag(tag).getUnclippedBoundsInRoot().let {
        Rect(it.left.value, it.top.value, it.right.value, it.bottom.value)
    }

    @Test
    fun startsOnHomeInTheCentre() {
        showShell()
        tab(Tab.Home).assertIsSelected()
        tab(Tab.Downloads).assertIsNotSelected()
        tab(Tab.Settings).assertIsNotSelected()
        page(Tab.Home).assertExists()
        // Downloads on the left, Home in the centre, Settings on the right.
        val downloads = bounds("tab Downloads")
        val home = bounds("tab Home")
        val settings = bounds("tab Settings")
        assertTrue(downloads.right <= home.left + 0.5f && home.right <= settings.left + 0.5f)
    }

    @Test
    fun everySquareIsATabWithItsNameAndPosition() {
        showShell()
        Tab.entries.forEachIndexed { index, tab ->
            compose.onNodeWithContentDescription(tab.label)
                .assert(SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Tab))
                .assert(
                    SemanticsMatcher("position ${index + 1} of 3") {
                        it.config.getOrNull(SemanticsProperties.CollectionItemInfo)?.columnIndex == index
                    },
                )
        }
        compose.onNodeWithTag("toolbar").assert(
            SemanticsMatcher("a collection of 3") {
                it.config.getOrNull(SemanticsProperties.CollectionInfo)?.columnCount == 3
            },
        )
    }

    @Test
    fun squaresAreAtLeastTheTouchMinimum() {
        showShell()
        Tab.entries.forEach { tab ->
            val size = tab(tab).getBoundsInRoot()
            assertTrue("${tab.name} is ${size.right - size.left} wide", size.right - size.left >= 48.dp)
            assertTrue("${tab.name} is ${size.bottom - size.top} tall", size.bottom - size.top >= 48.dp)
        }
    }

    @Test
    fun downloadsSaysHowManyFilesAreReadyToImport() {
        showShell(readyToImport = 3)
        compose.onNodeWithContentDescription("Downloads, 3 ready to import").assertExists()
        compose.onNodeWithContentDescription("Home").assertExists()
    }

    @Test
    fun tappingATabShowsItsPageAndMovesTheSelection() {
        showShell()
        tab(Tab.Settings).performClick()
        compose.waitForIdle()
        tab(Tab.Settings).assertIsSelected()
        tab(Tab.Home).assertIsNotSelected()
        page(Tab.Settings).assertExists()
        page(Tab.Home).assertDoesNotExist()
        // The blue square has slid under the tapped square.
        assertEquals(bounds("tab Settings").left, bounds("toolbar indicator").left, 0.5f)

        tab(Tab.Downloads).performClick()
        compose.waitForIdle()
        page(Tab.Downloads).assertExists()
        page(Tab.Settings).assertDoesNotExist()
        assertEquals(bounds("tab Downloads").left, bounds("toolbar indicator").left, 0.5f)
    }

    @Test
    fun aTabToTheRightSlidesInFromTheRight() {
        showShell()
        val width = bounds("page Home").width
        compose.mainClock.autoAdvance = false
        tab(Tab.Settings).performClick()
        advanceFrames(6)
        val incoming = bounds("page Settings")
        val outgoing = bounds("page Home")
        assertTrue("Settings enters from the right, at ${incoming.left}", incoming.left > 0f && incoming.left < width)
        assertTrue("Home leaves to the left, at ${outgoing.left}", outgoing.left < 0f)
        compose.mainClock.autoAdvance = true
        compose.waitForIdle()
        assertEquals(0f, bounds("page Settings").left, 0.5f)
    }

    @Test
    fun aTabToTheLeftSlidesInFromTheLeft() {
        showShell()
        compose.mainClock.autoAdvance = false
        tab(Tab.Downloads).performClick()
        advanceFrames(6)
        val incoming = bounds("page Downloads")
        val outgoing = bounds("page Home")
        assertTrue("Downloads enters from the left, at ${incoming.left}", incoming.left < 0f)
        assertTrue("Home leaves to the right, at ${outgoing.left}", outgoing.left > 0f)
        compose.mainClock.autoAdvance = true
        compose.waitForIdle()
        assertEquals(0f, bounds("page Downloads").left, 0.5f)
    }

    @Test
    fun theSlideLooksDoneInAboutAThirdOfASecond() {
        showShell()
        val width = bounds("page Home").width
        compose.mainClock.autoAdvance = false
        tab(Tab.Settings).performClick()
        // Two frames to take the tap and compose the new page, then about 300 ms of spring.
        advanceFrames(2 + 19)
        val left = bounds("page Settings").left
        assertTrue("Settings is still ${left}dp from its place", left < width * 0.03f)
        // The spring's invisible tail (under a pixel from its place) ends well within 0.8 s,
        // and the old page is let go.
        advanceFrames(28)
        page(Tab.Home).assertDoesNotExist()
        assertEquals(0f, bounds("page Settings").left, 0.5f)
    }

    @Test
    fun removeAnimationsCrossfadesInPlace() {
        showShell(reduceMotion = true)
        compose.mainClock.autoAdvance = false
        tab(Tab.Settings).performClick()
        advanceFrames(6)
        // Both pages are on screen at once, neither moved: a crossfade.
        assertEquals(0f, bounds("page Settings").left, 0.5f)
        assertEquals(0f, bounds("page Home").left, 0.5f)
        advanceFrames(14)
        page(Tab.Home).assertDoesNotExist()
    }

    @Test
    fun longPressShowsTheTabName() {
        showShell()
        compose.onAllNodesWithText("Downloads").assertCountEquals(0)
        tab(Tab.Downloads).performTouchInput { longClick() }
        compose.waitForIdle()
        compose.onNodeWithText("Downloads").assertExists()
        // A long press only names the tab; it does not switch to it.
        tab(Tab.Home).assertIsSelected()
    }

    @Test
    fun eachTabKeepsItsScrollAndStateWhileAnotherShows() {
        showShell()
        compose.onNodeWithTag("count Home").performClick()
        compose.onNodeWithText("Home taps: 1").assertExists()
        compose.onNodeWithTag("list Home").performScrollToIndex(40)
        tab(Tab.Settings).performClick()
        compose.waitForIdle()
        tab(Tab.Home).performClick()
        compose.waitForIdle()
        compose.onNodeWithText("Home row 40").assertExists()
        compose.onNodeWithText("Home row 0").assertDoesNotExist()
        compose.onNodeWithTag("list Home").performScrollToIndex(0)
        compose.onNodeWithText("Home taps: 1").assertExists()
    }

    @Test
    fun tappingTheCurrentTabScrollsToTheTop() {
        showShell()
        compose.onNodeWithTag("list Home").performScrollToIndex(40)
        tab(Tab.Home).performClick()
        compose.waitForIdle()
        compose.onNodeWithText("Home row 0").assertExists()
        tab(Tab.Home).assertIsSelected()
    }

    @Test
    fun backReturnsToHomeThenLeaves() {
        showShell()
        tab(Tab.Settings).performClick()
        compose.waitForIdle()
        compose.runOnUiThread { compose.activity.onBackPressedDispatcher.onBackPressed() }
        compose.waitForIdle()
        tab(Tab.Home).assertIsSelected()
        page(Tab.Home).assertExists()
        // On Home, Back is not handled by the shell, so the system closes the app.
        assertTrue(!compose.activity.onBackPressedDispatcher.hasEnabledCallbacks())
    }

    @Test
    fun theToolbarLeavesWhilePagesAskAndReturnsWhenTheLastIsGone() {
        var viewers by mutableIntStateOf(0)
        compose.setContent {
            PopcornTheme(darkTheme = false, reduceMotion = false) {
                AppShell { page ->
                    TestPage(page)
                    // Stand-ins for the photo viewer, on Home: each one shown asks the toolbar to leave.
                    if (page.tab == Tab.Home) repeat(viewers) { HideToolbarWhileShown() }
                }
            }
        }
        compose.onNodeWithTag("toolbar").assertExists()
        viewers = 2
        compose.waitForIdle()
        compose.onNodeWithTag("toolbar").assertDoesNotExist()
        viewers = 1
        compose.waitForIdle()
        compose.onNodeWithTag("toolbar").assertDoesNotExist()
        viewers = 0
        compose.waitForIdle()
        compose.onNodeWithTag("toolbar").assertExists()
    }

    @Test
    fun theSelectionSquareTravelsToTheTappedSquare() {
        showShell()
        val from = bounds("tab Home").left
        val to = bounds("tab Settings").left
        compose.mainClock.autoAdvance = false
        tab(Tab.Settings).performClick()
        advanceFrames(3)
        val moving = bounds("toolbar indicator").left
        assertTrue("The square is on its way, at $moving between $from and $to", moving > from + 0.5f && moving < to - 0.5f)
        compose.mainClock.autoAdvance = true
        compose.waitForIdle()
        assertEquals(to, bounds("toolbar indicator").left, 0.5f)
    }

    @Test
    fun underRemoveAnimationsTheSelectionSquareJumps() {
        showShell(reduceMotion = true)
        compose.mainClock.autoAdvance = false
        tab(Tab.Settings).performClick()
        advanceFrames(3)
        // Already under the tapped square while the pages are still crossfading.
        page(Tab.Home).assertExists()
        assertEquals(bounds("tab Settings").left, bounds("toolbar indicator").left, 0.5f)
    }

    @Test
    fun rightToLeftMirrorsTheToolbarAndTheSlides() {
        compose.setContent {
            CompositionLocalProvider(LocalLayoutDirection provides LayoutDirection.Rtl) {
                PopcornTheme(darkTheme = false, reduceMotion = false) {
                    AppShell { page -> TestPage(page) }
                }
            }
        }
        // Settings is on the left now, Downloads on the right.
        assertTrue(bounds("tab Settings").right <= bounds("tab Home").left + 0.5f)
        assertTrue(bounds("tab Home").right <= bounds("tab Downloads").left + 0.5f)
        compose.mainClock.autoAdvance = false
        tab(Tab.Settings).performClick()
        advanceFrames(6)
        val incoming = bounds("page Settings")
        val outgoing = bounds("page Home")
        assertTrue("Settings enters from the left, its side, at ${incoming.left}", incoming.left < 0f)
        assertTrue("Home leaves to the right, at ${outgoing.left}", outgoing.left > 0f)
        compose.mainClock.autoAdvance = true
        compose.waitForIdle()
        assertEquals(0f, bounds("page Settings").left, 0.5f)
        assertEquals(bounds("tab Settings").left, bounds("toolbar indicator").left, 0.5f)
    }

    @Test
    fun theSlideDirectionFollowsTheLayoutDirection() {
        assertEquals(1, slideDirection(Tab.Home, Tab.Settings))
        assertEquals(-1, slideDirection(Tab.Home, Tab.Settings, LayoutDirection.Rtl))
        assertEquals(1, slideDirection(Tab.Home, Tab.Downloads, LayoutDirection.Rtl))
        assertEquals(0, slideDirection(Tab.Home, Tab.Home, LayoutDirection.Rtl))
    }
}

/** A long list and a saveable counter, standing in for a tab's first page. */
@Composable
private fun TestPage(page: TabPage) {
    val state = rememberLazyListState()
    var taps by rememberSaveable { mutableIntStateOf(0) }
    LaunchedEffect(page.reselections) {
        if (page.reselections > 0) state.animateScrollToItem(0)
    }
    LazyColumn(state = state, modifier = Modifier.fillMaxSize().testTag("list ${page.tab.name}")) {
        item {
            Text(
                "${page.tab.name} taps: $taps",
                modifier = Modifier.testTag("count ${page.tab.name}").clickable { taps++ },
            )
        }
        items(100) { index -> Text("${page.tab.name} row $index") }
    }
}
