package com.thekozugroup.popcornstash.feature.settings

import com.thekozugroup.popcornstash.feature.downloads.formatBytes
import com.thekozugroup.popcornstash.feature.library.MediaFormat
import com.thekozugroup.popcornstash.update.AppUpdateUi
import com.thekozugroup.popcornstash.update.CheckProblem
import com.thekozugroup.popcornstash.update.FetchProblem
import com.thekozugroup.popcornstash.update.InstallFailure
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

    /** Opens Android's confirmation again, for an install that waits on it. */
    Confirm("Install", primary = true),
    OpenReleasePage("Open the release page", primary = true),
}

/**
 * The bar under the sentence: [percent] from 0 to 100 while downloading, or null while the
 * size is not known (an empty track); [working] for a step with no measure (checking the
 * download), drawn as a moving bar.
 */
data class UpdateProgress(val percent: Int?, val working: Boolean = false)

/** The words and the one action for an update state (android/DESIGN.md, Settings, This app). */
data class UpdateWords(
    val line: String,
    val detail: String? = null,
    val action: UpdateAction? = null,
    val busy: Boolean = false,
    val progress: UpdateProgress? = null,
)

/** "Version 0.1.0, build 412", like the server's "Version 0.2.0" above it. */
fun versionLine(ui: AppUpdateUi): String = "Version ${ui.version}, build ${ui.build}"

/** The switch row's line: what the daily check does, and where a new build shows while notifications are off. */
fun autoCheckLine(autoCheck: Boolean, notificationsOff: Boolean): String =
    if (autoCheck && notificationsOff) {
        "Asks GitHub once a day. Notifications are off, so a new build shows only here."
    } else {
        "Asks GitHub once a day for a newer build."
    }

fun updateWords(
    status: UpdateStatus,
    now: Long,
    zone: ZoneId = ZoneId.systemDefault(),
    locale: Locale = Locale.getDefault(),
): UpdateWords = when (status) {
    UpdateStatus.Idle -> UpdateWords("Not checked yet.", action = UpdateAction.Check)
    // The button keeps its words and turns busy; the sentence says what is happening.
    UpdateStatus.Checking -> UpdateWords("Asking GitHub for the newest build.", action = UpdateAction.Check, busy = true)
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
        detail = "Making sure it is the real build from GitHub.",
        progress = UpdateProgress(percent = null, working = true),
    )
    is UpdateStatus.ReadyToInstall -> if (status.cancelled) {
        UpdateWords(
            "Build ${status.build.number} was not installed.",
            detail = "Android's confirmation was closed. Install it whenever you like.",
            action = UpdateAction.Install,
        )
    } else {
        UpdateWords("Build ${status.build.number} is ready to install.", action = UpdateAction.Install)
    }
    is UpdateStatus.Installing -> UpdateWords(
        "Installing build ${status.build.number}",
        detail = if (status.canConfirm) {
            "Confirm in Android's window, or tap Install to show it again. PopcornStash closes while it updates: open it again afterwards. You stay signed in."
        } else {
            "Android may ask you to confirm. PopcornStash closes while it updates: open it again afterwards. You stay signed in."
        },
        action = UpdateAction.Confirm.takeIf { status.canConfirm },
    )
    is UpdateStatus.OtherKey -> UpdateWords(
        "Build ${status.build.number} needs a one-time reinstall.",
        detail = "This copy was not installed from GitHub, so it cannot update itself. Open the release page and download " +
            "build ${status.build.number}, then uninstall this app, open the download and sign in again.",
        action = UpdateAction.OpenReleasePage,
    )
    is UpdateStatus.Failed -> failedWords(status, zone, locale)
}

private fun failedWords(status: UpdateStatus.Failed, zone: ZoneId, locale: Locale): UpdateWords {
    val number = status.build?.number
    val build = number?.let { "build $it" } ?: "the update"
    val tryAgain = UpdateAction.UpdateAgain.takeIf { status.build != null }
    return when (val problem = status.problem) {
        is UpdateProblem.CheckFailed -> UpdateWords(
            when (val check = problem.problem) {
                CheckProblem.Offline -> "Could not reach GitHub. Check the connection and try again."
                is CheckProblem.RateLimited -> check.resetsAt?.let {
                    "GitHub is taking no more checks from this network for now. Try again after ${clockTime(it, zone, locale)}."
                } ?: "GitHub is taking no more checks from this network for now. Try again in an hour."
                is CheckProblem.HttpError -> "GitHub is having trouble right now. Try again later."
                CheckProblem.Unreadable -> "GitHub's answer could not be read. Try again later."
            },
            action = UpdateAction.CheckAgain,
        )
        is UpdateProblem.FetchFailed -> when (problem.problem) {
            FetchProblem.DownloadStopped -> UpdateWords("The download of $build stopped. Try again.", action = tryAgain)
            FetchProblem.ChecksumMismatch -> UpdateWords("The download was damaged, so it was deleted. Try again.", action = tryAgain)
            // Trying again would fetch the same file: these are for the owner to look into.
            FetchProblem.NotThisApp -> UpdateWords(
                "The download was not the right build.",
                detail = "It was deleted. Tell the owner.",
                action = UpdateAction.Check,
            )
            FetchProblem.OtherSigner -> UpdateWords(
                "${build.replaceFirstChar { it.uppercase(locale) }} was not signed by PopcornStash.",
                detail = "It was deleted. Do not install it by hand; tell the owner.",
                action = UpdateAction.Check,
            )
        }
        UpdateProblem.InstallCancelled -> UpdateWords(
            "${build.replaceFirstChar { it.uppercase(locale) }} was not installed.",
            detail = "Android's confirmation was closed. Update again whenever you like.",
            action = UpdateAction.Update.takeIf { status.build != null },
        )
        is UpdateProblem.InstallFailed -> when (problem.reason) {
            InstallFailure.Storage -> UpdateWords(
                "There is not enough space on this phone for $build.",
                detail = "Free some space and try again.",
                action = tryAgain,
            )
            InstallFailure.Incompatible -> UpdateWords(
                "This phone's Android cannot run $build.",
                detail = "Tell the owner.",
                action = UpdateAction.Check,
            )
            InstallFailure.Conflict -> UpdateWords(
                "Android refused $build: it does not match the app on this phone.",
                detail = "Tell the owner.",
                action = UpdateAction.Check,
            )
            InstallFailure.Blocked -> UpdateWords(
                "Android blocked the install of $build.",
                detail = "A device policy or Play Protect stopped it. Try again, or tell the owner.",
                action = tryAgain,
            )
            InstallFailure.Invalid -> UpdateWords("Android found the download of $build damaged. Try again.", action = tryAgain)
            InstallFailure.Other -> UpdateWords("Android did not install $build. Try again.", action = tryAgain)
        }
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
