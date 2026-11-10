package com.thekozugroup.popcornstash.screenshots

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollToIndex
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.github.takahirom.roborazzi.captureRoboImage
import com.thekozugroup.popcornstash.data.api.VideoCollection
import com.thekozugroup.popcornstash.data.api.VideoLibraryFeed
import com.thekozugroup.popcornstash.feature.library.LibraryFeedActions
import com.thekozugroup.popcornstash.feature.library.LibraryFeedScreen
import com.thekozugroup.popcornstash.feature.library.LibraryFeedViewModel
import com.thekozugroup.popcornstash.navigation.AppShell
import com.thekozugroup.popcornstash.navigation.Tab
import com.thekozugroup.popcornstash.testing.SampleMedia
import com.thekozugroup.popcornstash.testing.VideoFeedFixtures
import com.thekozugroup.popcornstash.testing.WithSampleImages
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/** Uses the incumbent screenshot fixtures, image loader, theme and real navigation shell. */
@RunWith(AndroidJUnit4::class)
@Config(qualifiers = PHONE)
class LibraryFeedScreenshotTest {
    @get:Rule val compose = createAndroidComposeRule<ComponentActivity>()
    private val feed = VideoFeedFixtures.movies.copy(nextCursor = null)

    private fun shoot(name: String, dark: Boolean, ui: LibraryFeedViewModel.Ui = LibraryFeedViewModel.Ui(feed, loading = false),
        offline: Boolean = false, index: Int? = null, menu: Boolean = false, atEnd: Boolean = false) {
        compose.setContent {
            PopcornTheme(darkTheme = dark, reduceMotion = true) {
                WithSampleImages {
                    AppShell(initialTab = Tab.Home) { page ->
                        if (page.tab == Tab.Home) LibraryFeedScreen(SampleMedia.movies, ui, page.bottomClearance,
                            LibraryFeedActions(), isOwner = true, connectionLost = offline, format = SampleFormat)
                    }
                }
            }
        }
        compose.waitForIdle()
        if (index != null) compose.onNodeWithTag("library feed").performScrollToIndex(index)
        if (menu) compose.onNodeWithContentDescription("Library view").performClick()
        compose.waitForIdle()
        // Record after the backdrop's placement and selection clip have drawn a fresh frame.
        compose.mainClock.advanceTimeByFrame()
        compose.runOnUiThread { compose.activity.window.decorView.invalidate() }
        compose.waitForIdle()
        val home = compose.onNodeWithTag("tab Home").captureToImage().toPixelMap()
        val whites = (home.width / 4 until home.width * 3 / 4).sumOf { x ->
            (home.height / 4 until home.height * 3 / 4).count { y ->
                val pixel = home[x, y]
                pixel.red > 0.9f && pixel.green > 0.9f && pixel.blue > 0.9f
            }
        }
        check(whites > 10) { "The selected Home glyph must be drawn before capture." }
        if (atEnd) compose.assertNothingUnderTheToolbar()
        compose.onRoot().captureRoboImage(screenshot(name))
    }

    @Test fun feedLight() = shoot("library-feed-light", false)
    @Test fun feedDark() = shoot("library-feed-dark", true)
    @Test fun collectionsLight() = shoot("library-feed-collections-light", false, index = 3)
    @Test fun collectionsDark() = shoot("library-feed-collections-dark", true, index = 3)
    @Test fun menuLight() = shoot("library-feed-menu-light", false, menu = true)
    @Test fun loadingDark() = shoot("library-feed-loading-dark", true, LibraryFeedViewModel.Ui())
    @Test fun failedLight() = shoot("library-feed-failed-light", false, LibraryFeedViewModel.Ui(loading = false, error = "PopcornStash did not answer. Check the connection and try again."))
    @Test fun emptyDark() = shoot("library-feed-empty-dark", true, LibraryFeedViewModel.Ui(VideoLibraryFeed(), loading = false))
    @Test fun offlineDark() = shoot("library-feed-offline-dark", true, offline = true)
    @Test fun emptyCollectionLight() = shoot("library-feed-empty-collection-light", false,
        LibraryFeedViewModel.Ui(VideoLibraryFeed(collections = listOf(VideoCollection(VideoFeedFixtures.collections.first().folder.copy(itemCount = 0)))) , loading = false))
    @Test fun endLight() = shoot("library-feed-end-light", false, index = 14, atEnd = true)

    @Test
    @Config(qualifiers = "w800dp-h1100dp-xhdpi")
    fun tabletLight() = shoot("library-feed-tablet-light", false)

    @Test
    @Config(qualifiers = "w360dp-h800dp-xxhdpi", fontScale = 2.0f)
    fun largeTextCollectionsDark() = shoot("library-feed-large-text-dark", true, index = 3)
}
