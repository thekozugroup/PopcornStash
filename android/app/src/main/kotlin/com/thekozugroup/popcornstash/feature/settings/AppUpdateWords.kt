package com.thekozugroup.popcornstash.feature.settings

import com.thekozugroup.popcornstash.feature.downloads.formatBytes
import com.thekozugroup.popcornstash.feature.library.MediaFormat
import com.thekozugroup.popcornstash.update.AppUpdateUi
import com.thekozugroup.popcornstash.update.CheckProblem
import com.thekozugroup.popcornstash.update.FetchProblem
import com.thekozugroup.popcornstash.update.UpdateProblem
import com.thekozugroup.popcornstash.update.UpdateStatus
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.Locale

/** What the update part of "This app" offers to do. */
enum class UpdateAction(val label: String, val primary: Boolean) {
    Check("Check for updates", primary = false),
    CheckAgain("Try again", primary = false),
    Update("Update", primary = true),
    UpdateAgain("Try again", primary = false),
    AllowInstalls("Allow installs", primary = true),
    Install("Install", primary = true),
    OpenReleasePage("Open the release page", primary = true),
}

/** Download progress: [percent] from 0 to 100, or null while the size is not known (an empty track). */
data class UpdateProgress(val percent: Int?)

/** The words and the one action for an update state (android/DESIGN.md, Settings, This app). */
data class UpdateWords(
    val line: String,
    val detail: String? = null,
    val action: UpdateAction? = null,
    val busy: Boolean = false,
    val progress: UpdateProgress? = null,
)

/** "0.1.0, build 412". */
fun versionLine(ui: AppUpdateUi): String = "${ui.version}, build ${ui.build}"

fun updateWords(
    status: UpdateStatus,
    now: Long,
    zone: ZoneId = ZoneId.systemDefault(),
    locale: Locale = Locale.getDefault(),
): UpdateWords = when (status) {
    UpdateStatus.Idle -> UpdateWords("Not checked yet.", action = UpdateAction.Check)
    UpdateStatus.Checking -> UpdateWords("Checking for updates", action = UpdateAction.Check, busy = true)
    is UpdateStatus.UpToDate -> UpdateWords("Up to date, checked ${checkedAgo(status.checkedAt, now, zone, locale)}.", action = UpdateAction.Check)
    is UpdateStatus.Available -> UpdateWords(
        "Build ${status.build.number} is available.",
        detail = if (status.build.size > 0) {
            "PopcornStash downloads it from GitHub (${formatBytes(status.build.size, locale)}) and Android installs it. You stay signed in."
        } else {
            "PopcornStash downloads it from GitHub and Android installs it. You stay signed in."
        },
        action = UpdateAction.Update,
    )
    is UpdateStatus.NeedsPermission -> UpdateWords(
        "Build ${status.build.number} is available.",
        detail = "Android asks first: allow PopcornStash to install apps, then come back here.",
        action = UpdateAction.AllowInstalls,
    )
    is UpdateStatus.Downloading -> UpdateWords(
        if (status.percent != null) "Downloading build ${status.build.number}, ${status.percent}%" else "Downloading build ${status.build.number}",
        progress = UpdateProgress(status.percent),
    )
    is UpdateStatus.Verifying -> UpdateWords(
        "Checking the download",
        detail = "Its checksum and its signature must match before it installs.",
        progress = UpdateProgress(100),
    )
    is UpdateStatus.ReadyToInstall -> UpdateWords("Build ${status.build.number} is ready to install.", action = UpdateAction.Install)
    is UpdateStatus.Installing -> UpdateWords(
        "Installing build ${status.build.number}",
        detail = "Confirm in the window Android shows. PopcornStash restarts once it is installed.",
    )
    is UpdateStatus.OtherKey -> UpdateWords(
        "Build ${status.build.number} is available, but this copy cannot update itself.",
        detail = "It is signed with a different key from the builds on GitHub. Install one by hand once: " +
            "uninstall this app, install build ${status.build.number} from the release page, then pair again.",
        action = UpdateAction.OpenReleasePage,
    )
    is UpdateStatus.Failed -> failedWords(status, zone, locale)
}

private fun failedWords(status: UpdateStatus.Failed, zone: ZoneId, locale: Locale): UpdateWords {
    val number = status.build?.number
    val build = number?.let { "build $it" } ?: "the update"
    return when (val problem = status.problem) {
        is UpdateProblem.CheckFailed -> UpdateWords(
            when (val check = problem.problem) {
                CheckProblem.Offline -> "Could not reach GitHub. Check the connection and try again."
                is CheckProblem.RateLimited -> check.resetsAt?.let {
                    "GitHub is taking no more checks from this network for now. Try again after ${clockTime(it, zone, locale)}."
                } ?: "GitHub is taking no more checks from this network for now. Try again in an hour."
                is CheckProblem.HttpError -> "GitHub answered with an error (HTTP ${check.code}). Try again later."
                CheckProblem.Unreadable -> "GitHub's answer could not be read. Try again later."
            },
            action = UpdateAction.CheckAgain,
        )
        is UpdateProblem.FetchFailed -> UpdateWords(
            when (problem.problem) {
                FetchProblem.DownloadStopped -> "The download of $build stopped. Try again."
                FetchProblem.ChecksumMismatch -> "The download did not match its checksum, so it was deleted. Try again."
                FetchProblem.NotThisApp -> "The download was not a PopcornStash build, so it was deleted."
                FetchProblem.OtherSigner -> "The download is signed with a different key, so it was deleted."
            },
            action = UpdateAction.UpdateAgain.takeIf { status.build != null },
        )
        UpdateProblem.InstallCancelled -> UpdateWords(
            "${build.replaceFirstChar { it.uppercase(locale) }} was not installed.",
            detail = "Android's confirmation was closed. Update again whenever you like.",
            action = UpdateAction.Update.takeIf { status.build != null },
        )
        is UpdateProblem.InstallFailed -> UpdateWords(
            problem.message?.takeIf { it.isNotBlank() }?.let { "Android did not install $build: ${it.trim().trimEnd('.')}." }
                ?: "Android did not install $build.",
            action = UpdateAction.UpdateAgain.takeIf { status.build != null },
        )
    }
}

/** "just now", "2 minutes ago", "3 hours ago", "yesterday", "on 12 Sep 2026". */
fun checkedAgo(at: Long, now: Long, zone: ZoneId = ZoneId.systemDefault(), locale: Locale = Locale.getDefault()): String {
    val minutes = (now - at).coerceAtLeast(0) / 60_000
    val hours = minutes / 60
    return when {
        minutes < 1 -> "just now"
        minutes == 1L -> "1 minute ago"
        hours < 1 -> "$minutes minutes ago"
        hours == 1L -> "1 hour ago"
        hours < 24 -> "$hours hours ago"
        hours < 48 -> "yesterday"
        else -> "on " + MediaFormat.unbroken(
            DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM).withLocale(locale).format(Instant.ofEpochMilli(at).atZone(zone)),
        )
    }
}

private fun clockTime(at: Long, zone: ZoneId, locale: Locale): String =
    MediaFormat.unbroken(DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).withLocale(locale).format(Instant.ofEpochMilli(at).atZone(zone)))
