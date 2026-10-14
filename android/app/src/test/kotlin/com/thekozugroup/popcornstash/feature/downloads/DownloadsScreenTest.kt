package com.thekozugroup.popcornstash.feature.downloads

import androidx.activity.ComponentActivity
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollToKey
import androidx.compose.ui.test.performTextInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.thekozugroup.popcornstash.navigation.Tab
import com.thekozugroup.popcornstash.navigation.TabPage
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import androidx.compose.ui.unit.dp
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/** What each control on the Downloads page does, and what it says to a screen reader. */
@RunWith(AndroidJUnit4::class)
@Config(qualifiers = "w411dp-h891dp-xxhdpi")
class DownloadsScreenTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val s = DownloadsSamples
    private val calls = mutableListOf<String>()

    private val actions = DownloadsActions(
        onOpenVpnSettings = { calls += "vpn settings" },
        onPauseOrResume = { calls += "toggle ${it.id}" },
        onAskRemove = { calls += "remove ${it.id}" },
        onConfirmRemove = { calls += "confirm remove" },
        onOpenAdd = { calls += "open add" },
        onAddText = { calls += "text $it" },
        onSubmitAdd = { calls += "add" },
        onChoose = { id, target -> calls += "choose $id ${target.name}" },
        onAccept = { calls += "accept ${it.item.id}" },
        onEdit = { calls += "edit ${it.item.id}" },
        onIgnore = { calls += "ignore ${it.item.id}" },
        onAskAcceptAll = { calls += "accept all" },
        onStopUpload = { calls += "stop $it" },
        onOpenPlace = { calls += "open ${it.name}" },
    )

    private val shown = mutableStateOf<DownloadsUiState?>(null)
    private var started = false

    /** Shows [state]; a later call swaps the state on the same page (setContent runs once per test). */
    private fun show(state: DownloadsUiState) {
        shown.value = state
        if (!started) {
            started = true
            compose.setContent {
                PopcornTheme(darkTheme = false, reduceMotion = true) {
                    shown.value?.let { DownloadsPage(TabPage(Tab.Downloads, 120.dp, 0), it, actions, s.NOW) }
                }
            }
        }
        compose.waitForIdle()
    }

    private fun scrollTo(key: String) {
        compose.onNodeWithTag("list Downloads").performScrollToKey(key)
        compose.waitForIdle()
    }

    @Test
    fun pauseResumeAndRemoveNameTheDownload() {
        show(s.content())
        compose.onNodeWithContentDescription("Pause ${s.downloading.name}").performClick()
        compose.onNodeWithContentDescription("Resume ${s.paused.name}").performClick()
        compose.onNodeWithContentDescription("More for ${s.paused.name}").performClick()
        compose.onNodeWithText("Remove").performClick()
        assertEquals(listOf("toggle d-lantern", "toggle d-trailer", "remove d-trailer"), calls)
    }

    @Test
    fun theRemoveDialogSaysWhatHappensToTheFiles() {
        show(s.content().copy(removing = s.downloading))
        compose.onNodeWithText("Remove this download?").assertIsDisplayed()
        compose.onNodeWithText("What was downloaded so far is deleted.").assertIsDisplayed()
        compose.onNodeWithText("Remove").performClick()
        assertEquals(listOf("confirm remove"), calls)
    }

    @Test
    fun theFirstImportAcceptsEditsAndIgnores() {
        show(s.content())
        scrollTo("import i-clip")
        compose.onNode(hasTestTag("import i-clip")).assertIsDisplayed()
        compose.onAllNodesWithText("Accept").onFirst().performClick()
        compose.onAllNodesWithText("Edit").onFirst().performClick()
        compose.onNodeWithContentDescription("More for clip_final_v2.mkv").performClick()
        compose.onNodeWithText("Ignore").performClick()
        assertEquals(listOf("accept i-clip", "edit i-clip", "ignore i-clip"), calls)
    }

    @Test
    fun thePickerListsOnlyPlacesThatFitAndSendsTheChoice() {
        show(s.content())
        scrollTo("import i-clip")
        compose.onNodeWithTag("picker clip_final_v2.mkv")
            .assert(SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, "Movies"))
            .performClick()
        compose.onNodeWithText("Kids").assertIsDisplayed()
        compose.onNodeWithText("Kids").performClick()
        assertEquals(listOf("choose i-clip Kids"), calls)
    }

    @Test
    fun aPhotoIsOfferedOnlyPhotosAndOtherFiles() {
        show(s.content())
        scrollTo("import i-photo")
        compose.onNodeWithTag("picker IMG_4821.HEIC").performClick()
        compose.onAllNodesWithText("Movies").fetchSemanticsNodes().let { assertTrue(it.isEmpty()) }
        compose.onAllNodesWithText("Photos").fetchSemanticsNodes().let { assertEquals(2, it.size) }
    }

    @Test
    fun withoutALibraryAcceptWaitsForAChoice() {
        val row = s.row(s.lowConfidence.copy(suggestion = s.lowConfidence.suggestion!!.copy(libraryId = null, place = null)))
        show(s.content().copy(imports = listOf(row)))
        scrollTo("import i-clip")
        compose.onNodeWithText("Choose a library").assertIsDisplayed()
        compose.onAllNodesWithText("Accept").onFirst().assertIsNotEnabled()
        compose.onAllNodesWithText("Edit").onFirst().assertIsEnabled()
    }

    @Test
    fun aSeriesWithoutDetailsAsksForThemInsteadOfAccepting() {
        val row = s.row(s.lowConfidence, chosen = ImportTarget(s.kids))
        show(s.content().copy(imports = listOf(row)))
        scrollTo("import i-clip")
        compose.onNodeWithText("Add details").performClick()
        assertEquals(listOf("accept i-clip"), calls)
    }

    @Test
    fun acceptAllIsOfferedForTwoOrMoreReadyFiles() {
        show(s.content())
        scrollTo("imports")
        compose.onNodeWithText("Accept all").performClick()
        assertEquals(listOf("accept all"), calls)
        show(s.content().copy(imports = s.content().imports.take(1)))
        scrollTo("imports")
        assertTrue(compose.onAllNodesWithText("Accept all").fetchSemanticsNodes().isEmpty())
    }

    @Test
    fun ignoredFilesWaitUnderAClosedDisclosure() {
        show(s.content())
        scrollTo("ignored")
        compose.onNodeWithText("Left where they are (1)").assertIsDisplayed()
        assertTrue(compose.onAllNodes(hasTestTag("import i-sample")).fetchSemanticsNodes().isEmpty())
        compose.onNodeWithText("Left where they are (1)").performClick()
        scrollTo("ignored i-sample")
        compose.onNode(hasTestTag("import i-sample")).assertIsDisplayed()
    }

    @Test
    fun aFinishedDownloadOpensTheLibraryItWentInto() {
        show(s.content())
        scrollTo("finished d-harbor")
        compose.onNode(hasText("In Movies") and hasClickAction()).performClick()
        assertEquals(listOf("open Movies"), calls)
    }

    @Test
    fun filesStillWaitingLeadBackToReadyToImport() {
        show(s.content())
        scrollTo("finished d-puddle-1")
        compose.onNodeWithText("2 waiting to import").performClick()
        compose.waitForIdle()
        compose.onNodeWithText("Ready to import").assertIsDisplayed()
    }

    @Test
    fun theOwnersVpnLineOpensTheSettingsAndAMembersDoesNot() {
        show(s.content())
        compose.onNodeWithTag("vpn line").assert(hasClickAction()).performClick()
        assertEquals(listOf("vpn settings"), calls)
        show(s.content(isOwner = false))
        compose.onNodeWithTag("vpn line").assert(SemanticsMatcher.keyNotDefined(SemanticsProperties.Role))
    }

    @Test
    fun anUploadFromThisPhoneCanBeStopped() {
        show(s.content())
        compose.onNodeWithContentDescription("More for ${s.upload.label}").performClick()
        compose.onNodeWithText("Stop uploading").performClick()
        assertEquals(listOf("stop batch-1"), calls)
    }

    @Test
    fun theTopBarAndTheEmptyPageBothAddALink() {
        show(s.empty)
        compose.onNodeWithText("Nothing downloading").assertIsDisplayed()
        compose.onNode(hasContentDescription("Add a link") and hasClickAction()).performClick()
        compose.onNode(hasText("Add a link") and hasClickAction()).performClick()
        assertEquals(listOf("open add", "open add"), calls)
    }

    @Test
    fun theAddSheetTakesALinkAndSaysWhatItIs() {
        show(s.empty.copy(add = AddLinkState(text = "https://example.org/clip.mp4", fromClipboard = true)))
        compose.onNodeWithText("From your clipboard. Web link from example.org. Downloads straight to PopcornStash.").assertIsDisplayed()
        compose.onNodeWithTag("link field").performTextInput("x")
        compose.onNode(hasText("Add") and hasClickAction()).performClick()
        assertEquals("add", calls.last())
        assertTrue(calls.any { it.startsWith("text ") })
    }

    @Test
    fun loadingAndFailuresShowTheirOwnPage() {
        show(s.loading)
        compose.onNodeWithContentDescription("Loading downloads").assertIsDisplayed()
        show(s.failedToLoad)
        compose.onNodeWithText("Downloads could not be loaded").assertIsDisplayed()
        compose.onNodeWithText("Not connected to the server. Trying again.").assertIsDisplayed()
        compose.onNodeWithText("Try again").assertIsDisplayed()
    }
}
