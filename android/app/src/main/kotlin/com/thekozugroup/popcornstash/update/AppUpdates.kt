package com.thekozugroup.popcornstash.update

import android.content.Intent
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import java.io.File
import java.io.IOException
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import kotlin.coroutines.CoroutineContext

/** The background jobs of the updater ([WorkManagerUpdateJobs] on a phone). */
interface UpdateJobs {
    /** The daily check on any network, or none. */
    fun scheduleDailyCheck(enabled: Boolean)

    /** Downloads and checks [build] in a job that outlives the screen. */
    fun startDownload(build: AppBuild)
}

/** The one notification the daily check posts ([UpdateNotifications] on a phone). */
interface UpdateNotifier {
    fun newBuild(build: AppBuild)

    fun cancel()
}

/**
 * The app's updater (docs/android.md, Updates): asks GitHub for a newer build of the app,
 * downloads and checks it in a job, and hands it to Android to install. One per process,
 * in [com.thekozugroup.popcornstash.AppContainer]; the jobs report back into it.
 *
 * [installedSigners] are the installed app's certificates, [releaseSigner] the release key's
 * (android/release-cert.sha256, built into the app; null when the repository had none), and
 * [debugBuild] is true for a debug build, which is never signed with the release key. A copy
 * that is not signed with the release key cannot install a GitHub build over itself, so it is
 * told so before anything is downloaded. A download signed with another key than a
 * release-signed copy's is refused and deleted, never offered for installing by hand.
 */
