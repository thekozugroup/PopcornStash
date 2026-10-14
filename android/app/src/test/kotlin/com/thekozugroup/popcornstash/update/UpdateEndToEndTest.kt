package com.thekozugroup.popcornstash.update

import android.content.Context
import android.content.Intent
import android.content.pm.PackageInstaller
import android.content.pm.PackageManager
import android.content.pm.Signature
import android.content.pm.SigningInfo
import androidx.test.core.app.ApplicationProvider
import com.android.apksig.ApkVerifier
import com.thekozugroup.popcornstash.PopcornStashApplication
import com.thekozugroup.popcornstash.data.api.baseHttpClient
import com.thekozugroup.popcornstash.update.UpdateFixtures.appRelease
import com.thekozugroup.popcornstash.update.UpdateFixtures.list
import com.thekozugroup.popcornstash.update.UpdateFixtures.serverRelease
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.runBlocking
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
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.shadows.ShadowLooper
import java.io.File
import java.security.MessageDigest
import java.util.concurrent.CopyOnWriteArrayList

/**
 * The whole update on Robolectric, as on a phone up to Android's own install: GitHub's release
 * list and the files are a MockWebServer serving a real APK (`resources/update/build-415.apk`, a
 * manifest-only build 415 signed with a throwaway key), the installed app is signed with that key,
 * and the update goes check, permission, download, checksum, PackageManager's reading of the APK,
 * signature, and a PackageInstaller session that is written and committed.
 *
 * Robolectric's PackageManager reads an APK's package and versionCode but not its certificates
 * (its copy of Android's APK signature verifier cannot reach the JDK's memory-mapped buffers), so
 * the test reads those with apksig, the library apksigner is built on; on a phone
 * [PackageManagerApks] reads all three with getPackageArchiveInfo(GET_SIGNING_CERTIFICATES, and
 * GET_SIGNATURES as well on Android 9 and 10, which read the certificates only then).
 */
@RunWith(RobolectricTestRunner::class)
class UpdateEndToEndTest {
    private val context = ApplicationProvider.getApplicationContext<Context>()
    private val server = MockWebServer()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private var apkBytes = fixture("build-415.apk")
    private val requests = CopyOnWriteArrayList<String>()

    private fun fixture(name: String): ByteArray =
        checkNotNull(javaClass.classLoader?.getResourceAsStream("update/$name")) { "no fixture $name" }.use { it.readBytes() }

    /** The certificates apksig finds in [apk], as SHA-256 hex, and the first one's encoding. */
    private fun certificates(apk: File): Pair<Set<String>, ByteArray> {
        val result = ApkVerifier.Builder(apk).build().verify()
        assertTrue("apksig verifies ${apk.name}: ${result.errors}", result.isVerified)
        val certs = result.signerCertificates
        return certs.map { MessageDigest.getInstance("SHA-256").digest(it.encoded).toHex() }.toSet() to certs.first().encoded
    }

    /** The installed app, signed with the certificate of [apk]. */
    private fun installSignedLike(apkName: String) {
        val file = File.createTempFile("signer", ".apk").apply { writeBytes(fixture(apkName)) }
        val (_, cert) = certificates(file)
        file.delete()
        val signing = SigningInfo()
        shadowOf(signing).setSignatures(arrayOf(Signature(cert)))
        shadowOf(context.packageManager).getInternalMutablePackageInfo(context.packageName).signingInfo = signing
    }

    /** [PackageManagerApks.inspect] as Robolectric can do it: PackageManager's package and build, apksig's certificates. */
    private val inspector = ApkInspector { apk ->
        val info = context.packageManager.getPackageArchiveInfo(apk.path, 0) ?: return@ApkInspector null
        ApkFacts(info.packageName, info.longVersionCode, certificates(apk).first)
    }

    private lateinit var memory: InMemoryUpdateMemory
    private lateinit var updates: AppUpdates
    private val downloads = CopyOnWriteArrayList<AppBuild>()

