package com.thekozugroup.popcornstash.navigation

import android.view.ViewGroup
import androidx.activity.ComponentActivity
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsNotSelected
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.getBoundsInRoot
import androidx.compose.ui.test.getUnclippedBoundsInRoot
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.longClick
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollToIndex
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.toSize
import androidx.core.graphics.Insets
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.thekozugroup.popcornstash.ui.components.GlassTier
import com.thekozugroup.popcornstash.ui.components.TabSnackbarHost
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import com.thekozugroup.popcornstash.ui.theme.ToolbarTokens
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import kotlin.math.abs
import kotlin.math.roundToInt
import org.robolectric.annotation.Config

/**
 * The shell (android/DESIGN.md, Navigation): the upright toolbar centered on the left edge, its
 * selection square, the vertical tab slides, Back, the page clearance and the snackbars
 * beside the bar.
 */
@RunWith(AndroidJUnit4::class)
@Config(qualifiers = "w840dp-h1100dp-xxhdpi")
class AppShellTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val tokens = ToolbarTokens()

    private fun showShell(
        reduceMotion: Boolean = false,
        readyToImport: Int = 0,
        dark: Boolean = false,
        layoutDirection: LayoutDirection = LayoutDirection.Ltr,
    ) {
        compose.setContent {
            CompositionLocalProvider(LocalLayoutDirection provides layoutDirection) {
                PopcornTheme(darkTheme = dark, reduceMotion = reduceMotion) {
                    AppShell(readyToImport = readyToImport) { page -> TestPage(page) }
                }
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
    private fun bounds(tag: String, unmerged: Boolean = false): Rect =
        compose.onNodeWithTag(tag, useUnmergedTree = unmerged).getUnclippedBoundsInRoot().let {
            Rect(it.left.value, it.top.value, it.right.value, it.bottom.value)
        }

    private fun screen(): Rect = compose.onRoot().getUnclippedBoundsInRoot().let {
        Rect(it.left.value, it.top.value, it.right.value, it.bottom.value)
    }

    @Test
    fun startsOnHomeInTheMiddleOfACenteredBarOnTheLeftEdge() {
        showShell()
        tab(Tab.Home).assertIsSelected()
        tab(Tab.Downloads).assertIsNotSelected()
        tab(Tab.Settings).assertIsNotSelected()
        page(Tab.Home).assertExists()
        // Downloads at the top, Home in the middle, Settings at the bottom, in one column.
        val downloads = bounds("tab Downloads")
        val home = bounds("tab Home")
        val settings = bounds("tab Settings")
        assertTrue(downloads.bottom <= home.top + 0.5f && home.bottom <= settings.top + 0.5f)
        assertEquals(downloads.left, home.left, 0.5f)
        assertEquals(home.left, settings.left, 0.5f)
        // The rail is inset from the physical left and centered in the safe height.
        val bar = bounds("toolbar")
        val screen = screen()
        assertEquals(screen.left + tokens.margin.value, bar.left, 0.5f)
        assertEquals(screen.center.y, bar.center.y, 0.5f)
        assertEquals(tokens.thickness.value, bar.width, 0.5f)
        assertEquals(tokens.length(Tab.entries.size).value, bar.height, 0.5f)
    }

    @Test
    fun everySquareIsATabWithItsNameAndPositionInOneColumn() {
        showShell()
        Tab.entries.forEachIndexed { index, tab ->
            compose.onNodeWithContentDescription(tab.label)
                .assert(SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Tab))
                .assert(
                    SemanticsMatcher("position ${index + 1} of 4, top to bottom") {
                        val info = it.config.getOrNull(SemanticsProperties.CollectionItemInfo)
                        info?.rowIndex == index && info.columnIndex == 0 && info.rowSpan == 1 && info.columnSpan == 1
                    },
                )
        }
        compose.onNodeWithTag("toolbar").assert(
            SemanticsMatcher("a selectable group of 4 rows in 1 column") {
                val info = it.config.getOrNull(SemanticsProperties.CollectionInfo)
                info?.rowCount == Tab.entries.size && info.columnCount == 1 && it.config.contains(SemanticsProperties.SelectableGroup)
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

    /** The badge sits on the Downloads square's top-left corner, away from the screen edge, inside the screen. */
    @Test
    fun theBadgeSitsOnTheTopLeftCorner() = badgeOnTheTopLeft(LayoutDirection.Ltr)

    @Test
    fun theBadgeStaysOnTheTopLeftInARightToLeftLayout() = badgeOnTheTopLeft(LayoutDirection.Rtl)

    private fun badgeOnTheTopLeft(layoutDirection: LayoutDirection) {
        showShell(readyToImport = 3, layoutDirection = layoutDirection)
        val square = bounds("tab Downloads")
        // Inside the Downloads square's own semantics, so only in the unmerged tree.
        val badge = bounds("toolbar badge", unmerged = true)
        assertTrue("The badge starts left of the square, at ${badge.left} for ${square.left}", badge.left < square.left)
        assertTrue("The badge ends over the square's left half, at ${badge.right}", badge.right > square.left && badge.right < square.center.x)
        assertTrue("The badge rises above the square, at ${badge.top} for ${square.top}", badge.top < square.top)
        assertTrue("The badge stays inside the screen", badge.right < screen().right - tokens.margin.value)
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
        // The blue square has slid down under the tapped square.
        assertEquals(bounds("tab Settings").top, bounds("toolbar indicator").top, 0.5f)
        assertEquals(bounds("tab Settings").left, bounds("toolbar indicator").left, 0.5f)

        tab(Tab.Downloads).performClick()
        compose.waitForIdle()
        page(Tab.Downloads).assertExists()
        page(Tab.Settings).assertDoesNotExist()
        assertEquals(bounds("tab Downloads").top, bounds("toolbar indicator").top, 0.5f)
    }

    @Test
    fun aTabBelowRisesFromTheBottom() {
        showShell()
        val height = bounds("page Home").height
        compose.mainClock.autoAdvance = false
        tab(Tab.Settings).performClick()
        advanceFrames(6)
        val incoming = bounds("page Settings")
        val outgoing = bounds("page Home")
        assertTrue("Settings rises from below, at ${incoming.top}", incoming.top > 0f && incoming.top < height)
        assertTrue("Home leaves upwards, at ${outgoing.top}", outgoing.top < 0f)
        assertEquals("No sideways move", tokens.besideClearance.value, incoming.left, 0.5f)
        compose.mainClock.autoAdvance = true
        compose.waitForIdle()
        assertEquals(0f, bounds("page Settings").top, 0.5f)
    }

    @Test
    fun aTabAboveComesDownFromTheTop() {
        showShell()
        compose.mainClock.autoAdvance = false
        tab(Tab.Downloads).performClick()
        advanceFrames(6)
        val incoming = bounds("page Downloads")
        val outgoing = bounds("page Home")
        assertTrue("Downloads comes down from above, at ${incoming.top}", incoming.top < 0f)
        assertTrue("Home leaves downwards, at ${outgoing.top}", outgoing.top > 0f)
        assertEquals("No sideways move", tokens.besideClearance.value, incoming.left, 0.5f)
        compose.mainClock.autoAdvance = true
        compose.waitForIdle()
        assertEquals(0f, bounds("page Downloads").top, 0.5f)
    }

    @Test
    fun theSlideLooksDoneInAboutAThirdOfASecond() {
        showShell()
        val height = bounds("page Home").height
        compose.mainClock.autoAdvance = false
        tab(Tab.Settings).performClick()
        // Two frames to take the tap and compose the new page, then about 300 ms of spring.
        advanceFrames(2 + 19)
        val top = bounds("page Settings").top
        assertTrue("Settings is still ${top}dp from its place", top < height * 0.03f)
        // The spring's invisible tail (under a pixel from its place) ends well within 0.8 s,
        // and the old page is let go.
        advanceFrames(28)
        page(Tab.Home).assertDoesNotExist()
        assertEquals(0f, bounds("page Settings").top, 0.5f)
    }

    @Test
    fun removeAnimationsCrossfadesInPlace() {
        showShell(reduceMotion = true)
        compose.mainClock.autoAdvance = false
        tab(Tab.Settings).performClick()
        advanceFrames(6)
        // Both pages are on screen at once, neither moved: a crossfade.
        assertEquals(0f, bounds("page Settings").top, 0.5f)
        assertEquals(0f, bounds("page Home").top, 0.5f)
        assertEquals(tokens.besideClearance.value, bounds("page Settings").left, 0.5f)
        advanceFrames(14)
        page(Tab.Home).assertDoesNotExist()
    }

    @Test
    fun longPressShowsTheTabNameToTheRightOfTheBar() = tooltipToTheRight(LayoutDirection.Ltr)

    /** The physical left rail keeps tooltips over the content to its right in every language. */
    @Test
    fun theTooltipStaysRightOfTheBarInARightToLeftLayout() = tooltipToTheRight(LayoutDirection.Rtl)

    private fun tooltipToTheRight(layoutDirection: LayoutDirection) {
        showShell(layoutDirection = layoutDirection)
        compose.onAllNodesWithText("Downloads").assertCountEquals(0)
        tab(Tab.Downloads).performTouchInput { longClick() }
        compose.waitForIdle()
        // The tooltip is a popup window of its own; compare on the screen, in pixels.
        val square = onScreen(tab(Tab.Downloads).fetchSemanticsNode())
        val bar = onScreen(compose.onNodeWithTag("toolbar").fetchSemanticsNode())
        val text = compose.onNodeWithText("Downloads").fetchSemanticsNode()
        val name = onScreen(text)
        // The popup's root is the tooltip itself, separated from the rail by the normal gap.
        val tooltip = onScreen(generateSequence(text) { it.parent }.last())
        val gap = tokens.tooltipGap.value * compose.density.density
        assertEquals("The tooltip starts ${tokens.tooltipGap} right of the bar", bar.right + gap, tooltip.left, 1f)
        assertTrue("The name starts right of the square", name.left >= square.right)
        assertTrue("The name is level with the square", name.center.y > square.top && name.center.y < square.bottom)
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
    fun backFromSettingsBringsHomeDownFromAbove() {
        showShell()
        tab(Tab.Settings).performClick()
        compose.waitForIdle()
        compose.mainClock.autoAdvance = false
        compose.runOnUiThread { compose.activity.onBackPressedDispatcher.onBackPressed() }
        advanceFrames(6)
        // Home is above Settings on the bar, so it comes down from the top.
        assertTrue("Home comes down from above, at ${bounds("page Home").top}", bounds("page Home").top < 0f)
        compose.mainClock.autoAdvance = true
        compose.waitForIdle()
        tab(Tab.Home).assertIsSelected()
    }

    /**
     * Scrolled to its end, a page's last row sits the content gap above the bar: nothing on it is
     * under the bar. Mid-scroll the rows pass under the glass.
     */
    @Test
    fun scrolledToTheEndTheLastRowClearsTheBar() {
        showShell()
        compose.onNodeWithTag("list Home").performScrollToIndex(LAST_ROW + 1)
        compose.waitForIdle()
        val last = bounds("row Home $LAST_ROW")
        val bar = bounds("toolbar")
        assertTrue("The last row stays right of the rail", last.left >= bar.right + tokens.margin.value - 0.5f)
    }

    /**
     * A snackbar sits at the safe bottom beside the left rail gutter, never under the rail. Measured on the snackbar's visible surface, not on its host.
     */
    @Test
    fun aSnackbarSitsBesideTheBarNotUnderIt() {
        val snackbar = SnackbarHostState()
        compose.setContent {
            PopcornTheme(darkTheme = false, reduceMotion = true) {
                AppShell { page ->
                    Box(Modifier.fillMaxSize()) {
                        TestPage(page)
                        TabSnackbarHost(snackbar, page)
                    }
                    if (page.tab == Tab.Home) {
                        LaunchedEffect(Unit) { snackbar.showSnackbar("Moved to Kids.", duration = SnackbarDuration.Indefinite) }
                    }
                }
            }
        }
        compose.waitForIdle()
        val note = bounds("snackbar")
        val bar = bounds("toolbar")
        val screen = screen()
        assertEquals("At the safe bottom", screen.bottom - tokens.margin.value, note.bottom, 0.5f)
        assertTrue("The snackbar stays right of the rail gutter", note.left >= bar.right + tokens.margin.value - 0.5f)
        assertTrue("The snackbar respects the right gutter", note.right <= screen.right - tokens.margin.value + 0.5f)
        compose.onNodeWithText("Moved to Kids.").assertExists()
        // Those bounds are the visible surface, not space around it: 2dp inside each edge is the
        // snackbar's own zinc-900, where Material's 12dp of padding would show the page.
        val pixels = compose.onRoot().captureToImage().toPixelMap()
        val density = compose.density.density
        val surface = Color(0xFF18181B)
        listOf(
            "left" to (note.left + 2f to note.center.y),
            "right" to (note.right - 2f to note.center.y),
            "top" to (note.center.x to note.top + 2f),
            "bottom" to (note.center.x to note.bottom - 2f),
        ).forEach { (edge, at) ->
            val pixel = pixels[(at.first * density).roundToInt(), (at.second * density).roundToInt()]
            val near = abs(pixel.red - surface.red) < 0.02f && abs(pixel.green - surface.green) < 0.02f && abs(pixel.blue - surface.blue) < 0.02f
            assertTrue("Inside the $edge edge the snackbar is zinc-900, not $pixel", near)
        }
    }

    /**
     * While the photo viewer (or any page that sends the bar away) is up, the tab's snackbars
     * wait: they would sit on the viewer's plate. The message shows once the viewer is gone.
     */
    @Test
    fun aTabSnackbarWaitsWhileTheBarIsAway() {
        val snackbar = SnackbarHostState()
        var viewerOpen by mutableStateOf(true)
        compose.setContent {
            PopcornTheme(darkTheme = false, reduceMotion = true) {
                AppShell { page ->
                    Box(Modifier.fillMaxSize()) {
                        TestPage(page)
                        if (page.tab == Tab.Home && viewerOpen) HideToolbarWhileShown()
                        TabSnackbarHost(snackbar, page)
                    }
                    if (page.tab == Tab.Home) {
                        LaunchedEffect(Unit) { snackbar.showSnackbar("Kids is not available any more.", duration = SnackbarDuration.Indefinite) }
                    }
                }
            }
        }
        compose.waitForIdle()
        compose.onNodeWithText("Kids is not available any more.").assertDoesNotExist()
        viewerOpen = false
        compose.waitForIdle()
        compose.onNodeWithText("Kids is not available any more.").assertExists()
    }

    @Test
    fun theToolbarLeavesWhilePagesAskAndReturnsWhenTheLastIsGone() = toolbarLeavesAndReturns(GlassTier.Captured)

    /** With the platform blur (Android 17 QPR2) the bar fades without a layer of its own, and still leaves and comes back. */
    @Test
    fun withThePlatformBlurTheToolbarStillLeavesAndReturns() = toolbarLeavesAndReturns(GlassTier.Backdrop)

    private fun toolbarLeavesAndReturns(tier: GlassTier) {
        var viewers by mutableIntStateOf(0)
        compose.setContent {
            PopcornTheme(darkTheme = false, reduceMotion = false) {
                AppShell(glassTier = tier) { page ->
                    TestPage(page)
                    // Stand-ins for the photo viewer, on Home: each one shown asks the toolbar to leave.
                    if (page.tab == Tab.Home) repeat(viewers) { HideToolbarWhileShown() }
                }
            }
        }
        compose.onNodeWithTag("toolbar").assertExists()
        val shown = bounds("toolbar")
        viewers = 2
        // Half way out it has moved towards the left edge, not down.
        compose.mainClock.autoAdvance = false
        advanceFrames(6)
        val leaving = bounds("toolbar")
        assertTrue("The bar leaves to the left, at ${leaving.left} from ${shown.left}", leaving.left < shown.left - 1f)
        assertEquals("It does not move down", shown.top, leaving.top, 0.5f)
        compose.mainClock.autoAdvance = true
        compose.waitForIdle()
        compose.onNodeWithTag("toolbar").assertDoesNotExist()
        viewers = 1
        compose.waitForIdle()
        compose.onNodeWithTag("toolbar").assertDoesNotExist()
        viewers = 0
        compose.waitForIdle()
        compose.onNodeWithTag("toolbar").assertExists()
        assertEquals(shown.left, bounds("toolbar").left, 0.5f)
    }

    @Test
    fun theSelectionSquareTravelsToTheTappedSquare() {
        showShell()
        val from = bounds("tab Home").top
        val to = bounds("tab Settings").top
        compose.mainClock.autoAdvance = false
        tab(Tab.Settings).performClick()
        advanceFrames(3)
        val moving = bounds("toolbar indicator")
        assertTrue("The square is on its way down, at ${moving.top} between $from and $to", moving.top > from + 0.5f && moving.top < to - 0.5f)
        assertEquals("It moves straight down", bounds("tab Home").left, moving.left, 0.5f)
        compose.mainClock.autoAdvance = true
        compose.waitForIdle()
        assertEquals(to, bounds("toolbar indicator").top, 0.5f)
    }

    /**
     * Mid-slide, the edge of the square crosses the Home icon: the part over the blue is white and
     * the part beside it is the muted grey, and no icon is white on the glass or grey on the blue
     * (dark theme, where the glass is dark and the unselected icons zinc-400).
     */
    @Test
    fun eachIconIsWhiteExactlyWhereTheSquareIsUnderIt() {
        showShell(dark = true)
        val home = bounds("tab Home")
        compose.mainClock.autoAdvance = false
        tab(Tab.Search).performClick()
        var square = bounds("toolbar indicator")
        var frames = 0
        // Until the square's top edge is in the middle of Home's 24dp icon (14dp to 38dp down).
        while (square.top < home.top + 20f && frames++ < 60) {
            advanceFrames(1)
            square = bounds("toolbar indicator")
        }
        assertTrue("The square's edge is across the Home icon, at ${square.top - home.top}dp", square.top < home.top + 32f)

        val bar = bounds("toolbar")
        val pixels = compose.onNodeWithTag("toolbar").captureToImage().toPixelMap()
        val density = compose.density.density
        fun Color.white() = red > 0.95f && green > 0.95f && blue > 0.95f
        fun Color.muted() = abs(red - 0.62f) < 0.08f && abs(blue - 0.66f) < 0.08f
        fun Color.greyish() = abs(red - blue) < 0.1f && red > 0.45f && red < 0.8f
        var whiteOnSquare = 0
        var mutedBeside = 0
        for (name in listOf("tab Home", "tab Search")) {
            val target = bounds(name)
            var y = target.top + 12f
            while (y <= target.bottom - 12f) {
                var x = target.left + 12f
                while (x <= target.right - 12f) {
                    val pixel = pixels[((x - bar.left) * density).roundToInt(), ((y - bar.top) * density).roundToInt()]
                    when {
                        // The clip's own edge is anti-aliased: leave the 1dp either side of it out.
                        y > square.top + 1f && y < square.bottom - 1f -> {
                            assertTrue("Over the square at ${y - target.top}dp in $name: not grey, but $pixel", !pixel.greyish())
                            if (pixel.white()) whiteOnSquare++
                        }
                        y < square.top - 1f || y > square.bottom + 1f -> {
                            assertTrue("Beside the square at ${y - target.top}dp in $name: not white, but $pixel", !pixel.white())
                            if (pixel.muted()) mutedBeside++
                        }
                    }
                    x += 0.5f
                }
                y += 0.5f
            }
        }
        assertTrue("Part of the Home icon is white on the square ($whiteOnSquare samples)", whiteOnSquare > 10)
        assertTrue("Part of the icons is muted beside it ($mutedBeside samples)", mutedBeside > 10)
    }

    @Test
    fun underRemoveAnimationsTheSelectionSquareJumps() {
        showShell(reduceMotion = true)
        compose.mainClock.autoAdvance = false
        tab(Tab.Settings).performClick()
        advanceFrames(3)
        // Already under the tapped square while the pages are still crossfading.
        page(Tab.Home).assertExists()
        assertEquals(bounds("tab Settings").top, bounds("toolbar indicator").top, 0.5f)
    }

    /**
     * Handedness does not change with the language: in a right-to-left layout the bar stays on
     * the left edge with Downloads at the top, and the pages still slide up and down.
     */
    @Test
    fun rightToLeftKeepsTheBarOnTheLeftAndTheSlidesVertical() {
        showShell(layoutDirection = LayoutDirection.Rtl)
        val bar = bounds("toolbar")
        assertEquals(screen().left + tokens.margin.value, bar.left, 0.5f)
        assertEquals(screen().center.y, bar.center.y, 0.5f)
        assertTrue(bounds("tab Downloads").bottom <= bounds("tab Home").top + 0.5f)
        assertTrue(bounds("tab Home").bottom <= bounds("tab Settings").top + 0.5f)
        compose.mainClock.autoAdvance = false
        tab(Tab.Settings).performClick()
        advanceFrames(6)
        val incoming = bounds("page Settings")
        val outgoing = bounds("page Home")
        assertTrue("Settings rises from below, at ${incoming.top}", incoming.top > 0f)
        assertTrue("Home leaves upwards, at ${outgoing.top}", outgoing.top < 0f)
        assertEquals(tokens.besideClearance.value, incoming.left, 0.5f)
        compose.mainClock.autoAdvance = true
        compose.waitForIdle()
        assertEquals(0f, bounds("page Settings").top, 0.5f)
        assertEquals(bounds("tab Settings").top, bounds("toolbar indicator").top, 0.5f)
    }

    @Test
    fun theSlideDirectionFollowsTheBarFromTopToBottom() {
        // Settings is below Home: its page rises from the bottom. Downloads is above: it comes down.
        assertEquals(1, slideDirection(Tab.Home, Tab.Settings))
        assertEquals(-1, slideDirection(Tab.Home, Tab.Downloads))
        assertEquals(1, slideDirection(Tab.Downloads, Tab.Settings))
        assertEquals(0, slideDirection(Tab.Home, Tab.Home))
    }

    /**
     * The rail clears a side navigation bar or cutout and centers between the top and bottom
     * safe insets; bottom content clearance follows the navigation bar under the page.
     */
    @Test
    fun theBarKeepsClearOfANavigationBarOnTheLeft() {
        var clearance = 0.dp
        compose.setContent {
            PopcornTheme(darkTheme = false, reduceMotion = true) {
                AppShell { page ->
                    clearance = page.bottomClearance
                    TestPage(page)
                }
            }
        }
        compose.waitForIdle()
        applyInsets(navigation = Insets.of(dpToPx(48), 0, 0, 0))
        val screen = screen()
        val bar = bounds("toolbar")
        assertEquals(screen.left + 48f + tokens.margin.value, bar.left, 0.5f)
        assertEquals(screen.center.y, bar.center.y, 0.5f)
        assertEquals(tokens.contentGap.value, clearance.value, 0.5f)
    }

    @Test
    fun theBarCentersBetweenStatusAndNavigationBarsAndClearsALeftCutout() {
        var clearance = 0.dp
        compose.setContent {
            PopcornTheme(darkTheme = false, reduceMotion = true) {
                AppShell { page ->
                    clearance = page.bottomClearance
                    TestPage(page)
                }
            }
        }
        compose.waitForIdle()
        applyInsets(navigation = Insets.of(0, 0, 0, dpToPx(24)), cutout = Insets.of(dpToPx(32), 0, 0, 0), status = Insets.of(0, dpToPx(20), 0, 0))
        val screen = screen()
        val bar = bounds("toolbar")
        assertEquals(screen.left + 32f + tokens.margin.value, bar.left, 0.5f)
        assertEquals((screen.top + 20f + screen.bottom - 24f) / 2, bar.center.y, 0.5f)
        assertEquals(tokens.contentGap.value + 24f, clearance.value, 0.5f)
    }

    private fun onScreen(node: SemanticsNode): Rect = Rect(node.positionOnScreen, node.size.toSize())

    private fun dpToPx(dp: Int): Int = (dp * compose.density.density).roundToInt()

    /** Hands the Compose view window insets, as the system does when the phone turns. */
    private fun applyInsets(navigation: Insets = Insets.NONE, cutout: Insets = Insets.NONE, status: Insets = Insets.NONE) {
        compose.runOnUiThread {
            // The content view passes them on to the Compose view inside it, which reads them.
            val content = compose.activity.findViewById<ViewGroup>(android.R.id.content)
            val insets = WindowInsetsCompat.Builder()
                .setInsets(WindowInsetsCompat.Type.statusBars(), status)
                .setInsets(WindowInsetsCompat.Type.navigationBars(), navigation)
                .setInsets(WindowInsetsCompat.Type.displayCutout(), cutout)
                .build()
            ViewCompat.dispatchApplyWindowInsets(content, insets)
        }
        compose.waitForIdle()
    }
}

private const val LAST_ROW = 99

/** A long list and a saveable counter, standing in for a tab's first page, ending with the shell's clearance. */
@Composable
private fun TestPage(page: TabPage) {
    val state = rememberLazyListState()
    var taps by rememberSaveable { mutableIntStateOf(0) }
    LaunchedEffect(page.reselections) {
        if (page.reselections > 0) state.animateScrollToItem(0)
    }
    LazyColumn(
        state = state,
        contentPadding = PaddingValues(bottom = page.bottomClearance),
        modifier = Modifier.fillMaxSize().testTag("list ${page.tab.name}"),
    ) {
        item {
            Text(
                "${page.tab.name} taps: $taps",
                modifier = Modifier.testTag("count ${page.tab.name}").clickable { taps++ },
            )
        }
        items(LAST_ROW + 1) { index ->
            Text("${page.tab.name} row $index", Modifier.testTag("row ${page.tab.name} $index"))
        }
    }
}
