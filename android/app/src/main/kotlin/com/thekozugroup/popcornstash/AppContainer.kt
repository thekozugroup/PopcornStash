package com.thekozugroup.popcornstash

import android.content.Context
import android.os.Build
import android.provider.Settings
import androidx.core.content.edit
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
import com.thekozugroup.popcornstash.ui.media.imageCacheDir
import com.thekozugroup.popcornstash.ui.media.sessionImageLoader
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.job
import kotlinx.coroutines.launch
import okhttp3.HttpUrl
import java.io.File

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
        ),
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
