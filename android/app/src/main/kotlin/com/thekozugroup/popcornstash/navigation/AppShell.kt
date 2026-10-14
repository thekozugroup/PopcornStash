package com.thekozugroup.popcornstash.navigation

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.EnterExitState
import androidx.compose.animation.EnterTransition
import androidx.compose.animation.ExitTransition
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.displayCutout
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.isImeVisible
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.union
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.saveable.rememberSaveableStateHolder
import androidx.compose.runtime.setValue
import androidx.compose.ui.AbsoluteAlignment
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.CompositingStrategy
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.thekozugroup.popcornstash.ui.components.FloatingNavToolbar
import com.thekozugroup.popcornstash.ui.components.GlassTier
import com.thekozugroup.popcornstash.ui.components.glassSource
import com.thekozugroup.popcornstash.ui.components.rememberGlassBackdrop
import com.thekozugroup.popcornstash.ui.components.rememberGlassTier
import com.thekozugroup.popcornstash.ui.theme.Popcorn
import com.thekozugroup.popcornstash.ui.theme.PopcornMotion

/** What a tab's page needs from the shell. */
data class TabPage(
    val tab: Tab,
    /**
     * Padding a page leaves at its end (under its last row) so that, scrolled to the end, its last
     * row sits above the floating toolbar on the right edge, clear of it and of the navigation
     * bar. Mid-scroll the rows pass under the bar; at the end nothing is under it.
     */
    val bottomClearance: Dp,
    /** Grows each time the person taps the tab they are already on: scroll to the top, or pop to the first page. */
    val reselections: Int,
    /**
     * Where the page's snackbars go ([com.thekozugroup.popcornstash.ui.components.TabSnackbarHost]):
     * padding from the window's edges that puts them at the bottom, level with the toolbar's
     * bottom edge and to its left, never under it.
     */
    val snackbarPadding: PaddingValues = PaddingValues(0.dp),
)

