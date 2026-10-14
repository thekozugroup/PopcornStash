package com.thekozugroup.popcornstash.update

import android.content.Intent
import com.thekozugroup.popcornstash.data.api.baseHttpClient
import com.thekozugroup.popcornstash.update.UpdateFixtures.appRelease
import com.thekozugroup.popcornstash.update.UpdateFixtures.list
import com.thekozugroup.popcornstash.update.UpdateFixtures.serverRelease
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.async
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import mockwebserver3.Dispatcher
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import mockwebserver3.RecordedRequest
import okio.Buffer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.io.File
import java.io.IOException
import java.security.MessageDigest
import java.util.concurrent.TimeUnit

/**
 * The updater's states from start to install: GitHub and the download are a MockWebServer,
 * PackageManager, PackageInstaller, WorkManager and the notification are stand-ins.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class AppUpdatesTest {
    @get:Rule
    val temp = TemporaryFolder()

    private val ours = "7721ec2f9c281a3bde0e53633e947fa6e89e31ef53cb9816e4dda8ef1224fda4"
    private val theirs = "f288080fcc7845f386f1ff1911a122d6b5285636f5553d2ba5327e018d0275eb"
    private val apk = ByteArray(50_000) { (it % 97).toByte() }
    private val apkSha = MessageDigest.getInstance("SHA-256").digest(apk).toHex()

    private val server = MockWebServer()
    private var releases = ""
    private var apkFacts = ApkFacts("com.thekozugroup.popcornstash", 415, setOf(ours))
    private var installedSigners = setOf(ours)
    private var releaseSigner: String? = ours
    private var debugBuild = false
    private var foreground = true
    private var slowApk = false
    private val memory = InMemoryUpdateMemory()

    private class FakeInstaller : UpdateInstaller {
        var allowed = true
        var failWith: Exception? = null
        var session = false
        val installed = mutableListOf<File>()

        override fun canInstall() = allowed

        override fun permissionIntent() = Intent("manage unknown app sources")

        override fun install(apk: File) {
            failWith?.let { throw it }
            installed += apk
            session = true
        }

        override fun confirmation(): Intent? = null

        override fun hasSession() = session
    }

    private class FakeJobs : UpdateJobs {
        val daily = mutableListOf<Boolean>()
        val downloads = mutableListOf<AppBuild>()

        override fun scheduleDailyCheck(enabled: Boolean) {
            daily += enabled
        }

        override fun startDownload(build: AppBuild) {
            downloads += build
        }
    }

    private class FakeNotifier : UpdateNotifier {
        val posted = mutableListOf<Int>()
        var cancelled = 0

        override fun newBuild(build: AppBuild) {
            posted += build.number
        }

        override fun cancel() {
            cancelled++
        }
    }

    private val installer = FakeInstaller()
    private val jobs = FakeJobs()
    private val notifier = FakeNotifier()
    private lateinit var fetcher: UpdateFetcher

    @Before
    fun setUp() {
        server.dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse = when (request.url.encodedPath) {
                "/releases" -> MockResponse.Builder().body(releases).build()
                "/files/android-415/popcornstash-android-415.apk" -> MockResponse.Builder().body(Buffer().write(apk))
                    .apply { if (slowApk) throttleBody(2_000, 100, TimeUnit.MILLISECONDS) }
                    .build()
                "/files/android-415/popcornstash-android-415.apk.sha256" ->
                    MockResponse.Builder().body("$apkSha  popcornstash-android-415.apk\n").build()
                else -> MockResponse.Builder().code(404).build()
            }
        }
        server.start()
        releases = list(serverRelease("0.2.0"), appRelease(415, server.url("/files/android-415").toString().trimEnd('/')), appRelease(412))
        val http = baseHttpClient("PopcornStash-Android/test")
        fetcher = UpdateFetcher(http, temp.newFolder("updates"), "com.thekozugroup.popcornstash", { apkFacts }, { installedSigners })
    }

    @After
    fun tearDown() = server.close()

    private fun TestScope.updates(installedBuild: Int = 412) = AppUpdates(
        versionName = "0.1.0+$installedBuild",
        installedBuild = installedBuild,
        memory = memory,
        checker = UpdateChecker(baseHttpClient("PopcornStash-Android/test"), memory, server.url("/releases"), clock = { 1_000L }),
        fetcher = fetcher,
        installer = installer,
        jobs = jobs,
        notifier = notifier,
        scope = backgroundScope,
        installedSigners = { installedSigners },
        releaseSigner = releaseSigner,
        debugBuild = debugBuild,
        inForeground = { foreground },
        ioContext = StandardTestDispatcher(testScheduler),
    )

    private fun AppUpdates.status() = state.value.status

    private fun releaseCount() = server.requestCount

    // ------------------------------------------------------------------ start ----

    @Test
    fun theVersionAndBuildAreTheInstalledApps() = runTest {
        val ui = updates(412).state.value
        assertEquals("0.1.0", ui.version)
        assertEquals(412, ui.build)
        assertTrue(ui.autoCheck)
        assertEquals(UpdateStatus.Idle, ui.status)
    }

    @Test
    fun atStartTheLastAnswerIsShownWithoutAsking() = runTest {
        updates().apply { restore() }.let { assertEquals(UpdateStatus.Idle, it.status()) }

        memory.lastChecked = 500L
        updates().apply { restore() }.let { assertEquals(UpdateStatus.UpToDate(500L), it.status()) }

        memory.remember("\"e\"", UpdateFixtures.build(415))
        updates().apply { restore() }.let { assertEquals(UpdateStatus.Available(UpdateFixtures.build(415)), it.status()) }
        assertEquals(0, releaseCount())
    }

    @Test
    fun atStartAVerifiedDownloadIsReadyAndAnythingElseIsDeleted() = runTest {
        val build = UpdateFixtures.build(415)
        val file = fetcher.dir.resolve("popcornstash-android-415.apk").apply { writeBytes(apk) }
        memory.ready = ReadyBuild(build, apkSha, file.name)
        updates().apply { restore() }.let { assertEquals(UpdateStatus.ReadyToInstall(build), it.status()) }

        // Changed on disk since it was checked: it goes.
        file.writeBytes(apk + 1)
        updates().apply { restore() }.let { assertEquals(UpdateStatus.Idle, it.status()) }
        assertNull(memory.ready)
        assertFalse(file.exists())
    }

    @Test
    fun aLateStartDoesNotUndoWhatHasBegun() = runTest {
        val updates = updates()
        updates.check()
        updates.update()
        val partial = fetcher.dir.resolve("popcornstash-android-415.apk.part").apply { writeText("half") }
        updates.restore()
        assertTrue(updates.status() is UpdateStatus.Downloading)
        assertTrue(partial.exists())
    }

    @Test
    fun afterTheUpdateTheOldDownloadIsDeleted() = runTest {
        val build = UpdateFixtures.build(415)
        val file = fetcher.dir.resolve("popcornstash-android-415.apk").apply { writeBytes(apk) }
        memory.ready = ReadyBuild(build, apkSha, file.name)
        memory.remember("\"e\"", build)
        val updates = updates(installedBuild = 415)
        updates.restore()
        assertNull(memory.ready)
        assertFalse(file.exists())
        assertEquals(UpdateStatus.Idle, updates.status())
    }

    // ------------------------------------------------------------------ check ----

    @Test
    fun aNewerBuildIsOffered() = runTest {
        val updates = updates(412)
        assertEquals(415, updates.check()?.number)
        val status = updates.status() as UpdateStatus.Available
        assertEquals(415, status.build.number)
        assertEquals(status.build.pageUrl, updates.releasePage())
    }

    @Test
    fun theNewestBuildInstalledIsUpToDate() = runTest {
        val updates = updates(415)
        assertNull(updates.check())
        assertEquals(UpdateStatus.UpToDate(1_000L), updates.status())
    }

    @Test
    fun aCopySignedWithAnotherKeyIsToldBeforeAnyDownload() = runTest {
        installedSigners = setOf(theirs)
        val updates = updates(1)
        updates.check()
        val status = updates.status() as UpdateStatus.OtherKey
        assertEquals(415, status.build.number)
        assertEquals("https://github.com/thekozugroup/PopcornStash/releases/tag/android-415", updates.releasePage())
        // Update does nothing from here: the release page is the way.
        updates.update()
        assertTrue(jobs.downloads.isEmpty())
    }

    @Test
    fun aDebugBuildIsToldBeforeAnyDownloadAndIsNotNotified() = runTest {
        // No release key in the build (as today) and a debug build: it can never install a GitHub build.
        releaseSigner = null
        debugBuild = true
        installedSigners = setOf(theirs)
        val updates = updates()
        updates.backgroundCheck()
        assertEquals(415, (updates.status() as UpdateStatus.OtherKey).build.number)
        assertTrue("no notification for a build this copy cannot install", notifier.posted.isEmpty())
        // A start shows it again from the last answer.
        updates().apply { restore() }.let { assertTrue(it.status() is UpdateStatus.OtherKey) }
    }

    @Test
    fun aCopySignedWithAnotherKeyIsNotNotified() = runTest {
        installedSigners = setOf(theirs)
        updates(1).backgroundCheck()
        assertTrue(notifier.posted.isEmpty())
        assertEquals(0, memory.lastNotified)
    }

    @Test
    fun aReleaseSignedCopyRefusesADownloadSignedWithAnotherKey() = runTest {
        // The installed app is the release build; the file on GitHub (and its checksum) were replaced.
        apkFacts = apkFacts.copy(signers = setOf(theirs), lineage = setOf(theirs))
        val updates = updates()
        updates.check()
        val build = (updates.status() as UpdateStatus.Available).build
        updates.update()
        assertFalse(updates.download(build))
        // Not "reinstall by hand from the release page": a refused build, deleted.
        assertEquals(UpdateStatus.Failed(UpdateProblem.FetchFailed(FetchProblem.OtherSigner), build), updates.status())
        assertNull(memory.ready)
        assertEquals(emptyList<String>(), fetcher.dir.list().orEmpty().toList())
    }

    @Test
    fun withoutAReleaseKeyInTheBuildADownloadFromAnotherKeyIsRefused() = runTest {
        // Not a debug build, and no release key built in: whether this copy is release-signed is not known,
        // so a download signed with another key is refused rather than offered for installing by hand.
        releaseSigner = null
        installedSigners = setOf(theirs)
        val updates = updates()
        updates.check()
        val build = (updates.status() as UpdateStatus.Available).build
        updates.update()
        assertFalse(updates.download(build))
        assertEquals(UpdateStatus.Failed(UpdateProblem.FetchFailed(FetchProblem.OtherSigner), build), updates.status())
        assertEquals(emptyList<String>(), fetcher.dir.list().orEmpty().toList())
    }

    @Test
    fun onlyThisRepositorysPagesOpen() = runTest {
        memory.remember("\"e\"", UpdateFixtures.build(415).copy(pageUrl = "https://example.test/releases/tag/android-415"))
        val updates = updates().apply { restore() }
        assertTrue(updates.status() is UpdateStatus.Available)
        assertEquals(UpdateChecker.RELEASES_PAGE, updates.releasePage())
    }

    @Test
    fun aFailedCheckSaysWhy() = runTest {
        server.close()
        val updates = updates()
        updates.check()
        assertEquals(UpdateStatus.Failed(UpdateProblem.CheckFailed(CheckProblem.Offline), null), updates.status())
        assertEquals(UpdateChecker.RELEASES_PAGE, updates.releasePage())
    }

    // ----------------------------------------------------------------- update ----

    @Test
    fun updatingAsksForThePermissionFirstThenCarriesOn() = runTest {
        installer.allowed = false
        val updates = updates()
        updates.check()
        val build = (updates.status() as UpdateStatus.Available).build

        assertFalse(updates.update())
        assertEquals(UpdateStatus.NeedsPermission(build), updates.status())
        assertTrue(jobs.downloads.isEmpty())

        // Back without allowing it: still waiting.
        updates.onResume()
        assertEquals(UpdateStatus.NeedsPermission(build), updates.status())

        installer.allowed = true
        updates.onResume()
        assertEquals(UpdateStatus.Downloading(build, null), updates.status())
        assertEquals(listOf(build), jobs.downloads)
    }

    @Test
    fun aVerifiedDownloadInstallsWhileThePersonIsHere() = runTest {
        notifier.posted += 415
        val updates = updates()
        updates.check()
        val build = (updates.status() as UpdateStatus.Available).build
        assertTrue(updates.update())
        assertEquals(1, notifier.cancelled)

        val percents = mutableListOf<Int?>()
        assertTrue(updates.download(build) { percents += it })
        assertEquals(100, percents.last())
        assertEquals(ReadyBuild(build, apkSha, "popcornstash-android-415.apk"), memory.ready)
        assertEquals(UpdateStatus.Installing(build), updates.status())
        runCurrent()
        assertEquals(listOf("popcornstash-android-415.apk"), installer.installed.map { it.name })
    }

    @Test
    fun inTheBackgroundADownloadWaitsForInstall() = runTest {
        foreground = false
        val updates = updates()
        updates.check()
        val build = (updates.status() as UpdateStatus.Available).build
        updates.update()
        updates.download(build)
        assertEquals(UpdateStatus.ReadyToInstall(build), updates.status())
        assertTrue(installer.installed.isEmpty())

        assertTrue(updates.install())
        runCurrent()
        assertEquals(1, installer.installed.size)
    }

    @Test
    fun aDownloadThatDoesNotMatchIsAFailureWithTryAgain() = runTest {
        apkFacts = apkFacts.copy(versionCode = 414)
        val updates = updates()
        updates.check()
        val build = (updates.status() as UpdateStatus.Available).build
        updates.update()
        assertFalse(updates.download(build))
        assertEquals(UpdateStatus.Failed(UpdateProblem.FetchFailed(FetchProblem.NotThisApp), build), updates.status())
        assertNull(memory.ready)
        // "Try again" downloads the same build again.
        updates.update()
        assertEquals(listOf(build, build), jobs.downloads)
    }

    @Test
    fun aFileChangedBeforeInstallIsNotInstalled() = runTest {
        foreground = false
        val updates = updates()
        updates.check()
        val build = (updates.status() as UpdateStatus.Available).build
        updates.download(build)
        fetcher.dir.resolve("popcornstash-android-415.apk").appendBytes(byteArrayOf(1))
        updates.install()
        runCurrent()
        assertTrue(installer.installed.isEmpty())
        assertEquals(UpdateStatus.Failed(UpdateProblem.FetchFailed(FetchProblem.ChecksumMismatch), build), updates.status())
        assertEquals(emptyList<String>(), fetcher.dir.list().orEmpty().toList())
    }

    // ---------------------------------------------------------------- install ----

    private suspend fun TestScope.installing(): Pair<AppUpdates, AppBuild> {
        val updates = updates()
        updates.check()
        val build = (updates.status() as UpdateStatus.Available).build
        updates.update()
        updates.download(build)
        runCurrent()
        return updates to build
    }

    @Test
    fun androidsConfirmationCanBeOpenedAgain() = runTest {
        val (updates, build) = installing()
        updates.onInstallStatus(InstallStatus.NeedsConfirmation(canReopen = true))
        assertEquals(UpdateStatus.Installing(build, canConfirm = true), updates.status())
    }

    @Test
    fun withoutAConfirmationToShowTheFileIsOfferedAgain() = runTest {
        val (updates, build) = installing()
        updates.onInstallStatus(InstallStatus.NeedsConfirmation(canReopen = false))
        assertEquals(UpdateStatus.ReadyToInstall(build), updates.status())
        assertTrue(fetcher.dir.resolve("popcornstash-android-415.apk").isFile)
    }

    @Test
    fun anInstallAndroidDroppedWithoutAWordIsNotLeftInstalling() = runTest {
        val (updates, build) = installing()
        // Back on the page while Android still holds the session (its confirmation left with Home): still installing.
        updates.onResume()
        assertEquals(UpdateStatus.Installing(build), updates.status())
        // Android has no session of this app's any more and never answered: the verified file is offered again.
        installer.session = false
        updates.onResume()
        assertEquals(UpdateStatus.ReadyToInstall(build), updates.status())
        assertTrue(updates.install())
        runCurrent()
        assertEquals(2, installer.installed.size)
    }

    @Test
    fun aCancelledInstallKeepsTheVerifiedFileForInstall() = runTest {
        val (updates, build) = installing()
        updates.onInstallStatus(InstallStatus.Cancelled)
        assertEquals(UpdateStatus.ReadyToInstall(build, cancelled = true), updates.status())
        assertEquals(build, memory.ready?.build)
        assertTrue(fetcher.dir.resolve("popcornstash-android-415.apk").isFile)
        // "Install" hands the same file to Android, without downloading it again.
        val requests = releaseCount()
        assertTrue(updates.install())
        runCurrent()
        assertEquals(2, installer.installed.size)
        assertEquals(requests, releaseCount())
    }

    @Test
    fun aCancelWhenTheFileIsGoneOffersUpdateAgain() = runTest {
        val (updates, build) = installing()
        fetcher.clear()
        updates.onInstallStatus(InstallStatus.Cancelled)
        assertEquals(UpdateStatus.Failed(UpdateProblem.InstallCancelled, build), updates.status())
        assertNull(memory.ready)
    }

    @Test
    fun aRefusedInstallSaysWhyInPlainWords() = runTest {
        val (updates, build) = installing()
        updates.onInstallStatus(InstallStatus.Failed(InstallFailure.Storage))
        assertEquals(UpdateStatus.Failed(UpdateProblem.InstallFailed(InstallFailure.Storage), build), updates.status())
        assertEquals(emptyList<String>(), fetcher.dir.list().orEmpty().toList())
    }

    @Test
    fun aSessionThatCannotBeWrittenIsAFailedInstall() = runTest {
        installer.failWith = IOException("No space left on device")
        val (updates, build) = installing()
        assertEquals(UpdateStatus.Failed(UpdateProblem.InstallFailed(InstallFailure.Other), build), updates.status())
        assertEquals(emptyList<String>(), fetcher.dir.list().orEmpty().toList())
    }

    @Test
    fun aSessionAndroidRefusesToOpenIsAFailedInstallNotACrash() = runTest {
        // createSession throws SecurityException or IllegalStateException (too many sessions).
        installer.failWith = SecurityException("Too many active sessions")
        val (updates, build) = installing()
        assertEquals(UpdateStatus.Failed(UpdateProblem.InstallFailed(InstallFailure.Other), build), updates.status())
    }

    @Test
    fun aDownloadFileGoneBeforeInstallIsAFailureNotACrash() = runTest {
        foreground = false
        val updates = updates()
        updates.check()
        val build = (updates.status() as UpdateStatus.Available).build
        updates.download(build)
        fetcher.clear()
        updates.install()
        runCurrent()
        assertEquals(UpdateStatus.Failed(UpdateProblem.FetchFailed(FetchProblem.ChecksumMismatch), build), updates.status())
        // And at the next start nothing is offered from it.
        updates().apply { restore() }.let { assertTrue(it.status() is UpdateStatus.Available) }
    }

    @Test
    fun anInstalledUpdateForgetsTheFile() = runTest {
        val (updates, _) = installing()
        updates.onInstallStatus(InstallStatus.Installed)
        assertTrue(updates.status() is UpdateStatus.UpToDate)
        assertNull(memory.ready)
    }

    @Test
    fun aStoppedDownloadJobSaysSoAndLeavesNothing() = runTest {
        slowApk = true
        val updates = updates()
        updates.check()
        val build = (updates.status() as UpdateStatus.Available).build
        updates.update()
        val job = launch(Dispatchers.IO) { updates.download(build) }
        withContext(Dispatchers.Default) {
            withTimeout(10_000) {
                while ((updates.status() as? UpdateStatus.Downloading)?.percent == null) delay(10)
            }
        }
        job.cancelAndJoin()
        assertEquals(UpdateStatus.Failed(UpdateProblem.FetchFailed(FetchProblem.DownloadStopped), build), updates.status())
        assertEquals(emptyList<String>(), fetcher.dir.list().orEmpty().toList())
        // And it can be started again.
        updates.update()
        assertTrue(updates.status() is UpdateStatus.Downloading)
    }

    @Test
    fun aReplacedDownloadJobNeitherReportsNorTouchesTheNewOne() = runTest {
        slowApk = true
        foreground = false
        val updates = updates()
        updates.check()
        val build = (updates.status() as UpdateStatus.Available).build
        updates.update()
        val first = launch(Dispatchers.IO) { updates.download(build) }
        withContext(Dispatchers.Default) {
            withTimeout(10_000) {
                while ((updates.status() as? UpdateStatus.Downloading)?.percent == null) delay(10)
            }
        }
        // WorkManager replaces the job (ExistingWorkPolicy.REPLACE): the new one starts while the old one unwinds.
        val second = async(Dispatchers.IO) { updates.download(build) }
        first.cancelAndJoin()
        assertFalse("the old job's stop is not reported", updates.status() is UpdateStatus.Failed)
        assertTrue(withContext(Dispatchers.Default) { withTimeout(20_000) { second.await() } })
        assertEquals(UpdateStatus.ReadyToInstall(build), updates.status())
        assertEquals(listOf("popcornstash-android-415.apk"), fetcher.dir.list().orEmpty().toList())
    }

    @Test
    fun noCheckWhileADownloadRuns() = runTest {
        val updates = updates()
        updates.check()
        updates.update()
        val before = releaseCount()
        updates.checkNow()
        runCurrent()
        assertNull(updates.check())
        assertEquals(before, releaseCount())
        assertTrue(updates.status() is UpdateStatus.Downloading)
    }

    // ------------------------------------------------------------- daily check ----

    @Test
    fun theDailyCheckNotifiesOncePerBuild() = runTest {
        val updates = updates()
        updates.backgroundCheck()
        updates.backgroundCheck()
        assertEquals(listOf(415), notifier.posted)
        assertEquals(415, memory.lastNotified)
    }

    @Test
    fun aBuildThatDisappearedDoesNotHoldBackTheNext() = runTest {
        // A mistaken android-999999999 was notified, then deleted from GitHub.
        memory.lastNotified = 999_999_999
        val updates = updates()
        updates.backgroundCheck()
        assertEquals(415, memory.lastNotified)
        releases = list(appRelease(416, server.url("/files/android-415").toString().trimEnd('/')), appRelease(415))
        updates.backgroundCheck()
        assertEquals(listOf(416), notifier.posted)
    }

    @Test
    fun theDailyCheckDoesNotNotifyWhenUpToDate() = runTest {
        updates(415).backgroundCheck()
        assertTrue(notifier.posted.isEmpty())
    }

    @Test
    fun notificationsAreAskedForOnceUnlessTheSwitchIsTurnedOn() = runTest {
        val updates = updates()
        assertTrue(updates.askForNotifications(turningOn = false))
        assertFalse(updates.askForNotifications(turningOn = false))
        assertTrue(memory.askedForNotifications)
        assertTrue(updates.askForNotifications(turningOn = true))
    }

    @Test
    fun turnedOffTheAppAsksGitHubNothing() = runTest {
        val updates = updates()
        updates.setAutoCheck(false)
        assertFalse(memory.autoCheck)
        assertFalse(updates.state.value.autoCheck)
        assertEquals(listOf(false), jobs.daily)
        updates.backgroundCheck()
        assertEquals(0, releaseCount())
        assertTrue(notifier.posted.isEmpty())

        updates.setAutoCheck(true)
        assertEquals(listOf(false, true), jobs.daily)
        updates.schedule()
        assertEquals(listOf(false, true, true), jobs.daily)
    }
}
