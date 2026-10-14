package com.thekozugroup.popcornstash.screenshots

import androidx.activity.ComponentActivity
import androidx.compose.runtime.Composable
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performScrollToIndex
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.github.takahirom.roborazzi.captureRoboImage
import com.thekozugroup.popcornstash.data.api.Home
import com.thekozugroup.popcornstash.data.api.HomeLibrary
import com.thekozugroup.popcornstash.data.api.HomeShow
import com.thekozugroup.popcornstash.data.repo.Loadable
import com.thekozugroup.popcornstash.feature.home.HomeActions
import com.thekozugroup.popcornstash.feature.home.HomeScreen
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
 * Home as a phone shows it, light and dark: content, loading, a failed load, no libraries
 * (owner and member) and a lost connection. Recorded with `./gradlew recordRoborazziDebug`.
 */
@RunWith(AndroidJUnit4::class)
@Config(qualifiers = PHONE)
class HomeScreenshotTest {
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

    private fun home(name: String, dark: Boolean, home: Loadable<Home>, owner: Boolean = true, connectionLost: Boolean = false) =
        shoot(name, dark) { page ->
            HomeScreen(page, home, isOwner = owner, actions = HomeActions(), connectionLost = connectionLost, format = SampleFormat)
        }

    @Test
    fun contentLight() = home("home-light", dark = false, Loadable.Ready(SampleMedia.home))

    @Test
    fun contentDark() = home("home-dark", dark = true, Loadable.Ready(SampleMedia.home))

    /**
     * A series library of three shows whose twelve newest episodes all belong to two of them:
     * the shelf still has a tile for each of the three shows its heading counts.
     */
    @Test
    fun aSeriesShelfHasATileForEveryShowLight() {
        val day = 86_400L
        val lantern = SampleMedia.lanternSeason2 + SampleMedia.lantern
        val tidepool = (1..5).map {
            SampleMedia.episode("e-t2$it", "Tidepool Detectives", 2, it, null, minutes = 11, added = SampleMedia.NOW - 2 * day - it)
        }
        val olderGarden = SampleMedia.episode("e-g324", "Garden Hour", 3, 24, "First frost", minutes = 28, added = SampleMedia.NOW - 60 * day)
        val library = SampleMedia.shows.copy(itemCount = 43, showCount = 3)
        val shelf = HomeLibrary(
            library,
            recent = (lantern + tidepool).sortedByDescending { it.addedAt }.take(12),
            showPosters = mapOf("The Lantern Keepers" to "e-l101", "Tidepool Detectives" to "e-t104"),
            shows = listOf(
                HomeShow("The Lantern Keepers", "e-l101", itemCount = 7, newCount = 2, latest = lantern[1], lastAddedAt = lantern[1].addedAt),
                HomeShow("Tidepool Detectives", "e-t104", itemCount = 12, newCount = 5, latest = tidepool[0], lastAddedAt = tidepool[0].addedAt),
                HomeShow("Garden Hour", "e-g301", itemCount = 24, newCount = 0, latest = olderGarden, lastAddedAt = olderGarden.addedAt),
            ),
        )
        home("home-series-shelf-light", dark = false, Loadable.Ready(Home(libraries = listOf(shelf))))
    }

    /**
     * Further down Home: the series shelf, camera videos, photos, an empty library and Other
     * files. [atTheEnd] also checks that nothing on the page lies under the toolbar there.
     */
    private fun scrolled(name: String, dark: Boolean, index: Int, atTheEnd: Boolean = false) =
        shoot(
            name,
            dark,
            before = {
                compose.onNodeWithTag("list Home").performScrollToIndex(index)
                if (atTheEnd) {
                    compose.waitForIdle()
                    compose.assertNothingUnderTheToolbar()
                }
            },
        ) { page ->
            HomeScreen(page, Loadable.Ready(SampleMedia.home), isOwner = true, actions = HomeActions(), format = SampleFormat)
        }

    @Test
    fun middleDark() = scrolled("home-middle-dark", dark = true, index = 3)

    /** Home scrolled to its end: Other files and its count sit above the bar. */
    @Test
    fun endLight() = scrolled("home-end-light", dark = false, index = 7, atTheEnd = true)

    @Test
    fun loadingLight() = home("home-loading-light", dark = false, Loadable.Loading)

    @Test
    fun loadingDark() = home("home-loading-dark", dark = true, Loadable.Loading)

    @Test
    fun failedDark() = home(
        "home-failed-dark",
        dark = true,
        Loadable.Failed("PopcornStash did not answer. Check the connection and try again."),
    )

    @Test
    fun noLibrariesOwnerLight() = home("home-empty-owner-light", dark = false, Loadable.Ready(Home()))

    @Test
    fun noLibrariesMemberDark() = home("home-empty-member-dark", dark = true, Loadable.Ready(Home()), owner = false)

    @Test
    fun connectionLostLight() = home(
        "home-offline-light",
        dark = false,
        Loadable.Ready(SampleMedia.home.copy(continueWatching = emptyList())),
        connectionLost = true,
    )
}
