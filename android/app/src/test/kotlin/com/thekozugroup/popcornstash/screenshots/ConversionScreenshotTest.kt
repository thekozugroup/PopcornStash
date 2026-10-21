package com.thekozugroup.popcornstash.screenshots

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onRoot
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.github.takahirom.roborazzi.captureRoboImage
import com.thekozugroup.popcornstash.data.api.*
import com.thekozugroup.popcornstash.feature.settings.ConversionUi
import com.thekozugroup.popcornstash.feature.settings.LibraryConversionPage
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

@RunWith(AndroidJUnit4::class)
@Config(qualifiers = "w411dp-h2400dp-mdpi")
class ConversionScreenshotTest {
    @get:Rule val compose = createAndroidComposeRule<ComponentActivity>()
    private fun shoot(dark: Boolean) {
        val policy = ConversionPolicy(enabled = true)
        val view = LibraryConversion(policy, listOf(ConversionEncoder("av1", "av1_vaapi", "Intel GPU · av1 · renderD128", true)), pending = 3,
            jobs = listOf(ConversionJob("1", title = "Paper Boats", state = "encoding", progress = .42, encoder = "Intel GPU · av1 · renderD128"), ConversionJob("3", title = "Paper Planes", state = "verifying", progress = .58), ConversionJob("2", title = "Evening Walk", state = "complete", savedBytes = 120_000_000)))
        compose.setContent { PopcornTheme(darkTheme = dark, reduceMotion = true) {
            LibraryConversionPage(ConversionUi(view, policy), {}, {}, {}, {})
        } }
        compose.onRoot().captureRoboImage(screenshot(if (dark) "conversion-dark" else "conversion-light"))
    }
    @Test fun light() = shoot(false)
    @Test fun dark() = shoot(true)
}
