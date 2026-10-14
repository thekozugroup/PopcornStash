package com.thekozugroup.popcornstash

import android.content.Context
import android.os.Build
import android.provider.Settings
import androidx.core.content.edit
import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.baseHttpClient
import com.thekozugroup.popcornstash.data.auth.KeystoreCipher
import com.thekozugroup.popcornstash.data.auth.SessionStore
import com.thekozugroup.popcornstash.data.session.ServerMemory
import com.thekozugroup.popcornstash.data.session.SessionRepository
import com.thekozugroup.popcornstash.feature.signin.SignInDependencies
import com.thekozugroup.popcornstash.share.UploadBatchStore
import com.thekozugroup.popcornstash.share.UploadQueue
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
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
            // Share sheet batches, their jobs and notifications.
            { uploads.forget() },
        ),
        beforeSignOut = { session -> uploads.stopAll(session.api) },
    )

    fun signInDependencies() = SignInDependencies(
        publicApi = { server: HttpUrl -> ApiClient(server, http) },
        finishSignIn = { server, token -> sessions.signIn(server, token) },
        deviceName = deviceName,
        packageName = app.packageName,
        allowLocalHttp = BuildConfig.DEBUG,
    )
}

private class PreferencesServerMemory(context: Context) : ServerMemory {
    private val preferences = context.getSharedPreferences("sign-in", Context.MODE_PRIVATE)

    override var lastServer: String?
        get() = preferences.getString("last_server", null)
        set(value) = preferences.edit { putString("last_server", value) }
}
