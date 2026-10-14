package com.thekozugroup.popcornstash.feature.settings

import com.thekozugroup.popcornstash.update.AppBuild
import com.thekozugroup.popcornstash.update.AppUpdateUi
import com.thekozugroup.popcornstash.update.CheckProblem
import com.thekozugroup.popcornstash.update.FetchProblem
import com.thekozugroup.popcornstash.update.InstallFailure
import com.thekozugroup.popcornstash.update.UpdateProblem
import com.thekozugroup.popcornstash.update.UpdateStatus
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.ZoneId
import java.util.Locale

/** What "This app" says in each state of an update, and its one action. */
class AppUpdateWordsTest {
    private val now = 1_760_430_000_000L
    private val zone = ZoneId.of("Europe/Lisbon")
    private val build = AppBuild(415, "https://x.test/a.apk", "https://x.test/a.sha256", "https://github.com/x", 14_268_412)

    private fun words(status: UpdateStatus) = updateWords(status, now, zone, Locale.UK)

    @Test
    fun theVersionLine() {
        // Read like the server's row above it ("Version 0.2.0").
        assertEquals("Version 0.1.0, build 412", versionLine(AppUpdateUi("0.1.0", 412, UpdateStatus.Idle, true)))
    }

    @Test
    fun theSwitchLineSaysWhenNotificationsAreOff() {
        assertEquals("Asks GitHub once a day for a newer build.", autoCheckLine(autoCheck = true, notificationsOff = false))
        assertEquals(
            "Asks GitHub once a day. Notifications are off, so a new build shows only here.",
            autoCheckLine(autoCheck = true, notificationsOff = true),
        )
        // Off, the line says what turning it on does.
        assertEquals("Asks GitHub once a day for a newer build.", autoCheckLine(autoCheck = false, notificationsOff = true))
    }

    @Test
    fun checkingAndUpToDate() {
        assertEquals(UpdateWords("Not checked yet.", action = UpdateAction.Check), words(UpdateStatus.Idle))
        // The busy button keeps its words; the sentence says something else, never the same words twice.
        assertEquals(UpdateWords("Asking GitHub for the newest build.", action = UpdateAction.Check, busy = true), words(UpdateStatus.Checking))
        assertEquals("Up to date, checked 2 minutes ago.", words(UpdateStatus.UpToDate(now - 2 * 60_000)).line)
        assertEquals(UpdateAction.Check, words(UpdateStatus.UpToDate(now)).action)
    }

    @Test
    fun anAvailableBuildHasUpdateAsTheOnePrimaryAction() {
        val w = words(UpdateStatus.Available(build))
        assertEquals("Build 415 is available.", w.line)
        assertTrue(w.detail!!.contains("from GitHub (14.3 MB)"))
        assertEquals(UpdateAction.Update, w.action)
        assertTrue(w.action!!.primary)
        assertFalse(words(UpdateStatus.Available(build.copy(size = 0))).detail!!.contains("MB"))
    }

    @Test
    fun theDownloadShowsItsPercentThenTheChecks() {
        val downloading = words(UpdateStatus.Downloading(build, 41))
        assertEquals("Downloading build 415, 41%", downloading.line)
        assertEquals(UpdateProgress(41), downloading.progress)
        assertNull(downloading.action)
        assertEquals("Downloading build 415", words(UpdateStatus.Downloading(build, null)).line)
        val verifying = words(UpdateStatus.Verifying(build))
        assertEquals("Checking the download", verifying.line)
        assertEquals("Making sure it is the real build from GitHub.", verifying.detail)
        // A moving bar, not a full one that reads as done.
        assertEquals(UpdateProgress(percent = null, working = true), verifying.progress)
        assertEquals(UpdateWords("Build 415 is ready to install.", action = UpdateAction.Install), words(UpdateStatus.ReadyToInstall(build)))
    }

