package com.thekozugroup.popcornstash.screenshots

import androidx.activity.ComponentActivity
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.github.takahirom.roborazzi.ExperimentalRoborazziApi
import com.github.takahirom.roborazzi.captureScreenRoboImage
import com.thekozugroup.popcornstash.feature.downloads.findLink
import com.thekozugroup.popcornstash.share.ShareActions
import com.thekozugroup.popcornstash.share.ShareSheet
import com.thekozugroup.popcornstash.share.ShareSheetState
import com.thekozugroup.popcornstash.share.SharedFile
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/**
 * The share sheet over another app (a flat stand-in for the app that shared), light and dark.
 * Recorded with `./gradlew recordRoborazziDebug`.
 */
@OptIn(ExperimentalRoborazziApi::class)
@RunWith(AndroidJUnit4::class)
@Config(qualifiers = PHONE)
class ShareScreenshotTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val magnet = findLink("magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Harbor.Lights.2019.1080p.BluRay.x264")!!
    private val photos = listOf(
        SharedFile("content://media/1", "PXL_20261012_181502.mp4", 84_200_000, "video/mp4"),
        SharedFile("content://media/2", "PXL_20261012_181733.jpg", 3_100_000, "image/jpeg"),
        SharedFile("content://media/3", "PXL_20261012_182010.jpg", 2_870_000, "image/jpeg"),
        SharedFile("content://media/4", "Grandma reading.jpg", 2_440_000, "image/jpeg"),
        SharedFile("content://media/5", "Party invitation.pdf", 412_000, "application/pdf"),
    )

    private fun show(state: ShareSheetState, dark: Boolean, name: String) {
        compose.setContent {
            PopcornTheme(darkTheme = dark, reduceMotion = true) {
                // The app that shared, behind the sheet.
                Box(Modifier.fillMaxSize().background(if (dark) Color(0xFF1F2A33) else Color(0xFFE9EEF2)))
                ShareSheet(state, ShareActions())
            }
        }
        compose.waitForIdle()
        captureScreenRoboImage(screenshot(name))
    }

    @Test
    fun linkLight() = show(ShareSheetState.Link(magnet), dark = false, name = "share-link-light")

    @Test
    fun linkDark() = show(ShareSheetState.Link(magnet), dark = true, name = "share-link-dark")

    @Test
    fun linkAddedLight() = show(ShareSheetState.Link(magnet, addedName = "Harbor.Lights.2019.1080p.BluRay.x264"), dark = false, name = "share-link-added-light")

    @Test
    fun linkRefusedDark() = show(
        ShareSheetState.Link(magnet, error = "Torrents are turned off on this server. Web links and uploads still work."),
        dark = true,
        name = "share-link-error-dark",
    )

    @Test
    fun filesLight() = show(
        ShareSheetState.Files(photos, refused = 0, userName = "Ana Ribeiro", server = "stash.example.com"),
        dark = false,
        name = "share-files-light",
    )

    @Test
    fun filesDark() = show(
        ShareSheetState.Files(photos.take(2), refused = 1, userName = "Ana Ribeiro", server = "stash.example.com"),
        dark = true,
        name = "share-files-dark",
    )

    @Test
    fun signedOutLight() = show(ShareSheetState.SignedOut, dark = false, name = "share-signed-out-light")

    @Test
    fun checkingDark() = show(ShareSheetState.Checking, dark = true, name = "share-checking-dark")

    @Test
    fun unusableLight() = show(ShareSheetState.Unusable(refused = 0), dark = false, name = "share-unusable-light")
}
