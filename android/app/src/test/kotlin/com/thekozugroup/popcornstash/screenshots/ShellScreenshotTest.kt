package com.thekozugroup.popcornstash.screenshots

import androidx.activity.ComponentActivity
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHostState
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.longClick
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.github.takahirom.roborazzi.RoborazziOptions
import com.github.takahirom.roborazzi.captureRoboImage
import com.thekozugroup.popcornstash.feature.downloads.DownloadsActions
import com.thekozugroup.popcornstash.feature.downloads.DownloadsPage
import com.thekozugroup.popcornstash.feature.downloads.DownloadsSamples
import com.thekozugroup.popcornstash.feature.settings.AppUpdateSectionState
import com.thekozugroup.popcornstash.feature.settings.SettingsRootPage
import com.thekozugroup.popcornstash.navigation.AppShell
import com.thekozugroup.popcornstash.navigation.Tab
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import com.thekozugroup.popcornstash.update.AppBuild
import com.thekozugroup.popcornstash.update.AppUpdateUi
import com.thekozugroup.popcornstash.update.UpdateStatus
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/**
 * The shell as a phone shows it, light and dark: the upright toolbar on the right edge over
 * each tab, a long page scrolled to its end clear of the bar, a snackbar beside it, the tab
 * slide and the phone turned on its side. Recorded with `./gradlew recordRoborazziDebug`,
 * compared with `verifyRoborazziDebug`.
 */