/**
 * The signed-in app: three tab pages and the floating toolbar over them
 * (android/DESIGN.md, Navigation).
 *
 * - The toolbar is one upright bar on the right edge of the screen, physically right in every
 *   layout direction, its bottom where a thumb rests: the toolbar margin above the navigation
 *   bar and in from the right edge, clear of the navigation bar and a display cutout on that
 *   side when the phone is turned.
 * - Tapping a tab slides the pages up or down, the way the selection moves on the bar
 *   ([tabTransition]); under reduce motion they crossfade.
 * - Each tab keeps its state and scroll position while another tab shows (one saveable
 *   state holder entry per tab).
 * - System Back on another tab returns to Home; on Home it leaves the app.
 * - The toolbar floats over the pages and leaves (towards the edge it lives on) while a page
 *   asks it to ([HideToolbarWhileShown]: the photo viewer) and while the keyboard is open. The
 *   player is its own activity, so the shell is not on screen while it plays.
 * - The toolbar is frosted glass as [glassTier] allows (FrostedGlass.kt). On Android 12 to
 *   17.1 the pages are recorded once per frame into one layer the toolbar draws blurred; on
 *   Android 17 QPR2 and later the platform blurs the window itself, so nothing is recorded.
 *   There the toolbar fades without an offscreen layer: the platform would blur that empty
 *   layer instead of the window, and the pages would show sharp under the tint while it fades.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun AppShell(
    modifier: Modifier = Modifier,
    initialTab: Tab = Tab.Start,
    readyToImport: Int = 0,
    /** A tab asked for from outside (a notification, the share sheet): shown once, then [onOpenedTab]. */
    openTab: Tab? = null,
    onOpenedTab: () -> Unit = {},
    glassTier: GlassTier = rememberGlassTier(),
    page: @Composable (TabPage) -> Unit,
) {
    var current by rememberSaveable { mutableStateOf(initialTab) }
    LaunchedEffect(openTab) {
        if (openTab != null) {
            current = openTab
            onOpenedTab()
        }
    }
    val reselections = remember { Tab.entries.associateWith { mutableIntStateOf(0) } }
    val holder = rememberSaveableStateHolder()
    val toolbarRequests = remember { ToolbarRequests() }
    val reduceMotion = Popcorn.reduceMotion
    val toolbar = Popcorn.toolbar
    // The bar keeps clear of the navigation bar under it and, with the phone turned, of the
    // navigation bar and a display cutout on the right; snackbars of both, on either side.
    val edges = WindowInsets.navigationBars.union(WindowInsets.displayCutout)
    val density = LocalDensity.current
    val layoutDirection = LocalLayoutDirection.current
    val bottomInset = with(density) { edges.getBottom(density).toDp() }
    val leftInset = with(density) { edges.getLeft(density, layoutDirection).toDp() }
    val rightInset = with(density) { edges.getRight(density, layoutDirection).toDp() }
    val clearance = toolbar.clearance(Tab.entries.size) + bottomInset
    val snackbarPadding = PaddingValues.Absolute(
        left = leftInset + toolbar.margin,
        right = rightInset + toolbar.besideClearance,
        bottom = bottomInset + toolbar.margin,
    )
    val backdrop = rememberGlassBackdrop(glassTier)

    BackHandler(enabled = current != Tab.Start) { current = Tab.Start }

    Box(modifier.fillMaxSize().background(Popcorn.colors.page)) {
        CompositionLocalProvider(LocalToolbarRequests provides toolbarRequests) {
            AnimatedContent(
                targetState = current,
                transitionSpec = { tabTransition(initialState, targetState, reduceMotion) },
                label = "tab pages",
                modifier = Modifier.fillMaxSize().glassSource(backdrop),
            ) { tab ->
                holder.SaveableStateProvider(tab.name) {
                    Box(
                        Modifier
                            .fillMaxSize()
                            .background(Popcorn.colors.page)
                            .testTag("page ${tab.name}"),
                    ) {
                        page(TabPage(tab, clearance, reselections.getValue(tab).intValue, snackbarPadding))
                    }
                }
            }
        }

        // The platform's backdrop blur reads the window only when the bar is not drawn into a
        // layer of its own, so on that tier the fade is drawn per operation (ModulateAlpha).
        val fadeThroughLayer = glassTier != GlassTier.Backdrop
        // Slide offsets are in screen pixels: the bar leaves towards the right edge, half its width.
        AnimatedVisibility(
            visible = !toolbarRequests.hidden && !WindowInsets.isImeVisible,
            enter = slideInHorizontally(tween(PopcornMotion.TOOLBAR_SHOW_HIDE_MS)) { it / 2 } +
                if (fadeThroughLayer) fadeIn(tween(PopcornMotion.TOOLBAR_SHOW_HIDE_MS)) else EnterTransition.None,
            exit = slideOutHorizontally(tween(PopcornMotion.TOOLBAR_SHOW_HIDE_MS)) { it / 2 } +
                if (fadeThroughLayer) fadeOut(tween(PopcornMotion.TOOLBAR_SHOW_HIDE_MS)) else ExitTransition.None,
            // The column the bar may use: from under the status bar to above the navigation bar,
            // in from the right edge. The bar sits at its bottom and shrinks only when the column is short.
            modifier = Modifier
                .align(AbsoluteAlignment.BottomRight)
                .fillMaxHeight()
                .windowInsetsPadding(WindowInsets.statusBars.only(WindowInsetsSides.Top))
                .windowInsetsPadding(edges.only(WindowInsetsSides.Right + WindowInsetsSides.Bottom))
                .padding(PaddingValues.Absolute(top = toolbar.margin, right = toolbar.margin, bottom = toolbar.margin)),
        ) {
            val fade = if (fadeThroughLayer) {
                Modifier
            } else {
                val alpha by transition.animateFloat(
                    transitionSpec = { tween(PopcornMotion.TOOLBAR_SHOW_HIDE_MS) },
                    label = "toolbar fade",
                ) { state -> if (state == EnterExitState.Visible) 1f else 0f }
                Modifier.graphicsLayer {
                    this.alpha = alpha
                    compositingStrategy = CompositingStrategy.ModulateAlpha
                }
            }
            Box(Modifier.fillMaxHeight().then(fade), contentAlignment = Alignment.BottomCenter) {
                FloatingNavToolbar(
                    selected = current,
                    readyToImport = readyToImport,
                    glass = glassTier,
                    backdrop = backdrop,
                    onSelect = { tab ->
                        if (tab == current) {
                            reselections.getValue(tab).intValue++
                        } else {
                            current = tab
                        }
                    },
                )
            }
        }
    }
}
