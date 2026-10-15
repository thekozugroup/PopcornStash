package com.thekozugroup.popcornstash.screenshots

import androidx.activity.ComponentActivity
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.github.takahirom.roborazzi.captureRoboImage
import com.thekozugroup.popcornstash.R
import com.thekozugroup.popcornstash.navigation.AppShell
import com.thekozugroup.popcornstash.navigation.Tab
import com.thekozugroup.popcornstash.ui.theme.Popcorn
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

@RunWith(AndroidJUnit4::class)
@Config(qualifiers = PHONE)
class AdaptiveScreenshotTest {
    @get:Rule val compose = createAndroidComposeRule<ComponentActivity>()

    private fun shell(dark: Boolean, name: String) {
        compose.setContent {
            PopcornTheme(darkTheme = dark, reduceMotion = true) {
                AppShell(initialTab = Tab.Home, readyToImport = 3) { SampleTabRoot(it) }
            }
        }
        compose.waitForIdle()
        compose.onRoot().captureRoboImage(screenshot(name))
    }

    @Test @Config(qualifiers = "w840dp-h1100dp-xxhdpi")
    fun tabletLight() = shell(false, "shell-tablet-light")

    @Test @Config(qualifiers = "w840dp-h1100dp-xxhdpi")
    fun tabletDark() = shell(true, "shell-tablet-dark")

    @Test @Config(qualifiers = "w673dp-h841dp-xxhdpi")
    fun unfoldedLight() = shell(false, "shell-unfolded-light")

    @Test fun launcherIcon() {
        compose.setContent {
            PopcornTheme(darkTheme = false) {
                Box(Modifier.fillMaxSize().background(Popcorn.colors.page), contentAlignment = Alignment.Center) {
                    Box(Modifier.size(216.dp).clip(RoundedCornerShape(48.dp))) {
                        Box(Modifier.fillMaxSize().background(Color(0xFF0088FF)))
                        Image(painterResource(R.drawable.ic_launcher_flat), null, Modifier.fillMaxSize())
                    }
                }
            }
        }
        compose.onRoot().captureRoboImage(screenshot("launcher-icon"))
    }
}
