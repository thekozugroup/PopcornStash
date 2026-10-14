package com.thekozugroup.popcornstash.screenshots

import androidx.activity.ComponentActivity
import androidx.compose.runtime.Composable
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.github.takahirom.roborazzi.ExperimentalRoborazziApi
import com.github.takahirom.roborazzi.captureRoboImage
import com.github.takahirom.roborazzi.captureScreenRoboImage
import com.thekozugroup.popcornstash.data.api.Category
import com.thekozugroup.popcornstash.data.api.ContentType
import com.thekozugroup.popcornstash.data.api.FolderState
import com.thekozugroup.popcornstash.data.api.Role
import com.thekozugroup.popcornstash.data.api.Structure
import com.thekozugroup.popcornstash.data.api.User
import com.thekozugroup.popcornstash.data.api.Visibility
import com.thekozugroup.popcornstash.data.repo.Loadable
import com.thekozugroup.popcornstash.feature.settings.ClaimMove
import com.thekozugroup.popcornstash.feature.settings.EditorField
import com.thekozugroup.popcornstash.feature.settings.EditorLoad
import com.thekozugroup.popcornstash.feature.settings.FolderMode
import com.thekozugroup.popcornstash.feature.settings.LibraryDraft
import com.thekozugroup.popcornstash.feature.settings.LibraryEditorActions
import com.thekozugroup.popcornstash.feature.settings.LibraryEditorPage
import com.thekozugroup.popcornstash.feature.settings.LibraryEditorUi
import com.thekozugroup.popcornstash.feature.settings.LibrarySettingsUi
import com.thekozugroup.popcornstash.feature.settings.SettingsFixtures
import com.thekozugroup.popcornstash.feature.settings.SettingsRootPage
import com.thekozugroup.popcornstash.navigation.AppShell
import com.thekozugroup.popcornstash.navigation.Tab
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/** A phone tall enough to show a whole form at once, for review. */
private const val TALL_PHONE = "w411dp-h2400dp-xxhdpi"

/**
 * Settings and the library editor as a phone shows them, light and dark, in every state:
 * loading, failed, empty and with content. Recorded with `./gradlew recordRoborazziDebug`.
 */
