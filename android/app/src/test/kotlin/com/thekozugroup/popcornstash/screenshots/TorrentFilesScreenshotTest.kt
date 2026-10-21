package com.thekozugroup.popcornstash.screenshots

import androidx.activity.ComponentActivity
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.ui.Modifier
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

    private fun capture(name: String, state: TorrentFilesState = ready, dark: Boolean = false) {
        compose.setContent {
            PopcornTheme(darkTheme = dark, reduceMotion = true) {
                Box(Modifier.fillMaxSize().background(Popcorn.colors.page)) {
                    TorrentFilesSheet(state, {}, {}, {}, {})
                }
            }
        }
        compose.waitForIdle()
        captureScreenRoboImage(screenshot(name))
    }

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