    @Test
    fun installingSaysTheAppClosesAndCanShowAndroidsWindowAgain() {
        val installing = words(UpdateStatus.Installing(build))
        assertEquals("Installing build 415", installing.line)
        assertEquals(
            "Android may ask you to confirm. PopcornStash closes while it updates: open it again afterwards. You stay signed in.",
            installing.detail,
        )
        assertNull(installing.action)
        val waiting = words(UpdateStatus.Installing(build, canConfirm = true))
        assertEquals(UpdateAction.Confirm, waiting.action)
        assertEquals("Install", waiting.action!!.label)
        assertTrue(waiting.detail!!.startsWith("Confirm in Android's window, or tap Install to show it again."))
    }

    @Test
    fun aClosedConfirmationKeepsTheFileForInstall() {
        assertEquals(
            UpdateWords(
                "Build 415 was not installed.",
                detail = "Android's confirmation was closed. Install it whenever you like.",
                action = UpdateAction.Install,
            ),
            words(UpdateStatus.ReadyToInstall(build, cancelled = true)),
        )
    }

    @Test
    fun permissionAndAnotherKey() {
        assertEquals(UpdateAction.AllowInstalls, words(UpdateStatus.NeedsPermission(build)).action)
        val other = words(UpdateStatus.OtherKey(build))
        assertEquals("Build 415 needs a one-time reinstall.", other.line)
        // In the order the person does it: the download first, while the app (and its button) is still there.
        assertEquals(
            "This copy was not installed from GitHub, so it cannot update itself. Open the release page and download build 415, " +
                "then uninstall this app, open the download and sign in again.",
            other.detail,
        )
        assertEquals(UpdateAction.OpenReleasePage, other.action)
    }

    @Test
    fun everyFailureIsAPlainSentence() {
        fun failed(problem: UpdateProblem, b: AppBuild? = build) = words(UpdateStatus.Failed(problem, b))
        assertEquals("Could not reach GitHub. Check the connection and try again.", failed(UpdateProblem.CheckFailed(CheckProblem.Offline), null).line)
        assertEquals(UpdateAction.CheckAgain, failed(UpdateProblem.CheckFailed(CheckProblem.Offline), null).action)
        // 13:05 UTC is 14:05 in Lisbon in October; the time never breaks inside.
        assertEquals(
            "GitHub is taking no more checks from this network for now. Try again after 14:05.",
            failed(UpdateProblem.CheckFailed(CheckProblem.RateLimited(1_760_447_100_000L)), null).line,
        )
        assertEquals(
            "GitHub is taking no more checks from this network for now. Try again in an hour.",
            failed(UpdateProblem.CheckFailed(CheckProblem.RateLimited(null)), null).line,
        )
        assertEquals("GitHub is having trouble right now. Try again later.", failed(UpdateProblem.CheckFailed(CheckProblem.HttpError(502)), null).line)
        assertEquals("GitHub's answer could not be read. Try again later.", failed(UpdateProblem.CheckFailed(CheckProblem.Unreadable), null).line)
        assertEquals("The download of build 415 stopped. Try again.", failed(UpdateProblem.FetchFailed(FetchProblem.DownloadStopped)).line)
        assertEquals(UpdateAction.UpdateAgain, failed(UpdateProblem.FetchFailed(FetchProblem.DownloadStopped)).action)
        assertEquals("The download was damaged, so it was deleted. Try again.", failed(UpdateProblem.FetchFailed(FetchProblem.ChecksumMismatch)).line)
        assertEquals(UpdateAction.UpdateAgain, failed(UpdateProblem.FetchFailed(FetchProblem.ChecksumMismatch)).action)
        // Trying again would fetch the same file: no "Try again", and the owner is the one to tell.
        val notThisApp = failed(UpdateProblem.FetchFailed(FetchProblem.NotThisApp))
        assertEquals("The download was not the right build.", notThisApp.line)
        assertEquals("It was deleted. Tell the owner.", notThisApp.detail)
        assertEquals(UpdateAction.Check, notThisApp.action)
        assertEquals("Build 415 was not installed.", failed(UpdateProblem.InstallCancelled).line)
        assertEquals(UpdateAction.Update, failed(UpdateProblem.InstallCancelled).action)
    }

