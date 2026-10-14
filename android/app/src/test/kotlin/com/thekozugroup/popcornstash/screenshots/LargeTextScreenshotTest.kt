package com.thekozugroup.popcornstash.screenshots

import androidx.activity.ComponentActivity
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performScrollTo
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.github.takahirom.roborazzi.ExperimentalRoborazziApi
import com.github.takahirom.roborazzi.captureRoboImage
import com.github.takahirom.roborazzi.captureScreenRoboImage
import com.thekozugroup.popcornstash.data.api.ContentType
import com.thekozugroup.popcornstash.data.api.Category
import com.thekozugroup.popcornstash.data.api.Structure
import com.thekozugroup.popcornstash.data.api.Visibility
import com.thekozugroup.popcornstash.data.repo.Loadable
import com.thekozugroup.popcornstash.feature.downloads.DownloadsActions
import com.thekozugroup.popcornstash.feature.downloads.DownloadsPage
import com.thekozugroup.popcornstash.feature.downloads.DownloadsSamples
import com.thekozugroup.popcornstash.feature.downloads.findLink
import com.thekozugroup.popcornstash.feature.library.LibraryActions
import com.thekozugroup.popcornstash.feature.library.LibraryScreen
import com.thekozugroup.popcornstash.feature.library.LibraryViewModel
import com.thekozugroup.popcornstash.feature.library.Paged
import com.thekozugroup.popcornstash.feature.settings.EditorLoad
import com.thekozugroup.popcornstash.feature.settings.FolderMode
import com.thekozugroup.popcornstash.feature.settings.LibraryDraft
import com.thekozugroup.popcornstash.feature.settings.LibraryEditorActions
import com.thekozugroup.popcornstash.feature.settings.LibraryEditorPage
import com.thekozugroup.popcornstash.feature.settings.LibraryEditorUi
import com.thekozugroup.popcornstash.feature.settings.LibrarySettingsUi
import com.thekozugroup.popcornstash.feature.settings.SettingsFixtures
import com.thekozugroup.popcornstash.feature.settings.SettingsRootPage
import com.thekozugroup.popcornstash.feature.signin.ServerInfo
import com.thekozugroup.popcornstash.feature.signin.SignInMethodPage
import com.thekozugroup.popcornstash.feature.signin.SignInStep
import com.thekozugroup.popcornstash.feature.signin.SignInUiState
import com.thekozugroup.popcornstash.navigation.AppShell
import com.thekozugroup.popcornstash.navigation.Tab
import com.thekozugroup.popcornstash.share.ShareActions
import com.thekozugroup.popcornstash.share.ShareSheet
import com.thekozugroup.popcornstash.share.ShareSheetState
import com.thekozugroup.popcornstash.testing.SampleMedia
import com.thekozugroup.popcornstash.testing.WithSampleImages
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import okhttp3.HttpUrl.Companion.toHttpUrl
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/** A smaller phone (360dp wide) with the largest font size Android offers. */
private const val SMALL_PHONE = "w360dp-h800dp-xxhdpi"
private const val LARGEST_TEXT = 2f

/** A family server with a long address, the kind that has to wrap. */
private const val LONG_HOST = "popcorn-stash.family-home.example.com"

/**
 * The places where long words meet a narrow screen: a server address, a folder name in a
 * sentence, a section header with its action, a photo month with its count and a link. Each
 * wraps at a space, a dot or a hyphen, never inside a word, and a name or a date stays whole
 * when it fits on a line. Recorded with `./gradlew recordRoborazziDebug`.
 */
