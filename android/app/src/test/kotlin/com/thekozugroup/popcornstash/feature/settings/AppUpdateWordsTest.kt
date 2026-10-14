package com.thekozugroup.popcornstash.feature.settings

import com.thekozugroup.popcornstash.update.AppBuild
import com.thekozugroup.popcornstash.update.AppUpdateUi
import com.thekozugroup.popcornstash.update.CheckProblem
import com.thekozugroup.popcornstash.update.FetchProblem
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
        assertEquals("0.1.0, build 412", versionLine(AppUpdateUi("0.1.0", 412, UpdateStatus.Idle, true)))
    }

    @Test
    fun checkingAndUpToDate() {
        assertEquals(UpdateWords("Not checked yet.", action = UpdateAction.Check), words(UpdateStatus.Idle))
        assertEquals(UpdateWords("Checking for updates", action = UpdateAction.Check, busy = true), words(UpdateStatus.Checking))
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
        assertEquals(UpdateProgress(100), verifying.progress)
        assertEquals(UpdateWords("Build 415 is ready to install.", action = UpdateAction.Install), words(UpdateStatus.ReadyToInstall(build)))
        assertEquals("Installing build 415", words(UpdateStatus.Installing(build)).line)
        assertNull(words(UpdateStatus.Installing(build)).action)
    }

    @Test
    fun permissionAndAnotherKey() {
        assertEquals(UpdateAction.AllowInstalls, words(UpdateStatus.NeedsPermission(build)).action)
        val other = words(UpdateStatus.OtherKey(build))
        assertEquals("Build 415 is available, but this copy cannot update itself.", other.line)
        assertTrue(other.detail!!.contains("uninstall this app, install build 415 from the release page, then pair again"))
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
        assertEquals("GitHub answered with an error (HTTP 502). Try again later.", failed(UpdateProblem.CheckFailed(CheckProblem.HttpError(502)), null).line)
        assertEquals("GitHub's answer could not be read. Try again later.", failed(UpdateProblem.CheckFailed(CheckProblem.Unreadable), null).line)
        assertEquals("The download of build 415 stopped. Try again.", failed(UpdateProblem.FetchFailed(FetchProblem.DownloadStopped)).line)
        assertEquals(UpdateAction.UpdateAgain, failed(UpdateProblem.FetchFailed(FetchProblem.DownloadStopped)).action)
        assertEquals(
            "The download did not match its checksum, so it was deleted. Try again.",
            failed(UpdateProblem.FetchFailed(FetchProblem.ChecksumMismatch)).line,
        )
        assertEquals("The download was not a PopcornStash build, so it was deleted.", failed(UpdateProblem.FetchFailed(FetchProblem.NotThisApp)).line)
        assertEquals("Build 415 was not installed.", failed(UpdateProblem.InstallCancelled).line)
        assertEquals(UpdateAction.Update, failed(UpdateProblem.InstallCancelled).action)
        assertEquals(
            "Android did not install build 415: INSTALL_FAILED_INSUFFICIENT_STORAGE.",
            failed(UpdateProblem.InstallFailed("INSTALL_FAILED_INSUFFICIENT_STORAGE")).line,
        )
        assertEquals("Android did not install build 415.", failed(UpdateProblem.InstallFailed(" ")).line)
        assertEquals("Android did not install the update.", failed(UpdateProblem.InstallFailed(null), null).line)
        assertNull(failed(UpdateProblem.InstallFailed(null), null).action)
    }

    @Test
    fun noWordHasAnEmDash() {
        val all = listOf(
            UpdateStatus.Idle, UpdateStatus.Checking, UpdateStatus.UpToDate(now), UpdateStatus.Available(build),
            UpdateStatus.NeedsPermission(build), UpdateStatus.Downloading(build, 3), UpdateStatus.Verifying(build),
            UpdateStatus.ReadyToInstall(build), UpdateStatus.Installing(build), UpdateStatus.OtherKey(build),
        ).map(::words)
        all.forEach { assertFalse(it.toString(), "${it.line} ${it.detail}".contains('—')) }
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
