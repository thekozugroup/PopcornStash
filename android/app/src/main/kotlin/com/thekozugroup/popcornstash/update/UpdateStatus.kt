package com.thekozugroup.popcornstash.update

/** Why an update did not happen, in words the Settings page shows. */
sealed interface UpdateProblem {
    data class CheckFailed(val problem: CheckProblem) : UpdateProblem

    data class FetchFailed(val problem: FetchProblem) : UpdateProblem

    /** The person backed out of Android's confirmation. */
    data object InstallCancelled : UpdateProblem

    /** Android refused the install; [message] is its reason, when it gave one. */
    data class InstallFailed(val message: String?) : UpdateProblem
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

    data class ReadyToInstall(val build: AppBuild) : UpdateStatus

    /** Handed to Android; its confirmation may be on screen. */
    data class Installing(val build: AppBuild) : UpdateStatus

    /** A newer build exists, but this copy is signed with another key (a debug build) and cannot update itself. */
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