    @Test
    fun aDownloadFromAnotherKeyIsNeverSentToTheReleasePage() {
        val w = words(UpdateStatus.Failed(UpdateProblem.FetchFailed(FetchProblem.OtherSigner), build))
        assertEquals("Build 415 was not signed by PopcornStash.", w.line)
        assertEquals("It was deleted. Do not install it by hand; tell the owner.", w.detail)
        assertEquals(UpdateAction.Check, w.action)
    }

    @Test
    fun androidsRefusalsArePlainSentencesWithoutItsCodes() {
        fun failed(reason: InstallFailure, b: AppBuild? = build) = words(UpdateStatus.Failed(UpdateProblem.InstallFailed(reason), b))
        assertEquals(
            UpdateWords("There is not enough space on this phone for build 415.", detail = "Free some space and try again.", action = UpdateAction.UpdateAgain),
            failed(InstallFailure.Storage),
        )
        assertEquals(UpdateWords("This phone's Android cannot run build 415.", detail = "Tell the owner.", action = UpdateAction.Check), failed(InstallFailure.Incompatible))
        assertEquals("Android refused build 415: it does not match the app on this phone.", failed(InstallFailure.Conflict).line)
        assertEquals(UpdateAction.Check, failed(InstallFailure.Conflict).action)
        assertEquals("Android blocked the install of build 415.", failed(InstallFailure.Blocked).line)
        assertEquals(UpdateAction.UpdateAgain, failed(InstallFailure.Blocked).action)
        assertEquals("Android found the download of build 415 damaged. Try again.", failed(InstallFailure.Invalid).line)
        assertEquals(UpdateWords("Android did not install build 415. Try again.", action = UpdateAction.UpdateAgain), failed(InstallFailure.Other))
        assertEquals("Android did not install the update. Try again.", failed(InstallFailure.Other, null).line)
        assertNull(failed(InstallFailure.Other, null).action)
        InstallFailure.entries.forEach { reason ->
            assertFalse("no INSTALL_FAILED code in $reason", "${failed(reason)}".contains("INSTALL_"))
        }
    }

    @Test
    fun noWordHasAnEmDash() {
        val all = listOf(
            UpdateStatus.Idle, UpdateStatus.Checking, UpdateStatus.UpToDate(now), UpdateStatus.Available(build),
            UpdateStatus.NeedsPermission(build), UpdateStatus.Downloading(build, 3), UpdateStatus.Verifying(build),
            UpdateStatus.ReadyToInstall(build), UpdateStatus.ReadyToInstall(build, cancelled = true), UpdateStatus.Installing(build),
            UpdateStatus.Installing(build, canConfirm = true), UpdateStatus.OtherKey(build),
            UpdateStatus.Failed(UpdateProblem.FetchFailed(FetchProblem.OtherSigner), build),
        ).map(::words) + InstallFailure.entries.map { words(UpdateStatus.Failed(UpdateProblem.InstallFailed(it), build)) }
        all.forEach { assertFalse(it.toString(), "${it.line} ${it.detail}".contains('—') || "${it.line} ${it.detail}".contains('–')) }
    }

    @Test
    fun checkedAgo() {
        val minute = 60_000L
        assertEquals("just now", checkedAgo(now - 20_000, now, zone, Locale.UK))
        assertEquals("1 minute ago", checkedAgo(now - minute, now, zone, Locale.UK))
        assertEquals("2 minutes ago", checkedAgo(now - 2 * minute, now, zone, Locale.UK))
        assertEquals("1 hour ago", checkedAgo(now - 61 * minute, now, zone, Locale.UK))
        assertEquals("5 hours ago", checkedAgo(now - 5 * 60 * minute, now, zone, Locale.UK))
        assertEquals("yesterday", checkedAgo(now - 30 * 60 * minute, now, zone, Locale.UK))
        assertEquals("on 9 Oct 2025", checkedAgo(now - 5 * 24 * 60 * minute, now, zone, Locale.UK))
        // A clock that went back reads as just now, never "-3 minutes ago".
        assertEquals("just now", checkedAgo(now + 3 * minute, now, zone, Locale.UK))
    }
}
