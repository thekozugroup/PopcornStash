package com.thekozugroup.popcornstash.share

import android.Manifest
import android.annotation.SuppressLint
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.getValue
import androidx.core.content.ContextCompat
import androidx.core.net.toUri
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import com.thekozugroup.popcornstash.PopcornStashApplication
import com.thekozugroup.popcornstash.navigation.OpenTab
import com.thekozugroup.popcornstash.navigation.Tab
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * The share sheet (docs/architecture.md section 10, Share sheet): `ACTION_SEND` and
 * `ACTION_SEND_MULTIPLE` from any app. A link goes to Downloads after a confirm; photos,
 * videos and files (`content://` only) are uploaded by a WorkManager job after the
 * signed-in person confirms.
 */
class ShareActivity : ComponentActivity() {
    // The check is about FragmentActivity before Fragment 1.3.0. This is a ComponentActivity,
    // and the app runs with Fragment 1.5.7; only the credentials library compiles against 1.2.5.
    @SuppressLint("InvalidFragmentVersionForActivityResult")
    private val askNotifications = registerForActivityResult(ActivityResultContracts.RequestPermission()) {
        // Allowed or not, the upload runs; without the permission it just stays quiet.
        startUpload()
    }

    private lateinit var model: ShareViewModel

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        val container = (application as PopcornStashApplication).container
        val request = parseShare(intent) { uri -> isShareableUri(this, uri) }
        model = ViewModelProvider(
            this,
            viewModelFactory {
                initializer {
                    ShareViewModel { scope ->
                        ShareController(
                            request = request,
                            sessions = container.sessions.state,
                            describe = { uris -> describeAll(uris) },
                            enqueue = { batch ->
                                keepReadGrants(batch)
                                container.uploads.enqueue(batch)
                            },
                            scope = scope,
                        )
                    }
                }
            },
        )[ShareViewModel::class.java]
        setContent {
            PopcornTheme {
                val state by model.controller.state.collectAsStateWithLifecycle()
                ShareSheet(
                    state = state,
                    actions = ShareActions(
                        onAddLink = model.controller::addLink,
                        onLibrary = model.controller::chooseLibrary,
                        onUpload = ::confirmUpload,
                        onOpenApp = { open(Tab.Home) },
                        onOpenDownloads = { open(Tab.Downloads) },
                        onClose = ::finish,
                    ),
                )
            }
        }
    }

    private fun confirmUpload() {
        val needsAsking = Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        if (needsAsking) askNotifications.launch(Manifest.permission.POST_NOTIFICATIONS) else startUpload()
    }

    private fun startUpload() {
        if (model.controller.upload()) finish()
    }

    /**
     * The sharing app's read grant ends with this screen; the upload job runs after it. The
     * app keeps its own grant for each file until the job ends ([UploadQueue.release]).
     */
    private fun keepReadGrants(batch: UploadBatch) {
        batch.files.forEach { file ->
            runCatching { grantUriPermission(packageName, file.uri.toUri(), Intent.FLAG_GRANT_READ_URI_PERMISSION) }
        }
    }

    private suspend fun describeAll(uris: List<Uri>): List<SharedFile> = withContext(Dispatchers.IO) {
        uris.mapIndexed { index, uri -> describeSharedFile(contentResolver, uri, "Shared file ${index + 1}") }
    }

    private fun open(tab: Tab) {
        startActivity(OpenTab.intent(this, tab))
        finish()
    }
}

/** Keeps the share sheet's controller (and a link being added) through a rotation. */
class ShareViewModel(make: (kotlinx.coroutines.CoroutineScope) -> ShareController) : ViewModel() {
    val controller: ShareController = make(viewModelScope)
}
