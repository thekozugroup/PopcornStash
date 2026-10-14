package com.thekozugroup.popcornstash.share

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.thekozugroup.popcornstash.feature.downloads.findLink
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

@RunWith(AndroidJUnit4::class)
@Config(qualifiers = "w411dp-h891dp-xxhdpi")
class ShareSheetTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val calls = mutableListOf<String>()
    private val actions = ShareActions(
        onAddLink = { calls += "add" },
        onUpload = { calls += "upload" },
        onOpenApp = { calls += "open app" },
        onOpenDownloads = { calls += "open downloads" },
        onClose = { calls += "close" },
    )

    private fun show(state: ShareSheetState) {
        compose.setContent { PopcornTheme(darkTheme = false, reduceMotion = true) { ShareSheetContent(state, actions) } }
        compose.waitForIdle()
    }

    @Test
    fun aLinkIsAddedOnlyWhenThePersonSaysSo() {
        show(ShareSheetState.Link(findLink("magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Harbor+Lights")!!))
        compose.onNodeWithText("Add to Downloads?").assertIsDisplayed()
        compose.onNodeWithText("Magnet link. Downloads as a torrent, only through the VPN.").assertIsDisplayed()
        compose.onNodeWithText("Add to Downloads").performClick()
        compose.onNodeWithText("Cancel").performClick()
        assertEquals(listOf("add", "close"), calls)
    }

    @Test
    fun anAddedLinkOffersDownloads() {
        show(ShareSheetState.Link(findLink("https://example.org/a.mp4")!!, addedName = "a.mp4"))
        compose.onNodeWithText("Added to Downloads").assertIsDisplayed()
        compose.onNodeWithText("Open Downloads").performClick()
        assertEquals(listOf("open downloads"), calls)
    }

    @Test
    fun filesNameThreeAndCountTheRestAndSayWhoUploads() {
        val files = (1..5).map { SharedFile("content://m/$it", "PXL_2026101${it}_1815.jpg", 2_400_000L, "image/jpeg") }
        show(ShareSheetState.Files(files, refused = 0, userName = "Ana Ribeiro", server = "stash.example.com"))
        compose.onNodeWithText("Upload 5 files?").assertIsDisplayed()
        compose.onNodeWithText("PXL_20261013_1815.jpg").assertIsDisplayed()
        compose.onNodeWithText("and 2 more").assertIsDisplayed()
        compose.onNodeWithText("Uploads as Ana Ribeiro to stash.example.com.").assertIsDisplayed()
        compose.onNodeWithText("Upload").performClick()
        assertEquals(listOf("upload"), calls)
    }

    @Test
    fun signedOutAsksToSignInFirst() {
        show(ShareSheetState.SignedOut)
        compose.onNodeWithText("Sign in to PopcornStash first.").assertIsDisplayed()
        compose.onNodeWithText("Open PopcornStash").performClick()
        assertEquals(listOf("open app"), calls)
    }
}
