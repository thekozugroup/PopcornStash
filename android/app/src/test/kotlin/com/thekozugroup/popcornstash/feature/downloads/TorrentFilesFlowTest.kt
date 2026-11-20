package com.thekozugroup.popcornstash.feature.downloads

import androidx.activity.ComponentActivity
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasAnyDescendant
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isDialog
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performSemanticsAction
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.thekozugroup.popcornstash.data.api.TorrentFile
import com.thekozugroup.popcornstash.data.api.TorrentFileKind
import com.thekozugroup.popcornstash.data.api.TorrentFiles
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import kotlinx.coroutines.CompletableDeferred
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/** Drives the Downloads row, live sheet and controller against a synthetic in-memory manifest. */
@RunWith(AndroidJUnit4::class)
class TorrentFilesFlowTest {
    @get:Rule val compose = createAndroidComposeRule<ComponentActivity>()

    @Test @Config(qualifiers = "w411dp-h891dp-xxhdpi")
    fun reviewToFinishOnPhone() = exerciseTransfer()

    @Test @Config(qualifiers = "w800dp-h1100dp-xhdpi")
    fun reviewToFinishOnTablet() = exerciseTransfer()

    private fun exerciseTransfer() {
        val writes = mutableListOf<String>()
        val finishResponse = CompletableDeferred<Unit>()
        val download = DownloadsSamples.downloading.copy(name = "Synthetic transfer")
        val files = listOf(
            TorrentFile(4, "clip-a.mkv", 1000, TorrentFileKind.Video, true),
            TorrentFile(8, "captions.srt", 100, TorrentFileKind.Subtitle, true),
            TorrentFile(12, "clip-b.mkv", 1000, TorrentFileKind.Video, false),
        )
        compose.setContent {
            val scope = rememberCoroutineScope()
            var manifest by remember { mutableStateOf(TorrentFiles(true, files, reviewRequired = true)) }
            val controller = remember {
                TorrentFilesController(
                    load = { manifest },
                    save = { _, _ -> error("Unexpected selection replacement") },
                    scope = scope,
                    start = { _, indices ->
                        writes += "start $indices"
                        manifest = manifest.copy(reviewRequired = false, files = manifest.files.map {
                            it.copy(selected = it.index in indices, complete = it.index == 4,
                                verifiedBytes = if (it.index == 4) it.size else it.size / 2)
                        })
                    },
                    skip = { _, index ->
                        writes += "skip $index"
                        manifest = manifest.copy(files = manifest.files.map { if (it.index == index) it.copy(selected = false) else it })
                    },
                    finish = { _, request ->
                        writes += "finish selected=${request.expectedSelected} complete=${request.expectedComplete}"
                        finishResponse.await()
                    },
                )
            }
            val row = download.copy(error = if (manifest.reviewRequired) "Ready to choose files." else null)
            PopcornTheme(darkTheme = false, reduceMotion = true) {
                DownloadRow(row, busy = false, onPauseOrResume = {}, onRemove = {}, onChooseFiles = { controller.open(row) })
                TorrentFilesDialog(controller)
            }
        }

        fun open(label: String) {
            compose.onNodeWithContentDescription("More for ${download.name}").performClick()
            compose.onNodeWithText(label).performClick()
        }
        open("Choose files")
        compose.onNodeWithText("Review torrent").assertIsDisplayed()
        compose.onNodeWithText("Finish completed").assertDoesNotExist()
        compose.onNode(SemanticsMatcher.keyIsDefined(SemanticsActions.Dismiss))
            .performSemanticsAction(SemanticsActions.Dismiss)
        compose.onNodeWithTag("torrent files").assertDoesNotExist()
        assertEquals(emptyList<String>(), writes)

        open("Choose files")
        compose.onNodeWithText("Filter").performScrollTo().performClick()
        compose.onNodeWithText("Start download").performClick()
        compose.onNodeWithText("Torrent files").assertIsDisplayed()
        compose.onNodeWithText("Start download").assertDoesNotExist()
        assertEquals(listOf("start [4, 8, 12]"), writes)

        compose.onNodeWithText("100%", substring = true).assertIsDisplayed()
        compose.onNodeWithContentDescription("Skip captions.srt").performClick()
        compose.onNodeWithText("Cancel").performClick()
        compose.onNodeWithText("Close").performClick()
        compose.onNodeWithTag("torrent files").assertDoesNotExist()
        assertEquals(listOf("start [4, 8, 12]"), writes)

        open("Files and progress")
        compose.onNodeWithContentDescription("Skip captions.srt").performClick()
        compose.onNodeWithText("Skip file").performClick()
        compose.onNodeWithText("Skipped · 50%", substring = true).assertIsDisplayed()
        compose.onNodeWithText("Finish completed").performClick()
        compose.onNodeWithText("1 of 2 selected files verified", substring = true).assertIsDisplayed()
        compose.onNodeWithText("Cancel").performClick()
        assertEquals(listOf("start [4, 8, 12]", "skip 8"), writes)

        compose.onNodeWithText("Finish completed").performClick()
        val confirmation = isDialog() and hasAnyDescendant(hasText("Finish completed files?"))
        compose.onNode(hasText("Finish completed") and hasAnyAncestor(confirmation)).performClick()
        compose.onNodeWithText("Close").assertIsNotEnabled()
        compose.runOnIdle { finishResponse.complete(Unit) }
        compose.onNodeWithTag("torrent files").assertDoesNotExist()
        assertEquals(listOf("start [4, 8, 12]", "skip 8", "finish selected=[4, 12] complete=[4]"), writes)
    }
}
