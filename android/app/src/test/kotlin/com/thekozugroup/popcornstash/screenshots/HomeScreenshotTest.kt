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

    /** Further down Home: the series shelf, camera videos, photos, an empty library and Other files. */
    private fun scrolled(name: String, dark: Boolean, index: Int) =
        shoot(name, dark, before = { compose.onNodeWithTag("list Home").performScrollToIndex(index) }) { page ->
            HomeScreen(page, Loadable.Ready(SampleMedia.home), isOwner = true, actions = HomeActions(), format = SampleFormat)
        }

    @Test
    fun middleDark() = scrolled("home-middle-dark", dark = true, index = 3)

    @Test
    fun endLight() = scrolled("home-end-light", dark = false, index = 7)

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