@RunWith(AndroidJUnit4::class)
@Config(qualifiers = PHONE)
class ShellScreenshotTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val sampleBuild = AppBuild(
        number = 415,
        apkUrl = "https://github.com/thekozugroup/PopcornStash/releases/download/android-415/popcornstash-android-415.apk",
        sha256Url = "https://github.com/thekozugroup/PopcornStash/releases/download/android-415/popcornstash-android-415.apk.sha256",
        pageUrl = "https://github.com/thekozugroup/PopcornStash/releases/tag/android-415",
        size = 14_268_412,
    )

    private fun shell(dark: Boolean, tab: Tab, readyToImport: Int = 0) {
        compose.setContent {
            PopcornTheme(darkTheme = dark, reduceMotion = false) {
                AppShell(initialTab = tab, readyToImport = readyToImport) { SampleTabRoot(it) }
            }
        }
        compose.waitForIdle()
    }

    @Test
    fun homeLight() {
        shell(dark = false, tab = Tab.Home)
        compose.onRoot().captureRoboImage(screenshot("shell-home-light"))
    }

    @Test
    fun homeDark() {
        shell(dark = true, tab = Tab.Home)
        compose.onRoot().captureRoboImage(screenshot("shell-home-dark"))
    }

    @Test
    fun downloadsWithFilesReadyLight() {
        shell(dark = false, tab = Tab.Downloads, readyToImport = DownloadsSamples.content().imports.size)
        compose.onRoot().captureRoboImage(screenshot("shell-downloads-light"))
    }

    @Test
    fun downloadsWithFilesReadyDark() {
        shell(dark = true, tab = Tab.Downloads, readyToImport = DownloadsSamples.content().imports.size)
        compose.onRoot().captureRoboImage(screenshot("shell-downloads-dark"))
    }

    @Test
    fun settingsLight() {
        shell(dark = false, tab = Tab.Settings)
        compose.onRoot().captureRoboImage(screenshot("shell-settings-light"))
    }

    @Test
    fun settingsDark() {
        shell(dark = true, tab = Tab.Settings)
        compose.onRoot().captureRoboImage(screenshot("shell-settings-dark"))
    }

    /** The floating toolbar close up, Home selected, with the Downloads badge on its top-left corner. */
    @Test
    fun toolbarLight() {
        shell(dark = false, tab = Tab.Home, readyToImport = DownloadsSamples.content().imports.size)
        compose.captureToolbarCloseUp("toolbar-light")
    }

    @Test
    fun toolbarDark() {
        shell(dark = true, tab = Tab.Home, readyToImport = DownloadsSamples.content().imports.size)
        compose.captureToolbarCloseUp("toolbar-dark")
    }

    /**
     * Three moments of Settings rising over Home after a tap on Settings (the square below Home):
     * early, about half way and most of the way on the page spring, the selection square
     * travelling down with it. The press ripple runs on the platform's clock, so 1% of the
     * pixels may differ.
     */
    @Test
    fun tabTransitionLight() {
        shell(dark = false, tab = Tab.Home, readyToImport = DownloadsSamples.content().imports.size)
        compose.mainClock.autoAdvance = false
        compose.onNodeWithTag("tab Settings").performClick()
        val ripple = RoborazziOptions(compareOptions = RoborazziOptions.CompareOptions(changeThreshold = 0.01f))
        var frame = 0
        listOf(5, 7, 10).forEachIndexed { index, at ->
            while (frame < at) {
                compose.mainClock.advanceTimeByFrame()
                compose.waitForIdle()
                frame++
            }
            compose.onRoot().captureRoboImage(screenshot("transition-${index + 1}-light"), roborazziOptions = ripple)
        }
    }

    /** Downloads scrolled to its end: the last row, its More button and its line sit above the bar. */
    private fun downloadsAtTheEnd(dark: Boolean) {
        val samples = DownloadsSamples
        compose.setContent {
            PopcornTheme(darkTheme = dark, reduceMotion = false) {
                AppShell(initialTab = Tab.Downloads, readyToImport = samples.content().imports.size) { page ->
                    if (page.tab == Tab.Downloads) {
                        // Past the last index: the list settles at its end.
                        val list = remember { LazyListState(firstVisibleItemIndex = 999) }
                        DownloadsPage(page, samples.content(), DownloadsActions(onOpenPlace = {}), samples.NOW, listState = list)
                    } else {
                        SampleTabRoot(page)
                    }
                }
            }
        }
        compose.waitForIdle()
        compose.assertNothingUnderTheToolbar()
    }

    @Test
    fun downloadsAtTheEndLight() {
        downloadsAtTheEnd(dark = false)
        compose.onRoot().captureRoboImage(screenshot("shell-downloads-end-light"))
    }

    @Test
    fun downloadsAtTheEndDark() {
        downloadsAtTheEnd(dark = true)
        compose.onRoot().captureRoboImage(screenshot("shell-downloads-end-dark"))
    }

    /**
     * Settings (the owner's) scrolled to its end, with "This app" in [update]: Sign out, the
     * automatic-check switch, the update's button or its bar all end 16dp or more above the bar.
     */
    private fun settingsAtTheEnd(dark: Boolean, update: UpdateStatus) {
        val ui = AppUpdateUi(version = "0.1.0", build = 412, status = update, autoCheck = true)
        compose.setContent {
            PopcornTheme(darkTheme = dark, reduceMotion = false) {
                AppShell(initialTab = Tab.Settings) { page ->
                    if (page.tab == Tab.Settings) {
                        val list = remember { LazyListState(firstVisibleItemIndex = 999) }
                        SettingsRootPage(
                            page,
                            SampleOwner,
                            "stash.example.com",
                            "0.2.0",
                            SampleLibrarySettings,
                            onOpenWeb = {},
                            onSignOut = {},
                            listState = list,
                            appUpdates = AppUpdateSectionState(ui, now = 0L),
                        )
                    } else {
                        SampleTabRoot(page)
                    }
                }
            }
        }
        compose.waitForIdle()
        compose.assertNothingUnderTheToolbar(clearance = 16.dp)
    }

    @Test
    fun settingsAtTheEndLight() {
        settingsAtTheEnd(dark = false, update = UpdateStatus.Available(sampleBuild))
        compose.onRoot().captureRoboImage(screenshot("shell-settings-end-light"))
    }

    @Test
    fun settingsAtTheEndDownloadingDark() {
        settingsAtTheEnd(dark = true, update = UpdateStatus.Downloading(sampleBuild, 41))
        compose.onRoot().captureRoboImage(screenshot("shell-settings-end-downloading-dark"))
    }

    /** A snackbar after an action: at the bottom, level with the bar's bottom edge and to its left. */
    private fun snackbarBesideTheBar(dark: Boolean) {
        val samples = DownloadsSamples
        compose.setContent {
            PopcornTheme(darkTheme = dark, reduceMotion = false) {
                AppShell(initialTab = Tab.Downloads, readyToImport = samples.content().imports.size) { page ->
                    if (page.tab == Tab.Downloads) {
                        val snackbar = remember { SnackbarHostState() }
                        LaunchedEffect(Unit) { snackbar.showSnackbar("Ignored. clip_final_v2.mkv stays where it is.", duration = SnackbarDuration.Indefinite) }
                        DownloadsPage(page, samples.content(), DownloadsActions(onOpenPlace = {}), samples.NOW, snackbar = snackbar)
                    } else {
                        SampleTabRoot(page)
                    }
                }
            }
        }
        compose.waitForIdle()
    }

    @Test
    fun snackbarLight() {
        snackbarBesideTheBar(dark = false)
        compose.onRoot().captureRoboImage(screenshot("shell-snackbar-light"))
    }

    @Test
    fun snackbarDark() {
        snackbarBesideTheBar(dark = true)
        compose.onRoot().captureRoboImage(screenshot("shell-snackbar-dark"))
    }

    /** The phone on its side: the bar stays on the right edge, bottom-aligned the same way. */
    @Test
    @Config(qualifiers = PHONE_LANDSCAPE)
    fun landscapeHomeLight() {
        shell(dark = false, tab = Tab.Home, readyToImport = DownloadsSamples.content().imports.size)
        compose.onRoot().captureRoboImage(screenshot("shell-landscape-home-light"))
    }

    @Test
    @Config(qualifiers = PHONE_LANDSCAPE)
    fun landscapeDownloadsDark() {
        shell(dark = true, tab = Tab.Downloads, readyToImport = DownloadsSamples.content().imports.size)
        compose.onRoot().captureRoboImage(screenshot("shell-landscape-downloads-dark"))
    }

    @Test
    fun toolbarTooltipLight() {
        shell(dark = false, tab = Tab.Home)
        // Hold the clock so the press ripple has faded the same way on every run, and the
        // tooltip (which hides by itself after a while) is still up.
        compose.mainClock.autoAdvance = false
        compose.onNodeWithTag("tab Settings").performTouchInput { longClick() }
        repeat(40) {
            compose.mainClock.advanceTimeByFrame()
            compose.waitForIdle()
        }
        // The press ripple is drawn by the platform on its own clock, so its last traces
        // differ a little from run to run; allow 1% of the pixels to differ.
        val ripple = RoborazziOptions(compareOptions = RoborazziOptions.CompareOptions(changeThreshold = 0.01f))
        compose.onNodeWithTag("toolbar").captureRoboImage(screenshot("toolbar-tooltip-light"), roborazziOptions = ripple)
        compose.onRoot().captureRoboImage(screenshot("shell-tooltip-light"), roborazziOptions = ripple)
    }
}
