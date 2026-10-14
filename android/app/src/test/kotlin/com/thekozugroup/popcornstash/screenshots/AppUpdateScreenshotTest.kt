package com.thekozugroup.popcornstash.screenshots

import androidx.activity.ComponentActivity
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onRoot
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.github.takahirom.roborazzi.captureRoboImage
import com.thekozugroup.popcornstash.data.api.Role
import com.thekozugroup.popcornstash.data.api.User
import com.thekozugroup.popcornstash.feature.settings.AppUpdateSectionState
import com.thekozugroup.popcornstash.feature.settings.SettingsRootPage
import com.thekozugroup.popcornstash.navigation.AppShell
import com.thekozugroup.popcornstash.navigation.Tab
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import com.thekozugroup.popcornstash.update.AppBuild
import com.thekozugroup.popcornstash.update.AppUpdateUi
import com.thekozugroup.popcornstash.update.CheckProblem
import com.thekozugroup.popcornstash.update.UpdateProblem
import com.thekozugroup.popcornstash.update.UpdateStatus
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/**
 * "This app" on the Settings page in every state of an update, light and dark: the page is
 * scrolled so the section sits under the top bar. Recorded with `./gradlew recordRoborazziDebug`.
 */
@RunWith(AndroidJUnit4::class)
@Config(qualifiers = PHONE)
class AppUpdateScreenshotTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val member = User(id = "u-tomas", displayName = "Tomas Ribeiro", role = Role.Member)
    private val now = 1_760_430_000_000L
    private val build = AppBuild(
        number = 415,
        apkUrl = "https://github.com/thekozugroup/PopcornStash/releases/download/android-415/popcornstash-android-415.apk",
        sha256Url = "https://github.com/thekozugroup/PopcornStash/releases/download/android-415/popcornstash-android-415.apk.sha256",
        pageUrl = "https://github.com/thekozugroup/PopcornStash/releases/tag/android-415",
        size = 14_268_412,
    )

    private fun shoot(name: String, dark: Boolean, status: UpdateStatus, autoCheck: Boolean = true) {
        val ui = AppUpdateUi(version = "0.1.0", build = 412, status = status, autoCheck = autoCheck)
        compose.setContent {
            PopcornTheme(darkTheme = dark, reduceMotion = true) {
                AppShell(initialTab = Tab.Settings) { page ->
                    if (page.tab == Tab.Settings) {
                        val list = rememberLazyListState()
                        // The section's first item: the heading "This app" under the top bar.
                        LaunchedEffect(Unit) { list.scrollToItem(SECTION_INDEX) }
                        SettingsRootPage(
                            page,
                            member,
                            "stash.example.com",
                            "0.2.0",
                            libraries = null,
                            onOpenWeb = {},
                            onSignOut = {},
                            listState = list,
                            appUpdates = AppUpdateSectionState(ui, now),
                        )
                    } else {
                        SampleTabRoot(page)
                    }
                }
            }
        }
        compose.waitForIdle()
        compose.onRoot().captureRoboImage(screenshot("app-update-$name-${if (dark) "dark" else "light"}"))
    }

    @Test fun checkingLight() = shoot("checking", false, UpdateStatus.Checking)

    @Test fun checkingDark() = shoot("checking", true, UpdateStatus.Checking)

    @Test fun upToDateLight() = shoot("up-to-date", false, UpdateStatus.UpToDate(now - 2 * 60_000))

    @Test fun upToDateDark() = shoot("up-to-date", true, UpdateStatus.UpToDate(now - 2 * 60_000))

    @Test fun availableLight() = shoot("available", false, UpdateStatus.Available(build))

    @Test fun availableDark() = shoot("available", true, UpdateStatus.Available(build))

    @Test fun needsPermissionLight() = shoot("needs-permission", false, UpdateStatus.NeedsPermission(build))

    @Test fun needsPermissionDark() = shoot("needs-permission", true, UpdateStatus.NeedsPermission(build))

    @Test fun downloadingLight() = shoot("downloading", false, UpdateStatus.Downloading(build, 41))

    @Test fun downloadingDark() = shoot("downloading", true, UpdateStatus.Downloading(build, 41))

    @Test fun verifyingLight() = shoot("verifying", false, UpdateStatus.Verifying(build))

    @Test fun verifyingDark() = shoot("verifying", true, UpdateStatus.Verifying(build))

    @Test fun readyLight() = shoot("ready", false, UpdateStatus.ReadyToInstall(build))

    @Test fun readyDark() = shoot("ready", true, UpdateStatus.ReadyToInstall(build))

    @Test fun installingLight() = shoot("installing", false, UpdateStatus.Installing(build))

    @Test fun installingDark() = shoot("installing", true, UpdateStatus.Installing(build))

    @Test fun otherKeyLight() = shoot("other-key", false, UpdateStatus.OtherKey(build))

    @Test fun otherKeyDark() = shoot("other-key", true, UpdateStatus.OtherKey(build))

    @Test fun offlineLight() = shoot("offline", false, UpdateStatus.Failed(UpdateProblem.CheckFailed(CheckProblem.Offline), null))

    @Test fun offlineDark() = shoot("offline", true, UpdateStatus.Failed(UpdateProblem.CheckFailed(CheckProblem.Offline), null))

    @Test fun cancelledLight() = shoot("cancelled", false, UpdateStatus.Failed(UpdateProblem.InstallCancelled, build))

    @Test fun cancelledDark() = shoot("cancelled", true, UpdateStatus.Failed(UpdateProblem.InstallCancelled, build))

    @Test fun automaticOffLight() = shoot("automatic-off", false, UpdateStatus.Idle, autoCheck = false)

    @Test fun automaticOffDark() = shoot("automatic-off", true, UpdateStatus.Idle, autoCheck = false)

    private companion object {
        /** You (0), This server (1), "On the web" and its two rows (2 to 4), then "This app" (5). */
        const val SECTION_INDEX = 5
    }
}
