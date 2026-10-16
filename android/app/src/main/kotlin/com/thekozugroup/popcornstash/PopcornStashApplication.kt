package com.thekozugroup.popcornstash

import android.app.Application
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.Build
import com.thekozugroup.popcornstash.privacy.AppLock
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import java.io.File

/** The process: holds the app's services (the session, the API client) for every screen. */
class PopcornStashApplication : Application() {
    val appLock = AppLock()

    lateinit var container: AppContainer
        private set

    override fun onCreate() {
        super.onCreate()
        val screenOff = object : BroadcastReceiver() {
            override fun onReceive(context: Context, intent: Intent) {
                if (intent.action == Intent.ACTION_SCREEN_OFF) appLock.lock()
            }
        }
        val filter = IntentFilter(Intent.ACTION_SCREEN_OFF)
        if (Build.VERSION.SDK_INT >= 33) registerReceiver(screenOff, filter, Context.RECEIVER_NOT_EXPORTED)
        else registerReceiver(screenOff, filter) // SCREEN_OFF is a protected system-only broadcast.
        container = AppContainer(this)
        // Reading the kept session touches the Keystore and a file: off the main thread.
        container.appScope.launch(Dispatchers.IO) { container.sessions.restore() }
        // Private copies of Other files opened before the process ended (docs/architecture.md section 10).
        container.appScope.launch(Dispatchers.IO) { File(cacheDir, "opened").deleteRecursively() }
        // The updater: a verified download waiting to be installed, and the daily check (on unless turned off).
        container.appScope.launch(Dispatchers.IO) { container.updates.restore() }
        try {
            container.updates.schedule()
        } catch (_: IllegalStateException) {
            // WorkManager starts with the app (androidx.startup); only a test process may lack it.
        }
        // Live events run only while the app is in the foreground.
        ProcessLifecycleOwner.get().lifecycle.addObserver(
            object : DefaultLifecycleObserver {
                override fun onStart(owner: LifecycleOwner) = container.sessions.setForeground(true)

                override fun onStop(owner: LifecycleOwner) = container.sessions.setForeground(false)
            },
        )
    }
}
