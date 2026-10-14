package com.thekozugroup.popcornstash.screenshots

import androidx.activity.ComponentActivity
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.runtime.Composable
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.github.takahirom.roborazzi.ExperimentalRoborazziApi
import com.github.takahirom.roborazzi.captureRoboImage
import com.github.takahirom.roborazzi.captureScreenRoboImage
import com.thekozugroup.popcornstash.data.api.VpnState
import com.thekozugroup.popcornstash.feature.downloads.AcceptAllState
import com.thekozugroup.popcornstash.feature.downloads.AddLinkState
import com.thekozugroup.popcornstash.feature.downloads.DownloadsActions
import com.thekozugroup.popcornstash.feature.downloads.DownloadsPage
import com.thekozugroup.popcornstash.feature.downloads.DownloadsSamples
import com.thekozugroup.popcornstash.feature.downloads.DownloadsUiState
import com.thekozugroup.popcornstash.feature.downloads.EditState
import com.thekozugroup.popcornstash.feature.downloads.ImportTarget
import com.thekozugroup.popcornstash.feature.downloads.initialMoveForm
import com.thekozugroup.popcornstash.feature.downloads.vpnLine
import com.thekozugroup.popcornstash.navigation.AppShell
import com.thekozugroup.popcornstash.navigation.Tab
import com.thekozugroup.popcornstash.testing.WithSampleImages
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/**
 * The Downloads tab, light and dark: content, loading, empty, error, the member's view, the
 * add and edit sheets and the dialogs. Recorded with `./gradlew recordRoborazziDebug`.
 */
@OptIn(ExperimentalRoborazziApi::class)
@RunWith(AndroidJUnit4::class)
@Config(qualifiers = PHONE)
class DownloadsScreenshotTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val s = DownloadsSamples

    private fun show(state: DownloadsUiState, dark: Boolean, list: LazyListState? = null) {
        compose.setContent {
            PopcornTheme(darkTheme = dark, reduceMotion = true) {
                Page(state, list)
            }
        }
        compose.waitForIdle()
    }

    @Composable
    private fun Page(state: DownloadsUiState, list: LazyListState?) {
        WithSampleImages {
            AppShell(initialTab = Tab.Downloads, readyToImport = state.imports.size) { page ->
                if (list != null) {
                    DownloadsPage(page, state, DownloadsActions(), s.NOW, listState = list)
                } else {
                    DownloadsPage(page, state, DownloadsActions(), s.NOW)
                }
            }
        }
    }

    private fun scrolledTo(index: Int) = LazyListState(firstVisibleItemIndex = index)

    @Test
    fun contentLight() {
        show(s.content(), dark = false)
        compose.onRoot().captureRoboImage(screenshot("downloads-light"))
    }

    @Test
    fun contentDark() {
        show(s.content(), dark = true)
        compose.onRoot().captureRoboImage(screenshot("downloads-dark"))
    }

    @Test
    fun readyToImportLight() {
        // The list: VPN, In progress header, the upload, 3 downloads, then Ready to import.
        show(s.content(), dark = false, list = scrolledTo(6))
        compose.onRoot().captureRoboImage(screenshot("downloads-import-light"))
    }

    @Test
    fun readyToImportDark() {
        show(s.content(), dark = true, list = scrolledTo(6))
        compose.onRoot().captureRoboImage(screenshot("downloads-import-dark"))
    }

    @Test
    fun finishedLight() {
        show(s.content(), dark = false, list = scrolledTo(12))
        compose.onRoot().captureRoboImage(screenshot("downloads-finished-light"))
    }

    @Test
    fun finishedDark() {
        show(s.content(), dark = true, list = scrolledTo(12))
        compose.onRoot().captureRoboImage(screenshot("downloads-finished-dark"))
    }

    @Test
    fun memberWhileTheVpnConnectsLight() {
        val member = s.content(isOwner = false).copy(
            vpn = vpnLine(s.vpn.copy(state = VpnState.Connecting), unavailable = false, isOwner = false),
        )
        show(member, dark = false, list = scrolledTo(5))
        compose.onRoot().captureRoboImage(screenshot("downloads-member-light"))
    }

    @Test
    fun loadingLight() {
        show(s.loading, dark = false)
        compose.onRoot().captureRoboImage(screenshot("downloads-loading-light"))
    }

    @Test
    fun loadingDark() {
        show(s.loading, dark = true)
        compose.onRoot().captureRoboImage(screenshot("downloads-loading-dark"))
    }

    @Test
    fun emptyLight() {
        show(s.empty, dark = false)
        compose.onRoot().captureRoboImage(screenshot("downloads-empty-light"))
    }

    @Test
    fun emptyDark() {
        show(s.empty, dark = true)
        compose.onRoot().captureRoboImage(screenshot("downloads-empty-dark"))
    }

    @Test
    fun errorLight() {
        show(s.failedToLoad, dark = false)
        compose.onRoot().captureRoboImage(screenshot("downloads-error-light"))
    }

    @Test
    fun errorDark() {
        show(s.failedToLoad, dark = true)
        compose.onRoot().captureRoboImage(screenshot("downloads-error-dark"))
    }

    @Test
    fun addLinkFromTheClipboardLight() {
        val add = AddLinkState(text = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Harbor+Lights+2019", fromClipboard = true)
        show(s.content().copy(add = add), dark = false)
        captureScreenRoboImage(screenshot("downloads-add-light"))
    }

    @Test
    fun addLinkFromTheClipboardDark() {
        val add = AddLinkState(text = "https://media.example.org/harbor-lights-trailer.mp4", fromClipboard = true)
        show(s.content().copy(add = add), dark = true)
        captureScreenRoboImage(screenshot("downloads-add-dark"))
    }

    @Test
    fun addLinkRefusedLight() {
        val add = AddLinkState(text = "example.org/clip.mp4", error = "Add https:// to the start of the web address.")
        show(s.empty.copy(add = add), dark = false)
        captureScreenRoboImage(screenshot("downloads-add-error-light"))
    }

    @Test
    fun editAMovieLight() {
        val row = s.content().imports.first()
        val edit = EditState(row.item.id, initialMoveForm(row.item, row.target).copy(title = "Paper Boats", year = "2011"))
        show(s.content().copy(edit = edit), dark = false)
        captureScreenRoboImage(screenshot("downloads-edit-light"))
    }

    @Test
    fun editAnEpisodeWithAServerErrorDark() {
        val row = s.content().imports.first()
        val form = initialMoveForm(row.item, ImportTarget(s.kids)).copy(show = "Puddle Club", season = "1", episode = "")
        val edit = EditState(
            row.item.id,
            form,
            errors = mapOf(com.thekozugroup.popcornstash.feature.downloads.MoveField.Episode to "Enter the episode number."),
        )
        show(s.content().copy(edit = edit), dark = true)
        captureScreenRoboImage(screenshot("downloads-edit-dark"))
    }

    @Test
    fun removeDialogLight() {
        show(s.content().copy(removing = s.downloading), dark = false)
        captureScreenRoboImage(screenshot("downloads-remove-light"))
    }

    @Test
    fun acceptAllDialogDark() {
        show(s.content().copy(acceptAll = AcceptAllState(total = 4)), dark = true, list = scrolledTo(6))
        captureScreenRoboImage(screenshot("downloads-accept-all-dark"))
    }

    @Test
    fun libraryPickerOpenLight() {
        show(s.content(), dark = false, list = scrolledTo(6))
        compose.onNodeWithTag("picker clip_final_v2.mkv").performClick()
        compose.waitForIdle()
        captureScreenRoboImage(screenshot("downloads-picker-light"))
    }
}
