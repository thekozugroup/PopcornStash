package com.thekozugroup.popcornstash.screenshots

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.longClick
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performTouchInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.github.takahirom.roborazzi.captureRoboImage
import com.thekozugroup.popcornstash.TabRoot
import com.thekozugroup.popcornstash.navigation.AppShell
import com.thekozugroup.popcornstash.navigation.Tab
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/**
 * The shell as a phone shows it, light and dark. Recorded with
 * `./gradlew recordRoborazziDebug`, compared with `verifyRoborazziDebug`.
 */
@RunWith(AndroidJUnit4::class)
@Config(qualifiers = PHONE)
class ShellScreenshotTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private fun shell(dark: Boolean, tab: Tab, readyToImport: Int = 0) {
        compose.setContent {
            PopcornTheme(darkTheme = dark, reduceMotion = false) {
                AppShell(initialTab = tab, readyToImport = readyToImport) { TabRoot(it) }
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
        shell(dark = false, tab = Tab.Downloads, readyToImport = 3)
        compose.onRoot().captureRoboImage(screenshot("shell-downloads-light"))
    }

    @Test
    fun downloadsWithFilesReadyDark() {
        shell(dark = true, tab = Tab.Downloads, readyToImport = 12)
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

    @Test
    fun toolbarTooltipLight() {
        shell(dark = false, tab = Tab.Home)
        compose.onNodeWithTag("tab Settings").performTouchInput { longClick() }
        compose.waitForIdle()
        compose.onNodeWithTag("toolbar").captureRoboImage(screenshot("toolbar-tooltip-light"))
        compose.onRoot().captureRoboImage(screenshot("shell-tooltip-light"))
    }
}