@OptIn(ExperimentalRoborazziApi::class)
@RunWith(AndroidJUnit4::class)
@Config(qualifiers = SMALL_PHONE, fontScale = LARGEST_TEXT)
class LargeTextScreenshotTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val server = ServerInfo(
        url = "https://$LONG_HOST/".toHttpUrl(),
        display = LONG_HOST,
        version = "0.2.0",
        passkeysOffered = true,
        setupRequired = false,
    )

    /** Folders named the way families name them: a space and an apostrophe. */
    private val folders = SettingsFixtures.folders().map { if (it.name == "Grandma's scans") it.copy(name = "Grandma's Slides") else it }

    @Test
    fun signInWithALongServerAddress() {
        compose.setContent {
            PopcornTheme(darkTheme = false, reduceMotion = true) {
                SignInMethodPage(SignInUiState(step = SignInStep.Method, server = server), {}, {}, {}, {}, {})
            }
        }
        compose.waitForIdle()
        compose.onRoot().captureRoboImage(screenshot("large-text-signin-light"))
    }

    @Test
    fun settingsWithALongServerAddress() {
        compose.setContent {
            PopcornTheme(darkTheme = false, reduceMotion = true) {
                AppShell(initialTab = Tab.Settings) { page ->
                    if (page.tab == Tab.Settings) {
                        SettingsRootPage(
                            page, SampleOwner, LONG_HOST, "0.2.0",
                            LibrarySettingsUi(libraries = Loadable.Ready(SettingsFixtures.libraries()), people = SettingsFixtures.people(), folders = folders),
                            onOpenWeb = {}, onSignOut = {},
                        )
                    }
                }
            }
        }
        compose.waitForIdle()
        compose.onRoot().captureRoboImage(screenshot("large-text-settings-light"))
    }

    @Test
    @Config(qualifiers = "w360dp-h1400dp-xxhdpi", fontScale = LARGEST_TEXT)
    fun aFolderNamedInASentence() {
        val folder = folders.first { it.name == "Grandma's Slides" }
        val state = LibraryEditorUi(
            isNew = true,
            load = EditorLoad.Ready,
            draft = LibraryDraft.new(folder, folder.name).copy(folderMode = FolderMode.Existing, visibility = Visibility.Everyone),
            folders = folders,
            people = SettingsFixtures.people(),
            libraries = SettingsFixtures.libraries(),
        )
        compose.setContent {
            PopcornTheme(darkTheme = false, reduceMotion = true) {
                AppShell(initialTab = Tab.Settings) { page ->
                    if (page.tab == Tab.Settings) LibraryEditorPage(state, NoEditorActions, onBack = {}, bottomClearance = page.bottomClearance)
                }
            }
        }
        compose.waitForIdle()
        // The folder group, its picker and the sentence under it, above the save button.
        compose.onNodeWithTag("save library").performScrollTo()
        compose.waitForIdle()
        compose.onRoot().captureRoboImage(screenshot("large-text-editor-folder-light"))
    }

    @Test
    fun readyToImportWithItsAction() {
        compose.setContent {
            PopcornTheme(darkTheme = false, reduceMotion = true) {
                WithSampleImages {
                    AppShell(initialTab = Tab.Downloads, readyToImport = DownloadsSamples.content().imports.size) { page ->
                        DownloadsPage(
                            page, DownloadsSamples.content(), DownloadsActions(onOpenPlace = {}), DownloadsSamples.NOW,
                            // The list: VPN, In progress header, the upload, 3 downloads, then Ready to import.
                            listState = LazyListState(firstVisibleItemIndex = 5),
                        )
                    }
                }
            }
        }
        compose.waitForIdle()
        compose.onRoot().captureRoboImage(screenshot("large-text-downloads-light"))
    }

    @Test
    fun photoMonthsWithTheirCounts() {
        compose.setContent {
            PopcornTheme(darkTheme = false, reduceMotion = true) {
                WithSampleImages {
                    AppShell(initialTab = Tab.Home) { page ->
                        if (page.tab == Tab.Home) {
                            LibraryScreen(
                                "Photos", "image",
                                LibraryViewModel.Ui(SampleMedia.photos, LibraryViewModel.Content.Timeline(Paged(SampleMedia.timeline))),
                                true, page.bottomClearance, LibraryActions(), format = SampleFormat,
                            )
                        }
                    }
                }
            }
        }
        compose.waitForIdle()
        compose.onRoot().captureRoboImage(screenshot("large-text-timeline-light"))
    }

    @Test
    fun aLinkFromALongAddress() {
        val link = findLink("https://$LONG_HOST/clips/old-pier-at-sunset-1999.mp4")!!
        compose.setContent {
            PopcornTheme(darkTheme = false, reduceMotion = true) {
                Box(Modifier.fillMaxSize().background(Color(0xFFE9EEF2)))
                ShareSheet(ShareSheetState.Link(link), ShareActions())
            }
        }
        compose.waitForIdle()
        captureScreenRoboImage(screenshot("large-text-share-link-light"))
    }
}

private object NoEditorActions : LibraryEditorActions {
    override fun load() = Unit
    override fun setName(value: String) = Unit
    override fun setIcon(icon: String) = Unit
    override fun setType(contentType: ContentType, structure: Structure?) = Unit
    override fun toggleClaim(kind: Category) = Unit
    override fun setFolderMode(mode: FolderMode) = Unit
    override fun setNewFolder(value: String) = Unit
    override fun chooseFolder(name: String) = Unit
    override fun setVisibility(visibility: Visibility) = Unit
    override fun toggleMember(id: String) = Unit
    override fun save() = Unit
    override fun confirmClaims() = Unit
    override fun cancelClaims() = Unit
    override fun askDelete() = Unit
    override fun cancelDelete() = Unit
    override fun confirmDelete() = Unit
}
