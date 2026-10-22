package com.thekozugroup.popcornstash.feature.settings

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.thekozugroup.popcornstash.data.api.*
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

@RunWith(AndroidJUnit4::class)
@Config(qualifiers = "w411dp-h2400dp-mdpi")
class LibraryConversionPageTest {
    @get:Rule val compose = createAndroidComposeRule<ComponentActivity>()
    @Test fun clearIsSeparateFromPolicySaveAndKeepsActiveOnlyDisabled() {
        var clears = 0
        var saves = 0
        val policy = ConversionPolicy(enabled = true)
        val view = LibraryConversion(policy = policy, jobs = listOf(ConversionJob("one", title = "Synthetic video", state = "complete")))
        compose.setContent { PopcornTheme {
            LibraryConversionPage(ConversionUi(view, policy.copy(quality = "small")), {}, { saves++ }, {}, {}, onClearActivity = { clears++ })
        } }
        compose.onNodeWithText("Clear finished activity").performScrollTo().performClick()
        assertEquals(1, clears)
        assertEquals(0, saves)
    }
    @Test fun clearingDoesNotRemoveTheAbilityToRetryHiddenFailures() {
        val policy = ConversionPolicy(enabled = true)
        val view = LibraryConversion(policy = policy, retryable = 2)
        compose.setContent { PopcornTheme {
            LibraryConversionPage(ConversionUi(view, policy), {}, {}, {}, {})
        } }
        compose.onNodeWithText("Clear finished activity").performScrollTo().assertIsNotEnabled()
        compose.onNodeWithText("Retry stopped conversions").performScrollTo().assertIsEnabled()
    }
}
