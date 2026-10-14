package com.thekozugroup.popcornstash.screenshots

import androidx.activity.ComponentActivity
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performScrollToIndex
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.github.takahirom.roborazzi.captureRoboImage
import com.thekozugroup.popcornstash.data.api.PhotoMonth
import com.thekozugroup.popcornstash.feature.library.LibraryActions
import com.thekozugroup.popcornstash.feature.library.LibraryScreen
import com.thekozugroup.popcornstash.feature.library.LibraryViewModel.Content
import com.thekozugroup.popcornstash.feature.library.LibraryViewModel.Ui
import com.thekozugroup.popcornstash.feature.library.Paged
import com.thekozugroup.popcornstash.navigation.AppShell
import com.thekozugroup.popcornstash.navigation.Tab
import com.thekozugroup.popcornstash.testing.SampleMedia
import com.thekozugroup.popcornstash.testing.WithSampleImages
import com.thekozugroup.popcornstash.ui.components.GlassTier
import com.thekozugroup.popcornstash.ui.components.deviceGlassTier
import com.thekozugroup.popcornstash.ui.theme.Popcorn
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/**
 * The floating toolbar's frosted glass (android/DESIGN.md, Floating toolbar) over a photo grid
 * scrolled under it, light and dark, as Android 12 to 17.1 draw it (Robolectric runs Android
 * 16); the opaque bar Android 9 to 11 keep; and the glass over pure white and pure black, the
 * cases its tint is measured for. Android 17 QPR2's platform blur cannot run here.
 */
@RunWith(AndroidJUnit4::class)
@Config(qualifiers = PHONE)
class GlassScreenshotTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    /** Three months of photos, so the grid runs on under the toolbar. */
    private val photos = Content.Timeline(
        Paged(SampleMedia.timeline + PhotoMonth("2026-07", 24, (1..24).map { SampleMedia.photo("j$it", daysAgo = 40 + it) })),
    )

    private fun photosUnderTheToolbar(dark: Boolean) {
        compose.setContent {
            PopcornTheme(darkTheme = dark, reduceMotion = true) {
                WithSampleImages {
                    AppShell(initialTab = Tab.Home, readyToImport = 3) { page ->
                        if (page.tab == Tab.Home) {
                            val library = SampleMedia.photos
                            LibraryScreen(library.name, library.icon, Ui(library, photos), true, page.bottomClearance, LibraryActions(), format = SampleFormat)
                        }
                    }
                }
            }
        }
        compose.waitForIdle()
        // August's heading at the top: its photos, then July's, run on under the bar.
        compose.onNodeWithTag("timeline").performScrollToIndex(8)
        compose.waitForIdle()
    }

    @Test
    fun photosLight() {
        assertEquals(GlassTier.Captured, deviceGlassTier())
        photosUnderTheToolbar(dark = false)
        compose.onRoot().captureRoboImage(screenshot("glass-photos-light"))
        compose.captureToolbarCloseUp("glass-toolbar-light")
    }

    @Test
    fun photosDark() {
        photosUnderTheToolbar(dark = true)
        compose.onRoot().captureRoboImage(screenshot("glass-photos-dark"))
        compose.captureToolbarCloseUp("glass-toolbar-dark")
    }

    /** Android 11 has no RenderEffect: the same page with today's opaque bar. */
    @Test
    @Config(sdk = [30])
    fun photosAndroid11() {
        assertEquals(GlassTier.Opaque, deviceGlassTier())
        photosUnderTheToolbar(dark = false)
        compose.onRoot().captureRoboImage(screenshot("glass-photos-android11-light"))
    }

    /**
     * Pure white under Downloads, pure black under Settings: the two cases the tint is chosen for.
     * The edge between them runs across the middle of the bar, through the selected Home square.
     */
    private fun whiteAndBlack(dark: Boolean) {
        compose.setContent {
            PopcornTheme(darkTheme = dark, reduceMotion = true) {
                AppShell(initialTab = Tab.Home, readyToImport = 3) {
                    val toolbar = Popcorn.toolbar
                    BoxWithConstraints(Modifier.fillMaxSize().background(Color.Black)) {
                        val white = maxHeight - toolbar.margin - toolbar.length(Tab.entries.size) / 2
                        Box(Modifier.fillMaxWidth().height(white).background(Color.White))
                    }
                }
            }
        }
        compose.waitForIdle()
    }

    @Test
    fun whiteAndBlackLight() {
        whiteAndBlack(dark = false)
        compose.captureToolbarCloseUp("glass-white-black-light")
    }

    @Test
    fun whiteAndBlackDark() {
        whiteAndBlack(dark = true)
        compose.captureToolbarCloseUp("glass-white-black-dark")
    }
}
