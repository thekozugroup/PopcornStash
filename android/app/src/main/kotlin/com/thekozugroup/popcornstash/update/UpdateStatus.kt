package com.thekozugroup.popcornstash.update

/** Why an update did not happen, in words the Settings page shows. */
sealed interface UpdateProblem {
    data class CheckFailed(val problem: CheckProblem) : UpdateProblem

    data class FetchFailed(val problem: FetchProblem) : UpdateProblem

    /** The person backed out of Android's confirmation, and the download was no longer there to install again. */
    data object InstallCancelled : UpdateProblem

    /** Android refused the install, for [reason]. */
    data class InstallFailed(val reason: InstallFailure) : UpdateProblem
}

/** Why Android did not install a build, from PackageInstaller's status (its own message goes to the log only). */
enum class InstallFailure {
    /** STATUS_FAILURE_STORAGE: not enough space. */
    Storage,

    /** STATUS_FAILURE_INCOMPATIBLE: the build cannot run on this phone's Android. */
    Incompatible,

    /** STATUS_FAILURE_CONFLICT: it does not fit the installed app (another key, or an older build). */
    Conflict,

    /** STATUS_FAILURE_BLOCKED: a policy or a verifier such as Play Protect stopped it. */
    Blocked,

    /** STATUS_FAILURE_INVALID: Android found the file damaged. */
    Invalid,

    /** Anything else, or the session could not be written. */
    Other,
}

/** Where the updater is (android/DESIGN.md, Settings, This app). */
sealed interface UpdateStatus {
    /** Nothing checked yet. */
    data object Idle : UpdateStatus

    data object Checking : UpdateStatus

    data class UpToDate(val checkedAt: Long) : UpdateStatus

    data class Available(val build: AppBuild) : UpdateStatus

    /** The person asked to update, and Android first needs their permission for this app to install apps. */
    data class NeedsPermission(val build: AppBuild) : UpdateStatus

    /** [percent] from 0 to 100, null until the size is known. */
    data class Downloading(val build: AppBuild, val percent: Int?) : UpdateStatus

    /** The file is all there; its checksum and signature are being checked. */
    data class Verifying(val build: AppBuild) : UpdateStatus

    /** Verified and waiting; [cancelled] when the person closed Android's confirmation and the file was kept. */
    data class ReadyToInstall(val build: AppBuild, val cancelled: Boolean = false) : UpdateStatus

    /**
     * Handed to Android; its confirmation may be on screen. [canConfirm] once Android has given
     * its confirmation, which the page can open again if the person left it.
     */
    data class Installing(val build: AppBuild, val canConfirm: Boolean = false) : UpdateStatus

    /**
     * A newer build exists, but this copy is not signed with the release key (a debug build, or
     * one signed with another key) and cannot update itself: it needs a one-time reinstall.
     */
    data class OtherKey(val build: AppBuild) : UpdateStatus

    data class Failed(val problem: UpdateProblem, val build: AppBuild?) : UpdateStatus
}

/** What the Settings page shows about this app and its updates. */
data class AppUpdateUi(
    /** The version without the build ("0.1.0"). */
    val version: String,
    val build: Int,
    val status: UpdateStatus,
    val autoCheck: Boolean,
)

/** The version part of a versionName ("0.1.0+412" is 0.1.0). */
fun versionOf(versionName: String): String = versionName.substringBefore('+')
