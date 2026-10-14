package com.thekozugroup.popcornstash

import android.app.Application
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import java.io.File

/** The process: holds the app's services (the session, the API client) for every screen. */
class PopcornStashApplication : Application() {
    lateinit var container: AppContainer
        private set

    override fun onCreate() {
        super.onCreate()
        container = AppContainer(this)
        // Reading the kept session touches the Keystore and a file: off the main thread.
        container.appScope.launch(Dispatchers.IO) { container.sessions.restore() }
        // Private copies of Other files opened before the process ended (docs/architecture.md section 10).
        container.appScope.launch(Dispatchers.IO) { File(cacheDir, "opened").deleteRecursively() }
        // Live events run only while the app is in the foreground.
        ProcessLifecycleOwner.get().lifecycle.addObserver(
            object : DefaultLifecycleObserver {
                override fun onStart(owner: LifecycleOwner) = container.sessions.setForeground(true)

                override fun onStop(owner: LifecycleOwner) = container.sessions.setForeground(false)
            },
        )
    }
}
