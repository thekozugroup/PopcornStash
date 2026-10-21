package com.thekozugroup.popcornstash.feature.downloads

import androidx.activity.ComponentActivity
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertIsOff
import androidx.compose.ui.test.assertIsOn
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollToIndex
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.thekozugroup.popcornstash.data.api.TorrentFile
import com.thekozugroup.popcornstash.data.api.TorrentFileKind
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

@RunWith(AndroidJUnit4::class)
@Config(qualifiers = "w411dp-h891dp-xxhdpi")
class TorrentFilesSheetTest {
    @get:Rule val compose = createAndroidComposeRule<ComponentActivity>()
    private val files = listOf(
        TorrentFile(4, "Film.mkv", 8_000_000_000L, TorrentFileKind.Video, true),
        TorrentFile(8, "English.srt", 54_000, TorrentFileKind.Subtitle, false),
    )
    private val shown = mutableStateOf(TorrentFilesState(DownloadsSamples.downloading, loading = false, ready = true, files = files, selected = setOf(4)))
    private val calls = mutableListOf<String>()

    private fun show() {
        compose.setContent {
            PopcornTheme(darkTheme = false, reduceMotion = true) {
                TorrentFilesContent(
                    shown.value,
                    onToggle = { index ->
                        val state = shown.value
                        shown.value = state.copy(selected = if (index in state.selected) state.selected - index else state.selected + index)
                    },
                    onSave = { calls += "save" },
                    onCancel = { calls += "cancel" },
                    onRetry = { calls += "retry" },
                )
            }
        }
    }

    @Test
    fun savedChoicesAreVisibleAndSubtitlesAreFullyOptional() {
        show()
        compose.onNodeWithTag("torrent file list").performScrollToIndex(1)
        compose.onNodeWithTag("torrent file 4").assertIsOn()
        compose.onNodeWithTag("torrent file 8").assertIsOff().performClick().assertIsOn()
        compose.onNodeWithText("Save").assertIsEnabled()
        compose.onNodeWithTag("torrent file 8").performClick().assertIsOff()
        compose.onNodeWithText("Save").assertIsNotEnabled()
        compose.onNodeWithTag("torrent file 4").performClick()
        compose.onNodeWithText("Select at least one file.").assertExists()
        compose.onNodeWithText("Save").assertIsNotEnabled()
        compose.onNodeWithTag("torrent file 8").performClick()
        compose.onNodeWithText("Save").assertIsEnabled().performClick()
        assertEquals(listOf("save"), calls)
    }

    @Test
    fun metadataWaitingOffersRetryAndCancelWithoutSaving() {
        shown.value = shown.value.copy(ready = false, files = emptyList(), selected = emptySet())
        show()
        compose.onNodeWithText("Check again").performClick()
        compose.onNodeWithText("Save").assertIsNotEnabled()
        compose.onNodeWithText("Cancel").performClick()
        assertEquals(listOf("retry", "cancel"), calls)
    }

    @Test
    fun aLargeManifestScrollsToTheLastFileAndKeepsSaveVisible() {
        val many = (0..499).map { TorrentFile(it * 3, "Episode $it.mkv", 1000, TorrentFileKind.Video, true) }
        shown.value = shown.value.copy(files = many, selected = many.map { it.index }.toSet())
        show()
        compose.onNodeWithTag("torrent file list").performScrollToIndex(500)
        compose.onNodeWithTag("torrent file 1497").performClick().assertIsOff()
        compose.onNodeWithText("Save").assertIsEnabled().performClick()
        assertEquals(listOf("save"), calls)
    }

    @Test
    fun aSaveInProgressLocksChoicesAndCancel() {
        shown.value = shown.value.copy(busy = true)
        show()
        compose.onNodeWithTag("torrent file list").performScrollToIndex(1)
        compose.onNodeWithTag("torrent file 4").assertIsNotEnabled()
        compose.onNodeWithText("Save").assertIsNotEnabled()
        compose.onNodeWithText("Cancel").assertIsNotEnabled()
    }
}