@RunWith(AndroidJUnit4::class)
@Config(qualifiers = PHONE)
class SettingsScreenshotTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val libraries = SettingsFixtures.libraries().map {
        if (it.name == "Home Videos") it.copy(folderState = FolderState.Missing) else it
    }
    private val people = SettingsFixtures.people()
    private val folders = SettingsFixtures.folders()
    private val owner = SampleOwner
    private val member = User(id = "u-tomas", displayName = "Tomas Ribeiro", role = Role.Member)

    private fun settings(dark: Boolean, user: User = owner, state: LibrarySettingsUi?, uploadsRunning: Int = 0) {
        compose.setContent {
            PopcornTheme(darkTheme = dark, reduceMotion = true) {
                AppShell(initialTab = Tab.Settings) { page ->
                    if (page.tab == Tab.Settings) {
                        SettingsRootPage(page, user, "stash.example.com", "0.2.0", state, onOpenWeb = {}, onSignOut = {}, uploadsRunning = uploadsRunning)
                    } else {
                        SampleTabRoot(page)
                    }
                }
            }
        }
        compose.waitForIdle()
    }

    private fun editor(dark: Boolean, state: LibraryEditorUi) {
        compose.setContent {
            PopcornTheme(darkTheme = dark, reduceMotion = true) {
                AppShell(initialTab = Tab.Settings) { page -> EditorFor(page.tab, page.bottomClearance, state) }
            }
        }
        compose.waitForIdle()
    }

    @Composable
    private fun EditorFor(tab: Tab, clearance: androidx.compose.ui.unit.Dp, state: LibraryEditorUi) {
        if (tab == Tab.Settings) LibraryEditorPage(state, NoActions, onBack = {}, bottomClearance = clearance)
    }

    private fun shot(name: String) = compose.onRoot().captureRoboImage(screenshot(name))

    /** Menus and dialogs draw in windows of their own: capture every window. */
    @OptIn(ExperimentalRoborazziApi::class)
    private fun screen(name: String) = captureScreenRoboImage(screenshot(name))

    private val content = LibrarySettingsUi(libraries = Loadable.Ready(libraries), people = people, folders = folders)

    // ---------------------------------------------------------------- Settings ----

    @Test
    fun ownerLight() {
        settings(dark = false, state = content)
        shot("settings-owner-light")
    }

    @Test
    fun ownerDark() {
        settings(dark = true, state = content)
        shot("settings-owner-dark")
    }

    @Test
    @Config(qualifiers = TALL_PHONE)
    fun ownerWholePageLight() {
        settings(dark = false, state = content)
        shot("settings-owner-whole-light")
    }

    @Test
    @Config(qualifiers = TALL_PHONE)
    fun ownerWholePageDark() {
        settings(dark = true, state = content)
        shot("settings-owner-whole-dark")
    }

    @Test
    fun librariesLoadingLight() {
        settings(dark = false, state = LibrarySettingsUi(libraries = Loadable.Loading))
        shot("settings-loading-light")
    }

    @Test
    fun librariesLoadingDark() {
        settings(dark = true, state = LibrarySettingsUi(libraries = Loadable.Loading))
        shot("settings-loading-dark")
    }

    @Test
    fun librariesFailedLight() {
        settings(dark = false, state = LibrarySettingsUi(libraries = Loadable.Failed("PopcornStash did not answer. Check the connection and try again.")))
        shot("settings-failed-light")
    }

    @Test
    fun librariesFailedDark() {
        settings(dark = true, state = LibrarySettingsUi(libraries = Loadable.Failed("PopcornStash did not answer. Check the connection and try again.")))
        shot("settings-failed-dark")
    }

    @Test
    fun noLibrariesLight() {
        settings(dark = false, state = LibrarySettingsUi(libraries = Loadable.Ready(emptyList()), people = people, folders = folders.take(1)))
        shot("settings-empty-light")
    }

    @Test
    fun noLibrariesDark() {
        settings(dark = true, state = LibrarySettingsUi(libraries = Loadable.Ready(emptyList()), people = people, folders = folders.take(1)))
        shot("settings-empty-dark")
    }

    @Test
    fun memberLight() {
        settings(dark = false, user = member, state = null)
        shot("settings-member-light")
    }

    @Test
    fun memberDark() {
        settings(dark = true, user = member, state = null)
        shot("settings-member-dark")
    }

    @Test
    fun moreMenuLight() {
        settings(dark = false, state = content)
        compose.onNodeWithTag("more Shows").performClick()
        compose.waitForIdle()
        screen("settings-more-menu-light")
    }

    @Test
    fun moreMenuDark() {
        settings(dark = true, state = content)
        compose.onNodeWithTag("more Shows").performClick()
        compose.waitForIdle()
        screen("settings-more-menu-dark")
    }

    private fun signOutWithUploads(dark: Boolean) {
        settings(dark = dark, user = member, state = null, uploadsRunning = 2)
        compose.onNodeWithText("Sign out").performScrollTo().performClick()
        compose.waitForIdle()
    }

    @Test
    fun signOutWithUploadsLight() {
        signOutWithUploads(dark = false)
        screen("settings-sign-out-light")
    }

    @Test
    fun signOutWithUploadsDark() {
        signOutWithUploads(dark = true)
        screen("settings-sign-out-dark")
    }

    // ------------------------------------------------------------ the editor ----

    private val newLibrary = LibraryEditorUi(
        isNew = true,
        load = EditorLoad.Ready,
        draft = LibraryDraft(),
        folders = folders,
        people = people,
        libraries = libraries,
    )

    private val kids = newLibrary.copy(
        draft = LibraryDraft(
            name = "Kids",
            icon = "baby",
            iconTouched = true,
            contentType = ContentType.Video,
            structure = Structure.Episodes,
            claims = listOf(Category.Episode),
            newFolder = "Kids",
            visibility = Visibility.Selected,
            memberIds = listOf("u-tomas"),
        ),
    )

    private val fromFolder = newLibrary.copy(draft = LibraryDraft.new(folders[0], "Family 2019"))

    private val movies = libraries.first { it.name == "Movies" }
    private val editMovies = LibraryEditorUi(
        isNew = false,
        load = EditorLoad.Ready,
        library = movies,
        draft = LibraryDraft.from(movies),
        people = people,
        libraries = libraries,
    )

    @Test
    fun newLibraryLight() {
        editor(dark = false, newLibrary)
        shot("editor-new-light")
    }

    @Test
    fun newLibraryDark() {
        editor(dark = true, newLibrary)
        shot("editor-new-dark")
    }

    @Test
    @Config(qualifiers = TALL_PHONE)
    fun filledInWholeFormLight() {
        editor(dark = false, kids)
        shot("editor-filled-whole-light")
    }

    @Test
    @Config(qualifiers = TALL_PHONE)
    fun filledInWholeFormDark() {
        editor(dark = true, kids)
        shot("editor-filled-whole-dark")
    }

    @Test
    @Config(qualifiers = TALL_PHONE)
    fun fromAFolderWholeFormLight() {
        editor(dark = false, fromFolder)
        shot("editor-from-folder-whole-light")
    }

    @Test
    @Config(qualifiers = TALL_PHONE)
    fun editingALibraryThatHoldsFilesWholeFormLight() {
        editor(dark = false, editMovies)
        shot("editor-edit-whole-light")
    }

    @Test
    @Config(qualifiers = TALL_PHONE)
    fun editingALibraryThatHoldsFilesWholeFormDark() {
        editor(dark = true, editMovies.copy(library = movies.copy(folderState = FolderState.ReadOnly)))
        shot("editor-edit-whole-dark")
    }

    @Test
    @Config(qualifiers = TALL_PHONE)
    fun errorsWholeFormLight() {
        editor(
            dark = false,
            newLibrary.copy(
                draft = LibraryDraft(name = "Movies", newFolder = "Movies", newFolderTouched = true),
                errors = mapOf(EditorField.Name to "A library called Movies already exists."),
                formError = null,
            ),
        )
        shot("editor-field-error-whole-light")
    }

    @Test
    @Config(qualifiers = TALL_PHONE)
    fun saveFailedWholeFormDark() {
        editor(dark = true, kids.copy(formError = "PopcornStash did not answer. Check the connection and try again."))
        shot("editor-save-failed-whole-dark")
    }

    @Test
    fun loadingLight() {
        editor(dark = false, LibraryEditorUi(isNew = false))
        shot("editor-loading-light")
    }

    @Test
    fun loadingDark() {
        editor(dark = true, LibraryEditorUi(isNew = false, libraryId = movies.id, libraries = libraries))
        shot("editor-loading-dark")
    }

    @Test
    fun goneLight() {
        editor(dark = false, LibraryEditorUi(isNew = false, load = EditorLoad.Failed("This library is not here any more.", gone = true)))
        shot("editor-gone-light")
    }

    @Test
    fun failedDark() {
        editor(dark = true, LibraryEditorUi(isNew = false, load = EditorLoad.Failed("PopcornStash did not answer. Check the connection and try again.", gone = false)))
        shot("editor-failed-dark")
    }

    @Test
    fun confirmTakingAKindLight() {
        editor(dark = false, kids.copy(draft = kids.draft!!.copy(contentType = ContentType.Video, structure = Structure.OneOffs, claims = listOf(Category.Movie)), confirmingClaims = ClaimMove(listOf(Category.Movie), listOf("Movies"))))
        screen("editor-confirm-claims-light")
    }

    @Test
    fun confirmTakingAKindDark() {
        editor(dark = true, kids.copy(confirmingClaims = ClaimMove(listOf(Category.Episode), listOf("Shows"))))
        screen("editor-confirm-claims-dark")
    }

    @Test
    fun confirmDeleteLight() {
        editor(dark = false, editMovies.copy(confirmingDelete = true))
        screen("editor-confirm-delete-light")
    }

    @Test
    fun confirmDeleteDark() {
        editor(dark = true, editMovies.copy(confirmingDelete = true))
        screen("editor-confirm-delete-dark")
    }

    @Test
    fun existingFolderPickerLight() {
        editor(dark = false, newLibrary.copy(draft = LibraryDraft(folderMode = FolderMode.Existing)))
        // Scroll past the picker so the floating toolbar does not sit over it, then open it.
        compose.onNodeWithTag("save library").performScrollTo()
        compose.onNodeWithTag("folder picker").performClick()
        compose.waitForIdle()
        screen("editor-folder-picker-light")
    }
}

private object NoActions : LibraryEditorActions {
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
