package com.thekozugroup.popcornstash.screenshots

import androidx.activity.ComponentActivity
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import com.github.takahirom.roborazzi.ExperimentalRoborazziApi
import com.github.takahirom.roborazzi.captureScreenRoboImage
import com.thekozugroup.popcornstash.data.api.TorrentFile
import com.thekozugroup.popcornstash.data.api.TorrentFileKind
import com.thekozugroup.popcornstash.feature.downloads.DownloadsSamples
import com.thekozugroup.popcornstash.feature.downloads.TorrentFilesSheet
import com.thekozugroup.popcornstash.feature.downloads.TorrentFilesState
import com.thekozugroup.popcornstash.feature.downloads.TorrentRuleDraft
import com.thekozugroup.popcornstash.ui.theme.Popcorn
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

@OptIn(ExperimentalRoborazziApi::class)
@RunWith(AndroidJUnit4::class)
@Config(qualifiers = PHONE)
class TorrentFilesScreenshotTest {
    @get:Rule val compose = createAndroidComposeRule<ComponentActivity>()
    private val files = listOf(
        TorrentFile(2, "Harbor Lights/Harbor Lights (2019).mkv", 8_000_000_000L, TorrentFileKind.Video, true),
        TorrentFile(5, "Harbor Lights/Subtitles/English.srt", 54_000, TorrentFileKind.Subtitle, true),
        TorrentFile(8, "Harbor Lights/Subtitles/Spanish.srt", 52_000, TorrentFileKind.Subtitle, false),
        TorrentFile(9, "Harbor Lights/Artwork/poster.jpg", 430_000, TorrentFileKind.Image, false),
        TorrentFile(12, "Harbor Lights/Read me.txt", 800, TorrentFileKind.Other, false),
    )
    private val ready = TorrentFilesState(
        DownloadsSamples.downloading.copy(name = "Harbor Lights (2019)"),
        loading = false,
        ready = true,
        reviewRequired = true,
        files = files,
        selected = setOf(2),
    )

    private fun capture(name: String, state: TorrentFilesState = ready, dark: Boolean = false, checkContent: () -> Unit = {}) {
        compose.setContent {
            PopcornTheme(darkTheme = dark, reduceMotion = true) {
                Box(Modifier.fillMaxSize().background(Popcorn.colors.page)) {
                    TorrentFilesSheet(state, {}, {}, {}, {})
                }
            }
        }
        compose.waitForIdle()
        checkContent()
        captureScreenRoboImage(screenshot(name))
    }

    private val transfer = ready.copy(
        download = DownloadsSamples.downloading.copy(
            name = "Synthetic transfer", progress = 0.93,
            doneBytes = 2_790_000_000L, totalBytes = 3_000_000_000L,
            downBps = 0, upBps = 0, peers = 0, etaS = null,
        ),
        reviewRequired = false,
        files = listOf(
            TorrentFile(2, "clip-a.mkv", 1_000_000_000, TorrentFileKind.Video, true, verifiedBytes = 1_000_000_000, complete = true),
            TorrentFile(5, "clip-b.mkv", 1_000_000_000, TorrentFileKind.Video, true, verifiedBytes = 930_000_000, complete = false),
            TorrentFile(8, "captions.srt", 20_000, TorrentFileKind.Subtitle, false, verifiedBytes = 400, complete = false),
            TorrentFile(12, "clip-c.mkv", 1_000_000_000, TorrentFileKind.Video, true, verifiedBytes = 860_000_000, complete = false),
        ),
        selected = setOf(2, 5, 12),
    )

    private fun captureTransfer(name: String, dark: Boolean = false) = capture(name, transfer, dark) {
        compose.onNodeWithText("Finish completed").assertIsDisplayed().assertIsEnabled()
        compose.onNodeWithText("93%", substring = true).assertIsDisplayed()
        compose.onNodeWithText("Skipped", substring = true).assertIsDisplayed()
        for (file in transfer.files) {
            val skip = compose.onNodeWithContentDescription("Skip ${file.path}")
            if (file.selected && file.isVerifiedIncomplete) skip.assertIsDisplayed().assertIsEnabled()
            else skip.assertDoesNotExist()
        }
    }

    @Test fun transferPhoneLight() = captureTransfer("torrent-transfer-phone-light")

    @Test @Config(qualifiers = "w800dp-h1100dp-xhdpi")
    fun transferTabletDark() = captureTransfer("torrent-transfer-tablet-dark", dark = true)

    @Test fun filtersLight() = capture("torrent-files-filters-light", ready.copy(draft = TorrentRuleDraft(enabled = true, include = "Harbor", minimum = "500")))
    @Test fun filtersDark() = capture("torrent-files-filters-dark", ready.copy(draft = TorrentRuleDraft(enabled = true)), dark = true)
    @Test fun filesLight() = capture("torrent-files-light")
    @Test fun filesDark() = capture("torrent-files-dark", dark = true)
    @Test fun noFilesSelectedLight() = capture("torrent-files-empty-light", ready.copy(selected = emptySet()))
    @Test fun metadataPendingDark() = capture("torrent-files-pending-dark", ready.copy(ready = false, files = emptyList(), selected = emptySet()), dark = true)
    @Test
    @Config(qualifiers = "w360dp-h800dp-xxhdpi", fontScale = 2.0f)
    fun largeTextLight() = capture("torrent-files-large-text-light")
}
