package com.thekozugroup.popcornstash.feature.settings

import androidx.activity.ComponentActivity
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.state.ToggleableState
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.performScrollToNode
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.thekozugroup.popcornstash.data.api.Role as PersonRole
import com.thekozugroup.popcornstash.data.api.User
import com.thekozugroup.popcornstash.navigation.AppShell
import com.thekozugroup.popcornstash.navigation.Tab
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import com.thekozugroup.popcornstash.update.AppBuild
import com.thekozugroup.popcornstash.update.AppUpdateUi
import com.thekozugroup.popcornstash.update.UpdateStatus
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/** "This app" on the Settings page: what its button and its switch do, and what TalkBack hears. */
@RunWith(AndroidJUnit4::class)
class AppUpdateSectionTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val build = AppBuild(415, "https://x.test/a.apk", "https://x.test/a.sha256", "https://github.com/x", 0)
    private val actions = mutableListOf<UpdateAction>()
    private val switches = mutableListOf<Boolean>()
    private var ui by mutableStateOf(AppUpdateUi("0.1.0", 412, UpdateStatus.Available(build), autoCheck = true))
    private var notificationsOff by mutableStateOf(false)

    private fun show() {
        compose.setContent {
            PopcornTheme(darkTheme = false, reduceMotion = true) {
                AppShell(initialTab = Tab.Settings) { page ->
                    if (page.tab == Tab.Settings) {
                        SettingsRootPage(
                            page,
                            User(id = "u-tomas", displayName = "Tomas Ribeiro", role = PersonRole.Member),
                            "stash.example.com",
                            "0.2.0",
                            libraries = null,
                            onOpenWeb = {},
                            onSignOut = {},
                            appUpdates = AppUpdateSectionState(ui, 0L, notificationsOff),
                            updateActions = AppUpdateActions(
                                onAction = { actions += it },
                                onAutoCheck = { on ->
                                    switches += on
                                    ui = ui.copy(autoCheck = on)
                                },
                            ),
                        )
                    }
                }
            }
        }
    }

    /** Brings an item of the Settings list into view (a lazy list composes only what is near the screen). */
    private fun reach(matcher: SemanticsMatcher) {
        compose.onNodeWithTag("list Settings").performScrollToNode(matcher)
        compose.waitForIdle()
    }

    @Test
    fun theVersionAndTheOneActionAreThere() {
        show()
        reach(hasText("This app"))
        compose.onNodeWithText("This app").assertIsDisplayed()
        compose.onNodeWithText("Version 0.1.0, build 412").assertIsDisplayed()
        compose.onNodeWithText("Build 415 is available.").assertIsDisplayed()
        reach(hasTestTag("update action"))
        compose.onNodeWithTag("update action").performClick()
        assertEquals(listOf(UpdateAction.Update), actions)
    }

    @Test
    fun theButtonFollowsTheState() {
        show()
        ui = ui.copy(status = UpdateStatus.ReadyToInstall(build))
        reach(hasText("Install"))
        compose.onNodeWithText("Install").performClick()
        ui = ui.copy(status = UpdateStatus.OtherKey(build))
        reach(hasText("Open the release page"))
        compose.onNodeWithText("Open the release page").performClick()
        ui = ui.copy(status = UpdateStatus.UpToDate(0L))
        reach(hasText("Check for updates"))
        compose.onNodeWithText("Check for updates").performClick()
        // Android's confirmation was left: "Install" opens it again.
        ui = ui.copy(status = UpdateStatus.Installing(build, canConfirm = true))
        reach(hasTestTag("update action"))
        compose.onNodeWithTag("update action").performClick()
        assertEquals(listOf(UpdateAction.Install, UpdateAction.OpenReleasePage, UpdateAction.Check, UpdateAction.Confirm), actions)
    }

    @Test
    fun checkingTheDownloadShowsAMovingBarAndNoButton() {
        ui = ui.copy(status = UpdateStatus.Verifying(build))
        show()
        reach(hasTestTag("update progress"))
        compose.onNodeWithText("Making sure it is the real build from GitHub.").assertIsDisplayed()
        compose.onNodeWithTag("update progress").assert(SemanticsMatcher.keyIsDefined(SemanticsProperties.ProgressBarRangeInfo))
        compose.onNodeWithTag("update action").assertDoesNotExist()
    }

    @Test
    fun theSwitchSaysWhenNotificationsAreOff() {
        notificationsOff = true
        show()
        reach(hasTestTag("auto check"))
        compose.onNodeWithText("Asks GitHub once a day. Notifications are off, so a new build shows only here.").assertIsDisplayed()
    }

    @Test
    fun whileDownloadingThereIsABarAndNoButton() {
        ui = ui.copy(status = UpdateStatus.Downloading(build, 41))
        show()
        reach(hasTestTag("update progress"))
        compose.onNodeWithText("Downloading build 415, 41%").assertIsDisplayed()
        compose.onNodeWithTag("update progress").assert(SemanticsMatcher.keyIsDefined(SemanticsProperties.ProgressBarRangeInfo))
        compose.onNodeWithTag("update action").assertDoesNotExist()
    }

    @Test
    fun theWholeRowIsOneSwitch() {
        show()
        reach(hasTestTag("auto check"))
        val row = compose.onNodeWithTag("auto check")
        row.assert(SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Switch))
        row.assert(SemanticsMatcher.expectValue(SemanticsProperties.ToggleableState, ToggleableState.On))
        compose.onNodeWithText("Asks GitHub once a day for a newer build.").performClick()
        assertEquals(listOf(false), switches)
        row.assert(SemanticsMatcher.expectValue(SemanticsProperties.ToggleableState, ToggleableState.Off))
    }

    @Test
    fun theStatusIsALiveRegion() {
        show()
        reach(hasTestTag("update status"))
        compose.onNodeWithTag("update status")
            .assert(SemanticsMatcher.keyIsDefined(SemanticsProperties.LiveRegion))
    }
}
