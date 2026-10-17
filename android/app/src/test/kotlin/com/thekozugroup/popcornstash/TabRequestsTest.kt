package com.thekozugroup.popcornstash

import androidx.activity.ComponentActivity
import androidx.compose.material3.Text
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.thekozugroup.popcornstash.feature.home.HomeRoute
import com.thekozugroup.popcornstash.feature.settings.SettingsRoute
import com.thekozugroup.popcornstash.navigation.AppShell
import com.thekozugroup.popcornstash.navigation.Tab
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.util.concurrent.CopyOnWriteArrayList

/**
 * One tab asking for a page of another, as SignedInApp wires it: the shell selects the tab,
 * the tab is handed the page once, and every request is cleared once it is used.
 */
@RunWith(AndroidJUnit4::class)
class TabRequestsTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val requests = TabRequests()
    private val opened = CopyOnWriteArrayList<Any>()

    private fun show() {
        compose.setContent {
            PopcornTheme(darkTheme = false, reduceMotion = true) {
                AppShell(openTab = requests.tab, onOpenedTab = requests::tabShown) { page ->
                    when (page.tab) {
                        Tab.Search -> Text("search page")
                        Tab.Downloads -> Text("downloads page")
                        Tab.Home -> {
                            Text("home page")
                            LaunchedEffect(requests.home) {
                                requests.home?.let {
                                    opened += it
                                    requests.homeOpened()
                                }
                            }
                        }
                        Tab.Settings -> {
                            Text("settings page")
                            LaunchedEffect(requests.settings) {
                                requests.settings?.let {
                                    opened += it
                                    requests.settingsOpened()
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    private fun waitFor(text: String) =
        compose.waitUntil(5_000) { compose.onAllNodes(hasText(text)).fetchSemanticsNodes().isNotEmpty() }

    @Test
    fun aPageAskedForFromAnotherTabSelectsItsTabAndIsHandedOverOnce() {
        show()
        waitFor("home page")

        compose.runOnUiThread { requests.openSettings(SettingsRoute.EditLibrary("lib-kids")) }
        waitFor("settings page")
        compose.runOnIdle {
            assertEquals(listOf<Any>(SettingsRoute.EditLibrary("lib-kids")), opened.toList())
            assertNull(requests.tab)
            assertNull(requests.settings)
        }

        compose.runOnUiThread { requests.openHome(HomeRoute.OtherFiles) }
        waitFor("home page")
        compose.runOnIdle {
            assertEquals(listOf(SettingsRoute.EditLibrary("lib-kids"), HomeRoute.OtherFiles), opened.toList())
            assertNull(requests.tab)
            assertNull(requests.home)
        }
    }

    @Test
    fun aTabAskedForAloneOpensWithNoPage() {
        show()
        waitFor("home page")
        compose.runOnUiThread { requests.showTab(Tab.Downloads) }
        waitFor("downloads page")
        compose.runOnIdle {
            assertNull(requests.tab)
            assertEquals(emptyList<Any>(), opened.toList())
        }
    }
}
