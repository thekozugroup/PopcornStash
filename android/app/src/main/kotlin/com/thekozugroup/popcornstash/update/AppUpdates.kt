package com.thekozugroup.popcornstash.update

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
 * (android/release-cert.sha256, built into the app; null when the repository had none). A copy
 * signed with another key, such as a debug build, cannot install a release over itself, so it
 * is told so before anything is downloaded.
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
    private val inForeground: () -> Boolean = { true },
    private val ioContext: CoroutineContext = Dispatchers.IO,
) {
    private val _state = MutableStateFlow(
        AppUpdateUi(versionOf(versionName), installedBuild, UpdateStatus.Idle, memory.autoCheck),
    )
    val state: StateFlow<AppUpdateUi> = _state.asStateFlow()

    private val checking = Mutex()
    private var checkJob: Job? = null

    /** The person asked for this build: install it as soon as it is verified, while they are here. */
    @Volatile
    private var installWhenReady: Int? = null

    private val status: UpdateStatus get() = _state.value.status

    private fun set(status: UpdateStatus) = _state.update { it.copy(status = status) }

    /** Whether this copy is signed with a key other than the release key. */
    private val otherKey: Boolean by lazy {
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
        val file = ready?.let { File(fetcher.dir, it.fileName) }
        val restored = if (ready != null && file != null && ready.build.number > installedBuild && file.isFile && sha256Of(file) == ready.sha256) {
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
    suspend fun check(): AppBuild? = checking.withLock {
        if (busy) return null
        val before = status
        set(UpdateStatus.Checking)
        when (val result = checker.check()) {
            is CheckResult.Failed -> {
                set(UpdateStatus.Failed(UpdateProblem.CheckFailed(result.problem), null))
                null
            }
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
                build
            }
        }
    }

    /** The daily job: checks when automatic checks are on, and posts one notification per new build. */
    suspend fun backgroundCheck() {
        if (!memory.autoCheck) return
        val build = check() ?: return
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

    /** The release page of the build on offer, or every release: where a build is installed by hand. */
    fun releasePage(): String = when (val current = status) {
        is UpdateStatus.OtherKey -> current.build.pageUrl
        is UpdateStatus.Available -> current.build.pageUrl
        is UpdateStatus.Failed -> current.build?.pageUrl
        else -> null
    }?.takeIf { it.startsWith("https://") } ?: UpdateChecker.RELEASES_PAGE

    /** The system page where the person lets this app install apps. */
    fun permissionIntent() = installer.permissionIntent()

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
        installWhenReady = build.number
        notifier.cancel()
        set(UpdateStatus.Downloading(build, null))
        jobs.startDownload(build)
    }

    /** The person came back to the page: carry on if they have just allowed installs. */
    fun onResume() {
        val waiting = status as? UpdateStatus.NeedsPermission ?: return
        if (!installer.canInstall()) return
        val ready = memory.ready
        if (ready != null && ready.build.number == waiting.build.number) install() else startDownload(waiting.build)
    }

    /**
     * The download job's work: fetches and checks [build], reporting progress through the
     * state (and [onProgress], for its notification). Returns true when the build is verified.
     */
    suspend fun download(build: AppBuild, onProgress: suspend (percent: Int?) -> Unit = {}): Boolean {
        set(UpdateStatus.Downloading(build, null))
        memory.ready = null
        var shown: Int? = null
        val outcome = try {
            fetcher.fetch(
                build,
                onProgress = { done, total ->
                    val percent = total?.let { ((done * 100) / it).toInt().coerceIn(0, 100) }
                    if (percent != shown) {
                        shown = percent
                        set(UpdateStatus.Downloading(build, percent))
                        onProgress(percent)
                    }
                },
                onVerifying = { set(UpdateStatus.Verifying(build)) },
            )
        } catch (e: CancellationException) {
            // The job was stopped (by Android, or a newer download): say so instead of "Downloading" forever.
            installWhenReady = null
            set(UpdateStatus.Failed(UpdateProblem.FetchFailed(FetchProblem.DownloadStopped), build))
            throw e
        }
        return when (outcome) {
            is FetchOutcome.Failed -> {
                installWhenReady = null
                set(
                    if (outcome.problem == FetchProblem.OtherSigner) {
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
        set(UpdateStatus.Installing(ready.build))
        // Reading the file again and writing it into the session takes a moment: off the main thread.
        scope.launch(ioContext) { installNow(ready) }
        return true
    }

    private fun installNow(ready: ReadyBuild) {
        val file = File(fetcher.dir, ready.fileName)
        if (!file.isFile || sha256Of(file) != ready.sha256) {
            forgetDownload()
            set(UpdateStatus.Failed(UpdateProblem.FetchFailed(FetchProblem.ChecksumMismatch), ready.build))
            return
        }
        try {
            installer.install(file)
        } catch (e: IOException) {
            forgetDownload()
            set(UpdateStatus.Failed(UpdateProblem.InstallFailed(e.message), ready.build))
        }
    }

    /** What Android said about the install. */
    fun onInstallStatus(result: InstallStatus) {
        val build = (status as? UpdateStatus.Installing)?.build ?: memory.ready?.build
        when (result) {
            InstallStatus.NeedsConfirmation -> build?.let { set(UpdateStatus.Installing(it)) }
            InstallStatus.Installed -> {
                forgetDownload()
                set(UpdateStatus.UpToDate(memory.lastChecked ?: 0L))
            }
            InstallStatus.Cancelled -> {
                forgetDownload()
                set(UpdateStatus.Failed(UpdateProblem.InstallCancelled, build))
            }
            is InstallStatus.Failed -> {
                forgetDownload()
                set(UpdateStatus.Failed(UpdateProblem.InstallFailed(result.message), build))
            }
        }
    }

    private fun forgetDownload() {
        memory.ready = null
        fetcher.clear()
    }
}
