package com.thekozugroup.popcornstash.screenshots

import androidx.activity.ComponentActivity
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHostState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollToIndex
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.github.takahirom.roborazzi.captureRoboImage
import com.thekozugroup.popcornstash.data.api.Library
import com.thekozugroup.popcornstash.data.repo.Loadable
import com.thekozugroup.popcornstash.feature.item.ItemActions
import com.thekozugroup.popcornstash.feature.item.ItemScreen
import com.thekozugroup.popcornstash.feature.item.ItemViewModel
import com.thekozugroup.popcornstash.feature.library.ItemSort
import com.thekozugroup.popcornstash.feature.library.LibraryActions
import com.thekozugroup.popcornstash.feature.library.LibraryScreen
import com.thekozugroup.popcornstash.feature.library.LibraryViewModel.Content
import com.thekozugroup.popcornstash.feature.library.LibraryViewModel.Ui
import com.thekozugroup.popcornstash.feature.library.OtherFilesActions
import com.thekozugroup.popcornstash.feature.library.OtherFilesScreen
import com.thekozugroup.popcornstash.feature.library.OtherFilesViewModel
import com.thekozugroup.popcornstash.feature.library.Paged
import com.thekozugroup.popcornstash.feature.library.ShowActions
import com.thekozugroup.popcornstash.feature.library.ShowScreen
import com.thekozugroup.popcornstash.feature.library.ShowViewModel
import com.thekozugroup.popcornstash.feature.photos.PhotoViewer
import com.thekozugroup.popcornstash.navigation.AppShell
import com.thekozugroup.popcornstash.navigation.Tab
import com.thekozugroup.popcornstash.navigation.TabPage
import com.thekozugroup.popcornstash.testing.SampleMedia
import com.thekozugroup.popcornstash.testing.WithSampleImages
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/**
 * The pages Home opens, as a phone shows them, light and dark: each library layout (posters,
 * frames, series, timeline) with its loading, empty and failed states, a show, an item, the
 * photo viewer and Other files.
 */
