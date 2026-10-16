package com.thekozugroup.popcornstash.privacy

import androidx.activity.ComponentActivity
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

@RunWith(AndroidJUnit4::class)
@Config(sdk = [36])
class PrivateContentTest {
    @get:Rule val compose = createAndroidComposeRule<ComponentActivity>()

    @Test fun privateDialogsCloseWhileNavigationAndFormsArePreserved() {
        var unlocked by mutableStateOf(true)
        compose.setContent {
            PopcornTheme {
                PrivateContent(unlocked, locked = { Text("Locked") }) {
                    var page by rememberSaveable { mutableStateOf("Home") }
                    var dialog by rememberSaveable { mutableStateOf(false) }
                    TextButton(onClick = { page = "Editing library"; dialog = true }) { Text(page) }
                    if (dialog) AlertDialog(onDismissRequest = { dialog = false }, title = { Text("Private draft") },
                        confirmButton = { TextButton(onClick = { dialog = false }) { Text("Save") } })
                }
            }
        }
        compose.onNodeWithText("Home").performClick()
        compose.onNodeWithText("Private draft").assertExists()
        compose.runOnIdle { unlocked = false }
        compose.onNodeWithText("Locked").assertExists()
        compose.onNodeWithText("Private draft").assertDoesNotExist()
        compose.onNodeWithText("Save").assertDoesNotExist()
        compose.runOnIdle { unlocked = true }
        compose.onNodeWithText("Private draft").assertExists()
        compose.onNodeWithText("Save").performClick()
        compose.onNodeWithText("Editing library").assertExists()
    }
}