class AppUpdates(
    versionName: String,
    private val installedBuild: Int,
    private val memory: UpdateMemory,
    private val checker: UpdateChecker,
    private val fetcher: UpdateFetcher,
    private val installer: UpdateInstaller,
    private val jobs: UpdateJobs,
    private val notifier: UpdateNotifier,
    private val scope: CoroutineScope,
    private val installedSigners: () -> Set<String>,
    private val releaseSigner: String?,
    private val debugBuild: Boolean = false,
    private val inForeground: () -> Boolean = { true },
    private val ioContext: CoroutineContext = Dispatchers.IO,
) {
    private val _state = MutableStateFlow(
        AppUpdateUi(versionOf(versionName), installedBuild, UpdateStatus.Idle, memory.autoCheck),
    )
    val state: StateFlow<AppUpdateUi> = _state.asStateFlow()

    private val checking = Mutex()
    private var checkJob: Job? = null

    /** One download at a time: a job replaced by a newer one finishes with its file before the next starts. */
    private val downloading = Mutex()

    /** Each requested or started download takes a number; only the newest one reports. */
    private val downloads = AtomicInteger()

    /** True from "Install" until the session is with Android, so a quick return to the page does not undo it. */
    private val writingSession = AtomicBoolean(false)

    /** The person asked for this build: install it as soon as it is verified, while they are here. */
    @Volatile
    private var installWhenReady: Int? = null

    private val status: UpdateStatus get() = _state.value.status

    private fun set(status: UpdateStatus) = _state.update { it.copy(status = status) }

    /** Whether this copy is known not to be signed with the release key: a debug build, or another certificate. */
    private val otherKey: Boolean by lazy {
        if (debugBuild) return@lazy true
        val release = releaseSigner?.lowercase() ?: return@lazy false
        val installed = installedSigners()
        installed.isNotEmpty() && release !in installed
    }

    /**
     * At the start of the process: a verified download of a newer build is ready to install,
     * anything else downloaded is deleted, and a newer build seen in the last check is offered.
     * It does nothing once something else has started (a check, or a download job WorkManager
     * resumed), so it never deletes a file being written or hides a newer state.
     */
    fun restore() {
        if (status != UpdateStatus.Idle) return
        val ready = memory.ready
        val restored = if (ready != null && ready.build.number > installedBuild && isIntact(ready)) {
            UpdateStatus.ReadyToInstall(ready.build)
        } else {
            memory.ready = null
            fetcher.clear()
            val newest = AppBuilds.offer(memory.newestSeen, installedBuild)
            val lastChecked = memory.lastChecked
            when {
                newest != null -> offer(newest)
                lastChecked != null -> UpdateStatus.UpToDate(lastChecked)
                else -> UpdateStatus.Idle
            }
        }
        _state.update { if (it.status == UpdateStatus.Idle) it.copy(status = restored) else it }
    }

    /** Whether the verified file is still there, unchanged. The system may clear the cache at any time. */
    private fun isIntact(ready: ReadyBuild): Boolean {
        val file = File(fetcher.dir, ready.fileName)
        return try {
            file.isFile && sha256Of(file) == ready.sha256
        } catch (_: IOException) {
            false
        } catch (_: RuntimeException) {
            false
        }
    }

    private fun offer(build: AppBuild): UpdateStatus = if (otherKey) UpdateStatus.OtherKey(build) else UpdateStatus.Available(build)

    /** True while a download, a check of it or an install runs: a new check would only get in the way. */
    private val busy: Boolean
        get() = when (status) {
            is UpdateStatus.Downloading, is UpdateStatus.Verifying, is UpdateStatus.Installing -> true
            else -> false
        }

    /** "Check for updates". */
    fun checkNow() {
        if (busy || checkJob?.isActive == true) return
        checkJob = scope.launch { check() }
    }

    /** Asks GitHub and shows what it said; returns the build on offer, if any. */
    suspend fun check(): AppBuild? = (ask() as? CheckResult.Answered)?.let { AppBuilds.offer(it.newest, installedBuild) }

    /** Asks GitHub and shows the answer; null when a download or an install was running. */
    private suspend fun ask(): CheckResult? = checking.withLock {
        if (busy) return null
        val before = status
        set(UpdateStatus.Checking)
        val result = checker.check()
        when (result) {
            is CheckResult.Failed -> set(UpdateStatus.Failed(UpdateProblem.CheckFailed(result.problem), null))
            is CheckResult.Answered -> {
                val build = AppBuilds.offer(result.newest, installedBuild)
                val ready = memory.ready
                set(
                    when {
                        build == null -> UpdateStatus.UpToDate(result.checkedAt)
                        ready != null && ready.build.number == build.number && before is UpdateStatus.ReadyToInstall ->
                            UpdateStatus.ReadyToInstall(build)
                        else -> offer(build)
                    },
                )
            }
        }
        result
    }

    /**
     * The daily job: checks when automatic checks are on, and posts one notification per new
     * build this copy can install. A copy that needs a one-time reinstall gets none (Settings
     * says so), and a build that disappeared from GitHub no longer holds back the next notification.
     */
    suspend fun backgroundCheck() {
        if (!memory.autoCheck) return
        val answer = ask() as? CheckResult.Answered ?: return
        val newest = answer.newest?.number ?: 0
        if (memory.lastNotified > newest) memory.lastNotified = newest
        val build = AppBuilds.offer(answer.newest, installedBuild) ?: return
        if (otherKey) return
        if (build.number > memory.lastNotified) {
            memory.lastNotified = build.number
            notifier.newBuild(build)
        }
    }

    fun setAutoCheck(enabled: Boolean) {
        memory.autoCheck = enabled
        _state.update { it.copy(autoCheck = enabled) }
        jobs.scheduleDailyCheck(enabled)
    }

    /** Makes sure the daily check matches the setting (at every start of the process). */
    fun schedule() = jobs.scheduleDailyCheck(memory.autoCheck)

    /**
     * Whether to ask for permission to post notifications now (Android 13 and later, while it is
     * not given): whenever the person turns automatic checks on, and otherwise only the first
     * time they check or update, so the daily check's notification can reach them.
     */
    fun askForNotifications(turningOn: Boolean): Boolean {
        if (!turningOn && memory.askedForNotifications) return false
        memory.askedForNotifications = true
        return true
    }

    /**
     * The release page of the build on offer, or every release: where a build is installed by
     * hand. Only pages of this repository on github.com open; anything else in the list is ignored.
     */
    fun releasePage(): String = when (val current = status) {
        is UpdateStatus.OtherKey -> current.build.pageUrl
        is UpdateStatus.Available -> current.build.pageUrl
        is UpdateStatus.Failed -> current.build?.pageUrl
        else -> null
    }?.takeIf { it.startsWith(UpdateChecker.RELEASES_PAGE + "/") } ?: UpdateChecker.RELEASES_PAGE

    /** The system page where the person lets this app install apps. */
    fun permissionIntent() = installer.permissionIntent()

    /** Android's confirmation for the install waiting on the person, to open again from the page. */
    fun confirmation(): Intent? = installer.confirmation()

    /**
     * "Update" (and "Try again" after a failed download): downloads the build on offer.
     * Returns false when Android first needs the person's permission for this app to install
     * apps; the page then opens [permissionIntent], and [onResume] carries on once it is given.
     */
    fun update(): Boolean {
        val build = buildOnOffer() ?: return true
        if (!installer.canInstall()) {
            set(UpdateStatus.NeedsPermission(build))
            return false
        }
        startDownload(build)
        return true
    }

    private fun buildOnOffer(): AppBuild? = when (val current = status) {
        is UpdateStatus.Available -> current.build
        is UpdateStatus.NeedsPermission -> current.build
        is UpdateStatus.Failed -> current.build
        else -> null
    }

    private fun startDownload(build: AppBuild) {
        // Whatever download was running is replaced (WorkManager cancels it): it reports no more.
        downloads.incrementAndGet()
        installWhenReady = build.number
        notifier.cancel()
        set(UpdateStatus.Downloading(build, null))
        jobs.startDownload(build)
    }

    /**
     * The person came back to the page: carry on if they have just allowed installs, and find
     * the way out of "Installing" when Android has no session of this app's any more (it was
     * dropped without a word), so the page is never left without an action.
     */
    fun onResume() {
        when (val current = status) {
            is UpdateStatus.NeedsPermission -> {
                if (!installer.canInstall()) return
                val ready = memory.ready
                if (ready != null && ready.build.number == current.build.number) install() else startDownload(current.build)
            }
            is UpdateStatus.Installing -> if (!writingSession.get() && !installer.hasSession()) set(installEnded(current.build))
            else -> Unit
        }
    }

    /** An install that ended without an answer: the verified file is offered again, or else the build. */
    private fun installEnded(build: AppBuild): UpdateStatus {
        val ready = memory.ready
        return if (ready != null && ready.build.number == build.number) UpdateStatus.ReadyToInstall(build) else offer(build)
    }

    /**
     * The download job's work: fetches and checks [build], reporting progress through the
     * state (and [onProgress], for its notification). Returns true when the build is verified.
     * A job that a newer download replaced waits for this one's files to be gone and then
     * neither downloads nor reports.
     */
    suspend fun download(build: AppBuild, onProgress: suspend (percent: Int?) -> Unit = {}): Boolean {
        val attempt = downloads.incrementAndGet()
        fun current() = downloads.get() == attempt
        fun report(status: UpdateStatus) {
            if (current()) set(status)
        }
        return downloading.withLock {
            if (!current()) return@withLock false
            report(UpdateStatus.Downloading(build, null))
            memory.ready = null
            var shown: Int? = null
            val outcome = try {
                fetcher.fetch(
                    build,
                    onProgress = { done, total ->
                        val percent = total?.let { ((done * 100) / it).toInt().coerceIn(0, 100) }
                        if (percent != shown && current()) {
                            shown = percent
                            report(UpdateStatus.Downloading(build, percent))
                            onProgress(percent)
                        }
                    },
                    onVerifying = { report(UpdateStatus.Verifying(build)) },
                )
            } catch (e: CancellationException) {
                // The job was stopped (by Android, or a newer download): say so instead of "Downloading" forever.
                if (current()) {
                    installWhenReady = null
                    set(UpdateStatus.Failed(UpdateProblem.FetchFailed(FetchProblem.DownloadStopped), build))
                }
                throw e
            }
            if (!current()) {
                // Replaced while it finished: the newer download starts from nothing.
                fetcher.clear()
                return@withLock false
            }
            when (outcome) {
                is FetchOutcome.Failed -> {
                    installWhenReady = null
                    set(
                        // Only a copy known not to be release-signed hears "reinstall once"; for any other
                        // copy a download signed with another key is a build that is not PopcornStash's.
                        if (outcome.problem == FetchProblem.OtherSigner && otherKey) {
                            UpdateStatus.OtherKey(build)
                        } else {
                            UpdateStatus.Failed(UpdateProblem.FetchFailed(outcome.problem), build)
                        },
                    )
                    false
                }
                is FetchOutcome.Verified -> {
                    memory.ready = ReadyBuild(build, outcome.sha256, outcome.file.name)
                    set(UpdateStatus.ReadyToInstall(build))
                    if (installWhenReady == build.number && inForeground()) install()
                    true
                }
            }
        }
    }

    /**
     * "Install": hands the verified file to Android. Returns false when Android first needs
     * the person's permission (as [update] does).
     */
    fun install(): Boolean {
        installWhenReady = null
        val ready = memory.ready ?: return true
        if (!installer.canInstall()) {
            set(UpdateStatus.NeedsPermission(ready.build))
            return false
        }
        writingSession.set(true)
        set(UpdateStatus.Installing(ready.build))
        // Reading the file again and writing it into the session takes a moment: off the main thread.
        scope.launch(ioContext) {
            try {
                installNow(ready)
            } finally {
                writingSession.set(false)
            }
        }
        return true
    }

    private fun installNow(ready: ReadyBuild) {
        if (!isIntact(ready)) {
            forgetDownload()
            set(UpdateStatus.Failed(UpdateProblem.FetchFailed(FetchProblem.ChecksumMismatch), ready.build))
            return
        }
        try {
            installer.install(File(fetcher.dir, ready.fileName))
        } catch (_: IOException) {
            forgetDownload()
            set(UpdateStatus.Failed(UpdateProblem.InstallFailed(InstallFailure.Other), ready.build))
        } catch (_: RuntimeException) {
            forgetDownload()
            set(UpdateStatus.Failed(UpdateProblem.InstallFailed(InstallFailure.Other), ready.build))
        }
    }

    /** What Android said about the install. */
    fun onInstallStatus(result: InstallStatus) {
        val ready = memory.ready
        val build = (status as? UpdateStatus.Installing)?.build ?: ready?.build
        when (result) {
            is InstallStatus.NeedsConfirmation -> build?.let {
                // Without a confirmation to show, the person can only start again from the verified file.
                set(if (result.canReopen) UpdateStatus.Installing(it, canConfirm = true) else installEnded(it))
            }
            InstallStatus.Installed -> {
                forgetDownload()
                set(UpdateStatus.UpToDate(memory.lastChecked ?: 0L))
            }
            InstallStatus.Cancelled -> when {
                // Closing Android's confirmation is not a failure: the verified file stays for "Install".
                ready != null && build != null && ready.build.number == build.number && isIntact(ready) ->
                    set(UpdateStatus.ReadyToInstall(build, cancelled = true))
                else -> {
                    forgetDownload()
                    set(UpdateStatus.Failed(UpdateProblem.InstallCancelled, build))
                }
            }
            is InstallStatus.Failed -> {
                forgetDownload()
                set(UpdateStatus.Failed(UpdateProblem.InstallFailed(result.reason), build))
            }
        }
    }

    private fun forgetDownload() {
        memory.ready = null
        fetcher.clear()
    }
}