@RunWith(AndroidJUnit4::class)
@Config(qualifiers = PHONE)
class LibraryScreenshotTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private fun shoot(name: String, dark: Boolean, before: () -> Unit = {}, content: @Composable (TabPage) -> Unit) {
        compose.setContent {
            PopcornTheme(darkTheme = dark, reduceMotion = true) {
                WithSampleImages {
                    AppShell(initialTab = Tab.Home) { page -> if (page.tab == Tab.Home) content(page) }
                }
            }
        }
        compose.waitForIdle()
        before()
        compose.waitForIdle()
        compose.onRoot().captureRoboImage(screenshot(name))
    }

    private fun library(name: String, dark: Boolean, library: Library, content: Content, owner: Boolean = true, before: () -> Unit = {}) =
        shoot(name, dark, before) { page ->
            LibraryScreen(library.name, library.icon, Ui(library, content), owner, page.bottomClearance, LibraryActions(), format = SampleFormat)
        }

    private val movies = Content.Items(
        ItemSort.Added,
        Paged(SampleMedia.recentMovies + SampleMedia.recentMovies.map { it.copy(id = it.id + "-2", progress = null) }),
    )
    private val homeVideos = Content.Items(
        ItemSort.Taken,
        Paged(listOf(SampleMedia.beachDay, SampleMedia.bikeRide, SampleMedia.birthday, SampleMedia.lakeMorning).let { it + it.map { v -> v.copy(id = v.id + "-2") } }),
    )

    // ------------------------------------------------------------------ libraries ----

    @Test
    fun postersLight() = library("library-posters-light", false, SampleMedia.movies, movies)

    @Test
    fun postersDark() = library("library-posters-dark", true, SampleMedia.movies, movies, owner = false)

    /** The poster grid scrolled to its end: the last row of posters and their titles sit above the bar. */
    private fun postersAtTheEnd(name: String, dark: Boolean) = library(name, dark, SampleMedia.movies, movies) {
        compose.onNodeWithTag("library grid").performScrollToIndex(movies.page.items.size - 1)
        compose.waitForIdle()
        compose.assertNothingUnderTheToolbar()
    }

    @Test
    fun postersAtTheEndLight() = postersAtTheEnd("library-posters-end-light", false)

    @Test
    fun postersAtTheEndDark() = postersAtTheEnd("library-posters-end-dark", true)

    @Test
    fun sortMenuLight() = library("library-sort-light", false, SampleMedia.movies, movies) {
        compose.onNodeWithContentDescription("Sort").performClick()
    }

    @Test
    fun framesLight() = library("library-frames-light", false, SampleMedia.homeVideos, homeVideos)

    @Test
    fun framesDark() = library("library-frames-dark", true, SampleMedia.homeVideos, homeVideos)

    @Test
    fun seriesLight() = library("library-series-light", false, SampleMedia.shows, Content.Shows(SampleMedia.showSummaries))

    @Test
    fun seriesDark() = library("library-series-dark", true, SampleMedia.shows, Content.Shows(SampleMedia.showSummaries))

    @Test
    fun timelineLight() = library("library-timeline-light", false, SampleMedia.photos, Content.Timeline(Paged(SampleMedia.timeline)))

    @Test
    fun timelineDark() = library("library-timeline-dark", true, SampleMedia.photos, Content.Timeline(Paged(SampleMedia.timeline)))

    @Test
    fun loadingPostersLight() = library("library-loading-light", false, SampleMedia.movies, Content.Loading)

    @Test
    fun loadingTimelineDark() = library("library-loading-timeline-dark", true, SampleMedia.photos, Content.Loading)

    @Test
    fun emptyOwnerLight() = library("library-empty-light", false, SampleMedia.kids, Content.Shows(emptyList()))

    @Test
    fun emptyMemberDark() = library("library-empty-dark", true, SampleMedia.kids, Content.Shows(emptyList()), owner = false)

    @Test
    fun failedDark() = library(
        "library-failed-dark",
        true,
        SampleMedia.movies,
        Content.Failed("PopcornStash did not answer. Check the connection and try again."),
    )

    // ---------------------------------------------------------------------- shows ----

    private fun show(name: String, dark: Boolean, ui: ShowViewModel.Ui) = shoot(name, dark) { page ->
        ShowScreen("The Lantern Keepers", "tv", ui, page.bottomClearance, ShowActions(), format = SampleFormat)
    }

    @Test
    fun showLight() = show("show-light", false, ShowViewModel.Ui(Loadable.Ready(SampleMedia.lanternShow), season = 1))

    @Test
    fun showDark() = show("show-dark", true, ShowViewModel.Ui(Loadable.Ready(SampleMedia.lanternShow), season = 1))

    @Test
    fun showLoadingLight() = show("show-loading-light", false, ShowViewModel.Ui(Loadable.Loading))

    @Test
    fun showFailedDark() = show("show-failed-dark", true, ShowViewModel.Ui(Loadable.Failed("That show could not be loaded. Try again in a moment.")))

    // ---------------------------------------------------------------------- items ----

    private fun item(name: String, dark: Boolean, ui: ItemViewModel.Ui, icon: String? = "film") = shoot(name, dark) { page ->
        ItemScreen("Paper Boats", ui, page.bottomClearance, ItemActions(), libraryIcon = icon, format = SampleFormat)
    }

    @Test
    fun itemLight() = item("item-light", false, ItemViewModel.Ui(Loadable.Ready(SampleMedia.paperBoatsDetail)))

    @Test
    fun itemDark() = item("item-dark", true, ItemViewModel.Ui(Loadable.Ready(SampleMedia.paperBoatsDetail)))

    @Test
    fun episodeWithNextDark() = item(
        "item-episode-dark",
        true,
        ItemViewModel.Ui(Loadable.Ready(SampleMedia.episodeDetail), next = SampleMedia.lantern[2]),
        icon = "tv",
    )

    @Test
    fun itemLoadingLight() = item("item-loading-light", false, ItemViewModel.Ui(Loadable.Loading))

    @Test
    fun itemFailedDark() = item("item-failed-dark", true, ItemViewModel.Ui(Loadable.Failed("PopcornStash ran into a problem. Try again in a moment.")))

    // --------------------------------------------------------------- photo viewer ----

    private fun viewer(name: String, dark: Boolean, startId: String, controls: Boolean = true, message: String? = null) = shoot(name, dark) {
        val snackbar = remember { SnackbarHostState() }
        if (message != null) LaunchedEffect(Unit) { snackbar.showSnackbar(message, duration = SnackbarDuration.Indefinite) }
        PhotoViewer(
            photos = SampleMedia.timeline.flatMap { it.items },
            startId = startId,
            onClose = {},
            onDetails = {},
            onPlay = {},
            onSaveOriginal = {},
            initialControls = controls,
            format = SampleFormat,
            snackbar = snackbar,
        )
    }

    @Test
    fun viewerLight() = viewer("viewer-light", false, "s2")

    @Test
    fun viewerDark() = viewer("viewer-dark", true, "s2")

    /** "Original saved." sits on top of the plate, never over Save the original or Details. */
    @Test
    fun viewerSavedLight() = viewer("viewer-saved-light", false, "s2", message = "Original saved.")

    @Test
    fun viewerControlsHiddenDark() = viewer("viewer-bare-dark", true, "s5", controls = false)

    @Test
    fun viewerVideoDark() = viewer("viewer-video-dark", true, "s3")

    // ---------------------------------------------------------------- other files ----

    private fun other(name: String, dark: Boolean, ui: OtherFilesViewModel.Ui, owner: Boolean = false) = shoot(name, dark) { page ->
        OtherFilesScreen(ui, owner, page.bottomClearance, OtherFilesActions(), format = SampleFormat)
    }

    @Test
    fun otherFilesLight() = other("other-files-light", false, OtherFilesViewModel.Ui(Loadable.Ready(Paged(SampleMedia.otherFiles)), opening = "o-2"))

    @Test
    fun otherFilesDark() = other("other-files-dark", true, OtherFilesViewModel.Ui(Loadable.Ready(Paged(SampleMedia.otherFiles))), owner = true)

    @Test
    fun otherFilesEmptyLight() = other("other-files-empty-light", false, OtherFilesViewModel.Ui(Loadable.Ready(Paged(emptyList()))))

    @Test
    fun otherFilesLoadingDark() = other("other-files-loading-dark", true, OtherFilesViewModel.Ui(Loadable.Loading))
}