    @Before
    fun setUp() {
        server.dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse {
                requests += request.url.encodedPath
                val base = server.url("/download/android-415").toString().trimEnd('/')
                return when (request.url.encodedPath) {
                    "/repos/thekozugroup/PopcornStash/releases" ->
                        MockResponse.Builder()
                            .body(list(appRelease(416, draft = true), serverRelease("0.2.0"), appRelease(415, base), appRelease(1)))
                            .addHeader("ETag", "\"releases-1\"")
                            .build()
                    "/download/android-415/popcornstash-android-415.apk" -> MockResponse.Builder().body(Buffer().write(apkBytes)).build()
                    "/download/android-415/popcornstash-android-415.apk.sha256" -> {
                        val sha = MessageDigest.getInstance("SHA-256").digest(fixture("build-415.apk")).toHex()
                        MockResponse.Builder().body("$sha  popcornstash-android-415.apk\n").build()
                    }
                    else -> MockResponse.Builder().code(404).build()
                }
            }
        }
        server.start()
        installSignedLike("build-415.apk")
        memory = InMemoryUpdateMemory()
        val http = baseHttpClient("PopcornStash-Android/0.1.0+1")
        val apks = PackageManagerApks(context)
        updates = AppUpdates(
            versionName = "0.1.0+1",
            installedBuild = 1,
            memory = memory,
            checker = UpdateChecker(http, memory, server.url("/repos/thekozugroup/PopcornStash/releases?per_page=30")),
            fetcher = UpdateFetcher(http, File(context.cacheDir, "updates"), context.packageName, inspector, apks::installedSigners),
            installer = PackageInstallerUpdates(context),
            jobs = object : UpdateJobs {
                override fun scheduleDailyCheck(enabled: Boolean) = Unit

                override fun startDownload(build: AppBuild) {
                    downloads += build
                }
            },
            notifier = object : UpdateNotifier {
                override fun newBuild(build: AppBuild) = Unit

                override fun cancel() = Unit
            },
            scope = scope,
            installedSigners = apks::installedSigners,
            releaseSigner = "7721ec2f9c281a3bde0e53633e947fa6e89e31ef53cb9816e4dda8ef1224fda4",
        )
    }

    @After
    fun tearDown() {
        scope.cancel()
        server.close()
        File(context.cacheDir, "updates").deleteRecursively()
    }

    private fun status() = updates.state.value.status

    private fun waitFor(what: String, condition: () -> Boolean) {
        val until = System.currentTimeMillis() + 10_000
        while (!condition()) {
            ShadowLooper.idleMainLooper()
            check(System.currentTimeMillis() < until) { "Timed out waiting for $what; the updater is at ${status()}" }
            Thread.sleep(20)
        }
    }

    @Test
    fun checkDownloadVerifyAndInstall() = runBlocking {
        // The installed app is signed with the release key, as a build from GitHub is.
        assertEquals(setOf("7721ec2f9c281a3bde0e53633e947fa6e89e31ef53cb9816e4dda8ef1224fda4"), PackageManagerApks(context).installedSigners())

        // Check: the newest app build that is not a draft, from the public list, without a login.
        val build = updates.check()
        assertEquals(415, build?.number)
        assertEquals(UpdateStatus.Available(build!!), status())
        assertEquals(listOf("/repos/thekozugroup/PopcornStash/releases"), requests)

        // Update: Android first wants the person to let PopcornStash install apps.
        shadowOf(context.packageManager).setCanRequestPackageInstalls(false)
        assertFalse(updates.update())
        assertEquals(UpdateStatus.NeedsPermission(build), status())
        val permission = updates.permissionIntent()
        assertEquals(android.provider.Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, permission.action)
        assertEquals("package:${context.packageName}", permission.dataString)
        shadowOf(context.packageManager).setCanRequestPackageInstalls(true)
        updates.onResume()
        assertEquals(listOf(build), downloads)

        // The download job: the file, its checksum, PackageManager's reading and the signature.
        val sessionsBefore = context.packageManager.packageInstaller.allSessions.size
        val finished = CopyOnWriteArrayList<Boolean>()
        context.packageManager.packageInstaller.registerSessionCallback(
            object : PackageInstaller.SessionCallback() {
                override fun onCreated(sessionId: Int) = Unit

                override fun onBadgingChanged(sessionId: Int) = Unit

                override fun onActiveChanged(sessionId: Int, active: Boolean) = Unit

                override fun onProgressChanged(sessionId: Int, progress: Float) = Unit

                override fun onFinished(sessionId: Int, success: Boolean) {
                    finished += success
                }
            },
        )
        assertTrue(updates.download(build))
        assertEquals(
            listOf(
                "/repos/thekozugroup/PopcornStash/releases",
                "/download/android-415/popcornstash-android-415.apk.sha256",
                "/download/android-415/popcornstash-android-415.apk",
            ),
            requests,
        )
        val ready = checkNotNull(memory.ready)
        assertEquals(415, ready.build.number)
        val file = File(context.cacheDir, "updates/${ready.fileName}")
        assertTrue(file.readBytes().contentEquals(fixture("build-415.apk")))

        // The person is here, so it goes straight to Android: a session is written and committed.
        assertEquals(UpdateStatus.Installing(build), status())
        waitFor("the session to be committed") { finished.isNotEmpty() }
        assertEquals(listOf(true), finished)
        assertTrue(context.packageManager.packageInstaller.allSessions.size >= sessionsBefore)
    }

    @Test
    fun aBuildSignedWithAnotherKeyIsRefusedAndDeleted() = runBlocking {
        apkBytes = fixture("build-415-other-key.apk")
        val build = updates.check()!!
        assertFalse(updates.download(build))
        // The checksum is the real build's, so the swapped file fails there first.
        assertEquals(UpdateStatus.Failed(UpdateProblem.FetchFailed(FetchProblem.ChecksumMismatch), build), status())
        assertNull(memory.ready)
        assertEquals(emptyList<String>(), File(context.cacheDir, "updates").list().orEmpty().toList())
    }

    @Test
    fun aGenuineBuildFromAnotherKeyIsRefusedAtTheSignature() = runBlocking {
        // Both files agree, but the APK is not signed with the installed app's key.
        apkBytes = fixture("build-415-other-key.apk")
        val sha = MessageDigest.getInstance("SHA-256").digest(apkBytes).toHex()
        server.dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse = when (request.url.encodedPath) {
                "/a.apk" -> MockResponse.Builder().body(Buffer().write(apkBytes)).build()
                "/a.apk.sha256" -> MockResponse.Builder().body("$sha  a.apk").build()
                else -> MockResponse.Builder().code(404).build()
            }
        }
        val build = AppBuild(415, server.url("/a.apk").toString(), server.url("/a.apk.sha256").toString(), "https://github.com/x", 0)
        assertFalse(updates.download(build))
        // The installed app is release-signed: a build from another key is refused, never sent to the release page.
        assertEquals(UpdateStatus.Failed(UpdateProblem.FetchFailed(FetchProblem.OtherSigner), build), status())
        assertEquals(emptyList<String>(), File(context.cacheDir, "updates").list().orEmpty().toList())
    }

    @Test
    fun aDebugSignedCopyIsToldBeforeItDownloads() = runBlocking {
        installSignedLike("build-415-other-key.apk")
        updates.check()
        assertTrue(status() is UpdateStatus.OtherKey)
        assertEquals(listOf("/repos/thekozugroup/PopcornStash/releases"), requests)
    }

    @Test
    fun theInstallReceiverOpensAndroidsConfirmationAndReportsTheEnd() {
        val app = context as PopcornStashApplication
        val confirm = Intent("android.content.pm.action.CONFIRM_INSTALL").setPackage("com.android.packageinstaller")
        UpdateInstallReceiver().onReceive(
            context,
            Intent().putExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_PENDING_USER_ACTION).putExtra(Intent.EXTRA_INTENT, confirm),
        )
        val started = shadowOf(app).nextStartedActivity
        assertEquals("android.content.pm.action.CONFIRM_INSTALL", started.action)
        assertTrue(started.flags and Intent.FLAG_ACTIVITY_NEW_TASK != 0)
        // Kept, so the page can open it again if the person leaves it.
        assertEquals("android.content.pm.action.CONFIRM_INSTALL", app.container.updates.confirmation()?.action)

        UpdateInstallReceiver().onReceive(
            context,
            Intent().putExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE_ABORTED),
        )
        assertEquals(UpdateStatus.Failed(UpdateProblem.InstallCancelled, null), app.container.updates.state.value.status)
        assertNull(app.container.updates.confirmation())

        // Android's own message stays out of what the page says.
        UpdateInstallReceiver().onReceive(
            context,
            Intent()
                .putExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE_STORAGE)
                .putExtra(PackageInstaller.EXTRA_STATUS_MESSAGE, "INSTALL_FAILED_INSUFFICIENT_STORAGE: Failed to allocate"),
        )
        assertEquals(
            UpdateStatus.Failed(UpdateProblem.InstallFailed(InstallFailure.Storage), null),
            app.container.updates.state.value.status,
        )
    }

    @Test
    fun packageInstallersAnswersAreReadAsPlainReasons() {
        assertEquals(InstallStatus.Installed, installStatusOf(PackageInstaller.STATUS_SUCCESS))
        assertEquals(InstallStatus.Cancelled, installStatusOf(PackageInstaller.STATUS_FAILURE_ABORTED))
        assertEquals(InstallStatus.Failed(InstallFailure.Storage), installStatusOf(PackageInstaller.STATUS_FAILURE_STORAGE))
        assertEquals(InstallStatus.Failed(InstallFailure.Incompatible), installStatusOf(PackageInstaller.STATUS_FAILURE_INCOMPATIBLE))
        assertEquals(InstallStatus.Failed(InstallFailure.Conflict), installStatusOf(PackageInstaller.STATUS_FAILURE_CONFLICT))
        assertEquals(InstallStatus.Failed(InstallFailure.Blocked), installStatusOf(PackageInstaller.STATUS_FAILURE_BLOCKED))
        assertEquals(InstallStatus.Failed(InstallFailure.Invalid), installStatusOf(PackageInstaller.STATUS_FAILURE_INVALID))
        assertEquals(InstallStatus.Failed(InstallFailure.Other), installStatusOf(PackageInstaller.STATUS_FAILURE))
        assertEquals(InstallStatus.Failed(InstallFailure.Other), installStatusOf(PackageInstaller.STATUS_FAILURE_TIMEOUT))
    }

    @Test
    fun theManifestDeclaresWhatInstallingNeeds() {
        val info = context.packageManager.getPackageInfo(context.packageName, PackageManager.GET_PERMISSIONS or PackageManager.GET_RECEIVERS)
        assertTrue(info.requestedPermissions.orEmpty().contains("android.permission.REQUEST_INSTALL_PACKAGES"))
        // Without it Android 12 and later ignore USER_ACTION_NOT_REQUIRED and ask every time.
        assertTrue(info.requestedPermissions.orEmpty().contains("android.permission.UPDATE_PACKAGES_WITHOUT_USER_ACTION"))
        val receiver = info.receivers.orEmpty().single { it.name == UpdateInstallReceiver::class.java.name }
        assertFalse("only the app's own PendingIntent may reach it", receiver.exported)
    }
}
