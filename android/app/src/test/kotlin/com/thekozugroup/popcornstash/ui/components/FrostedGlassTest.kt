package com.thekozugroup.popcornstash.ui.components

import android.app.UiModeManager
import androidx.activity.ComponentActivity
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.getUnclippedBoundsInRoot
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollToIndex
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.thekozugroup.popcornstash.navigation.AppShell
import com.thekozugroup.popcornstash.navigation.Tab
import com.thekozugroup.popcornstash.navigation.TabPage
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

/**
 * The toolbar's glass (android/DESIGN.md, Floating toolbar) shows the colour of what is under
 * it and follows it live: a scroll and a change of tab both reach the glass. Robolectric runs
 * Android 16, so this is the captured glass (Android 12 to 17.1). The selected square keeps a
 * `surface` ring on any glass, and a phone set to more contrast gets the opaque bar.
 */
@RunWith(AndroidJUnit4::class)
@Config(qualifiers = "w411dp-h891dp-xxhdpi")
class FrostedGlassTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    /** The shell with [tier], or with the tier the phone's settings give when it is null. */
    private fun shell(tier: GlassTier?, dark: Boolean = false, page: @Composable (TabPage) -> Unit) {
        compose.setContent {
            PopcornTheme(darkTheme = dark, reduceMotion = true) {
                if (tier == null) AppShell(page = page) else AppShell(glassTier = tier, page = page)
            }
        }
        compose.waitForIdle()
    }

    /** The glass at the start of the Downloads square, clear of its icon and the bar's edge. */
    private fun glass(): Color {
        val image = compose.onNodeWithTag("toolbar").captureToImage()
        val x = with(compose.density) { 10.dp.roundToPx() }
        return image.toPixelMap()[x, image.height / 2]
    }

    /** The pixel 1dp outside the selected square's start edge, half way down. */
    private fun besideTheSquare(): Color {
        val toolbar = compose.onNodeWithTag("toolbar")
        val bar = toolbar.getUnclippedBoundsInRoot()
        val square = compose.onNodeWithTag("toolbar indicator").getUnclippedBoundsInRoot()
        val image = toolbar.captureToImage().toPixelMap()
        return with(compose.density) {
            image[(square.left - bar.left - 1.dp).roundToPx(), ((square.top + square.bottom) / 2 - bar.top).roundToPx()]
        }
    }

    private fun Color.reddish() = red - blue > 0.1f
    private fun Color.bluish() = blue - red > 0.1f

    @Test
    fun theGlassShowsWhatIsUnderItAndFollowsAScroll() {
        // Bands far taller than the bar: red, blue, red, ... The bar starts over the second, blue.
        shell(GlassTier.Captured) { page -> Bands(page.tab, listOf(Red, Blue)) }
        val before = glass()
        assertTrue("Over a blue band the glass is bluish, not $before", before.bluish())

        compose.onNodeWithTag("bands Home").performScrollToIndex(1)
        compose.waitForIdle()
        val after = glass()
        assertTrue("After the scroll a red band is under the bar, and the glass is reddish, not $after", after.reddish())
    }

    @Test
    fun theGlassFollowsTheTabPages() {
        shell(GlassTier.Captured) { page -> Bands(page.tab, listOf(if (page.tab == Tab.Home) Red else Blue)) }
        val home = glass()
        assertTrue("Home is red under the bar, and the glass reddish, not $home", home.reddish())

        compose.onNodeWithTag("tab Settings").performClick()
        compose.waitForIdle()
        val settings = glass()
        assertTrue("Settings is blue under the bar, and the glass bluish, not $settings", settings.bluish())
    }

    /**
     * Over a white picture the dark glass is grey (1.79:1 against Action Blue); the ring keeps
     * zinc-900 next to the blue (3.38:1). Over black the light glass is light grey; the ring keeps white.
     */
    @Test
    fun theSelectedSquareIsRingedInSurfaceOnAnyGlass() {
        shell(GlassTier.Captured, dark = true) { page -> Bands(page.tab, listOf(Color.White)) }
        val dark = besideTheSquare()
        assertTrue("Beside the square on dark glass over white: zinc-900, not $dark", dark.red < 0.12f && dark.blue < 0.13f)
    }

    @Test
    fun theSelectedSquareIsRingedInWhiteOnLightGlassOverBlack() {
        shell(GlassTier.Captured) { page -> Bands(page.tab, listOf(Color.Black)) }
        val light = besideTheSquare()
        assertTrue("Beside the square on light glass over black: white, not $light", light.red > 0.97f && light.blue > 0.97f)
    }

    @Test
    fun aPhoneSetToMoreContrastGetsTheOpaqueBarAndFollowsTheSetting() {
        val uiMode = shadowOf(compose.activity.getSystemService(UiModeManager::class.java))
        uiMode.setContrast(0.5f)
        shell(tier = null) { page -> Bands(page.tab, listOf(Red, Blue)) }
        val opaque = glass()
        assertTrue("At medium contrast the bar is the opaque white, not $opaque", opaque.red > 0.97f && opaque.green > 0.97f && opaque.blue > 0.97f)

        compose.runOnUiThread { uiMode.setContrast(0f) }
        compose.waitForIdle()
        val frosted = glass()
        assertTrue("Back at standard contrast the glass shows the blue band under it, not $frosted", frosted.bluish())
    }

    @Test
    fun withoutBlurTheBarIsTheOpaqueSurface() {
        shell(GlassTier.Opaque) { page -> Bands(page.tab, listOf(Red, Blue)) }
        val color = glass()
        assertTrue("The opaque bar is white, not $color", color.red > 0.97f && color.green > 0.97f && color.blue > 0.97f)
    }
}

private val Red = Color(0xFFE11D48)
private val Blue = Color(0xFF2563EB)

/** Bands of [colors], 600dp tall each, repeated down a long list. */
@Composable
private fun Bands(tab: Tab, colors: List<Color>) {
    LazyColumn(Modifier.fillMaxSize().testTag("bands ${tab.name}")) {
        items(12) { index ->
            Box(Modifier.fillMaxWidth().height(600.dp).background(colors[index % colors.size]))
        }
    }
}
