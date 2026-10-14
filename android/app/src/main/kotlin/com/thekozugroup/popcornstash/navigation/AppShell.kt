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
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.isImeVisible
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
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
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.CompositingStrategy
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.Dp
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
    /** Bottom padding that lets the page's last row clear the floating toolbar and the navigation bar. */
    val bottomClearance: Dp,
    /** Grows each time the person taps the tab they are already on: scroll to the top, or pop to the first page. */
    val reselections: Int,
)

/**
 * The signed-in app: three tab pages and the floating toolbar over them
 * (android/DESIGN.md, Navigation).
 *
 * - Tapping a tab slides the pages towards the tapped square ([tabTransition]); under
 *   reduce motion they crossfade.
 * - Each tab keeps its state and scroll position while another tab shows (one saveable
 *   state holder entry per tab).
 * - System Back on another tab returns to Home; on Home it leaves the app.
 * - The toolbar floats over the pages and leaves while a page asks it to
 *   ([HideToolbarWhileShown]: the photo viewer) and while the keyboard is open. The player
 *   is its own activity, so the shell is not on screen while it plays.
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
    val layoutDirection = LocalLayoutDirection.current
    val toolbar = Popcorn.toolbar
    val navigationBar = WindowInsets.navigationBars.asPaddingValues().calculateBottomPadding()
    val clearance = toolbar.clearance + navigationBar
    val backdrop = rememberGlassBackdrop(glassTier)

    BackHandler(enabled = current != Tab.Start) { current = Tab.Start }

    Box(modifier.fillMaxSize().background(Popcorn.colors.page)) {
        CompositionLocalProvider(LocalToolbarRequests provides toolbarRequests) {
            AnimatedContent(
                targetState = current,
                transitionSpec = { tabTransition(initialState, targetState, reduceMotion, layoutDirection) },
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
                        page(TabPage(tab, clearance, reselections.getValue(tab).intValue))
                    }
                }
            }
        }

        // The platform's backdrop blur reads the window only when the bar is not drawn into a
        // layer of its own, so on that tier the fade is drawn per operation (ModulateAlpha).
        val fadeThroughLayer = glassTier != GlassTier.Backdrop
        AnimatedVisibility(
            visible = !toolbarRequests.hidden && !WindowInsets.isImeVisible,
            enter = slideInVertically(tween(PopcornMotion.TOOLBAR_SHOW_HIDE_MS)) { it / 2 } +
                if (fadeThroughLayer) fadeIn(tween(PopcornMotion.TOOLBAR_SHOW_HIDE_MS)) else EnterTransition.None,
            exit = slideOutVertically(tween(PopcornMotion.TOOLBAR_SHOW_HIDE_MS)) { it / 2 } +
                if (fadeThroughLayer) fadeOut(tween(PopcornMotion.TOOLBAR_SHOW_HIDE_MS)) else ExitTransition.None,
            modifier = Modifier
                .align(Alignment.BottomCenter)
                .fillMaxWidth()
                .padding(bottom = navigationBar + toolbar.margin),
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
            Box(Modifier.fillMaxWidth().then(fade), contentAlignment = Alignment.BottomCenter) {
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
