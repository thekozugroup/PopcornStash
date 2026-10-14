package com.thekozugroup.popcornstash

import android.content.Context
import android.os.Build
import android.provider.Settings
import androidx.core.content.edit
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.ProcessLifecycleOwner
import coil3.ImageLoader
import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.baseHttpClient
import com.thekozugroup.popcornstash.data.auth.KeystoreCipher
import com.thekozugroup.popcornstash.data.auth.SessionStore
import com.thekozugroup.popcornstash.data.session.ServerMemory
import com.thekozugroup.popcornstash.data.session.Session
import com.thekozugroup.popcornstash.data.session.SessionRepository
import com.thekozugroup.popcornstash.feature.signin.SignInDependencies
import com.thekozugroup.popcornstash.navigation.ViewModelStores
import com.thekozugroup.popcornstash.share.UploadBatchStore
import com.thekozugroup.popcornstash.share.UploadQueue
import com.thekozugroup.popcornstash.ui.media.imageCacheDir
import com.thekozugroup.popcornstash.ui.media.sessionImageLoader
import com.thekozugroup.popcornstash.update.AppUpdates
import com.thekozugroup.popcornstash.update.PackageInstallerUpdates
import com.thekozugroup.popcornstash.update.PackageManagerApks
import com.thekozugroup.popcornstash.update.PreferencesUpdateMemory
import com.thekozugroup.popcornstash.update.UpdateChecker
import com.thekozugroup.popcornstash.update.UpdateFetcher
import com.thekozugroup.popcornstash.update.UpdateNotifications
import com.thekozugroup.popcornstash.update.WorkManagerUpdateJobs
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.job
import kotlinx.coroutines.launch
import okhttp3.HttpUrl
import java.io.File
import java.util.concurrent.TimeUnit

/** The app's services, made once per process by [PopcornStashApplication]. */
class AppContainer(context: Context) {
    private val app = context.applicationContext

    val userAgent = "PopcornStash-Android/${BuildConfig.VERSION_NAME} (${Build.MODEL}; Android ${Build.VERSION.RELEASE})"

    val http = baseHttpClient(userAgent)

    val appScope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)

    /** How this phone introduces itself when pairing and in the person's list of sessions. */
    val deviceName: String =
        runCatching { Settings.Global.getString(app.contentResolver, Settings.Global.DEVICE_NAME) }.getOrNull()
            ?.takeIf { it.isNotBlank() } ?: Build.MODEL

    /** The share sheet's uploads; they belong to the person signed in and end with the session. */
    val uploads = UploadQueue(app, UploadBatchStore(UploadQueue.storeDir(app)), appScope)

    val sessions = SessionRepository(
        store = SessionStore(app.noBackupFilesDir, KeystoreCipher()),
        memory = PreferencesServerMemory(app),
        baseClient = http,
        appScope = appScope,
        sessionContext = Dispatchers.Main.immediate,
        cleaners = listOf(
            // Private copies of opened Other files (docs/architecture.md section 10).
            { File(app.cacheDir, "opened").deleteRecursively() },
            // The pictures the person looked at (Coil's disk cache).
            { imageCacheDir(app).deleteRecursively() },
            // Share sheet batches, their jobs and notifications.
            { uploads.forget() },
        ),
        beforeSignOut = { session -> uploads.stopAll(session.api) },
    )

    /**
     * The updater's own client: GitHub sees the app's name and version and nothing about the
     * phone or the person, and a download may follow GitHub's redirect to its file host (https only).
     */
    private val updateHttp = baseHttpClient("PopcornStash-Android/${BuildConfig.VERSION_NAME}").newBuilder()
        .followRedirects(true)
        .followSslRedirects(false)
        .readTimeout(60, TimeUnit.SECONDS)
        .build()

    private val apks = PackageManagerApks(app)
    private val updateMemory = PreferencesUpdateMemory(app)

    /** Android's install sessions for updates; [com.thekozugroup.popcornstash.update.UpdateInstallReceiver] keeps Android's confirmation in it. */
    val updateInstaller = PackageInstallerUpdates(app)

    /** Updates of the app itself from GitHub releases (docs/android.md, Updates); they do not depend on who is signed in. */
    val updates = AppUpdates(
        versionName = BuildConfig.VERSION_NAME,
        installedBuild = BuildConfig.VERSION_CODE,
        memory = updateMemory,
        checker = UpdateChecker(updateHttp, updateMemory),
        fetcher = UpdateFetcher(updateHttp, File(app.cacheDir, "updates"), app.packageName, apks, apks::installedSigners),
        installer = updateInstaller,
        jobs = WorkManagerUpdateJobs(app),
        notifier = UpdateNotifications(app),
        scope = appScope,
        installedSigners = apks::installedSigners,
        releaseSigner = BuildConfig.RELEASE_CERT_SHA256.ifEmpty { null },
        // A debug build is signed with its builder's debug key, never the release key.
        debugBuild = BuildConfig.DEBUG,
        inForeground = { ProcessLifecycleOwner.get().lifecycle.currentState.isAtLeast(Lifecycle.State.STARTED) },
    )

    private var sessionUi: SessionUi? = null

    /**
     * What the signed-in screens keep for [session] beyond one screen: the view models of the
     * pages in every tab and the image loader. Made once per session, so they survive a
     * rotation, and dropped with their memory caches as soon as the session ends.
     */
    @Synchronized
    fun sessionUi(session: Session): SessionUi {
        sessionUi?.takeIf { it.sessionId == session.id }?.let { return it }
        sessionUi?.close()
        val ui = SessionUi(session.id, ViewModelStores(), sessionImageLoader(app, session.http))
        sessionUi = ui
        session.scope.coroutineContext.job.invokeOnCompletion {
            appScope.launch { closeSessionUi(session.id) }
        }
        return ui
    }

    @Synchronized
    private fun closeSessionUi(sessionId: Long) {
        val ui = sessionUi?.takeIf { it.sessionId == sessionId } ?: return
        sessionUi = null
        ui.close()
    }

    fun signInDependencies() = SignInDependencies(
        publicApi = { server: HttpUrl -> ApiClient(server, http) },
        finishSignIn = { server, token -> sessions.signIn(server, token) },
        deviceName = deviceName,
        packageName = app.packageName,
        allowLocalHttp = BuildConfig.DEBUG,
    )
}

/** One session's page view models and image loader (see [AppContainer.sessionUi]). */
class SessionUi(val sessionId: Long, val pages: ViewModelStores, val images: ImageLoader) {
    fun close() {
        pages.clearAll()
        images.memoryCache?.clear()
        images.shutdown()
    }
}

private class PreferencesServerMemory(context: Context) : ServerMemory {
    private val preferences = context.getSharedPreferences("sign-in", Context.MODE_PRIVATE)

    override var lastServer: String?
        get() = preferences.getString("last_server", null)
        set(value) = preferences.edit { putString("last_server", value) }
}
