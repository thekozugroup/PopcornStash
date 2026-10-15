package com.thekozugroup.popcornstash.ui.components

import androidx.activity.ComponentActivity
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.thekozugroup.popcornstash.feature.downloads.DownloadsSamples
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class DownloadDestinationPickerTest {
    @get:Rule val compose = createAndroidComposeRule<ComponentActivity>()

    @Test fun aLibraryCanBeChosenAndReturnedToAutomatic() {
        val selected = mutableStateOf<String?>(null)
        compose.setContent {
            PopcornTheme {
                DownloadDestinationPicker(DownloadsSamples.libraries, selected.value, { selected.value = it })
            }
        }
        compose.onNodeWithText("Sorted automatically").performClick()
        compose.onNodeWithText("Movies").performClick()
        compose.runOnIdle { assertEquals("lib-movies", selected.value) }
        compose.onNodeWithText("Movies").performClick()
        compose.onNodeWithText("Sorted automatically").performClick()
        compose.runOnIdle { assertEquals(null, selected.value) }
    }
}
