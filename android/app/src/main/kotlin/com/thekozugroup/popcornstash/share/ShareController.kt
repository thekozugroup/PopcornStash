package com.thekozugroup.popcornstash.share

import android.net.Uri
import com.thekozugroup.popcornstash.data.api.ApiException
import com.thekozugroup.popcornstash.data.auth.ServerAddress
import com.thekozugroup.popcornstash.data.session.Session
import com.thekozugroup.popcornstash.data.session.SessionState
import com.thekozugroup.popcornstash.feature.downloads.DetectedLink
import com.thekozugroup.popcornstash.feature.downloads.isNoVpnMode
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** What the share sheet shows. */
sealed interface ShareSheetState {
    /** Reading the kept session or the shared files. */
    data object Checking : ShareSheetState

    /** "Sign in to PopcornStash first." */
    data object SignedOut : ShareSheetState

    data class Link(
        val link: DetectedLink,
        /** False when the server runs torrents without a VPN (the sentence changes). */
        val viaVpn: Boolean = true,
        val busy: Boolean = false,
        val error: String? = null,
        /** The download it became, once added. */
        val addedName: String? = null,
        val libraries: List<com.thekozugroup.popcornstash.data.api.Library> = emptyList(),
        val libraryId: String? = null,
    ) : ShareSheetState

    data class Files(
        val files: List<SharedFile>,
        val refused: Int,
        /** "Uploads as Ana to stash.example.com." */
        val userName: String,
        val server: String,
        val libraries: List<com.thekozugroup.popcornstash.data.api.Library> = emptyList(),
        val libraryId: String? = null,
        val busy: Boolean = false,
    ) : ShareSheetState

    data class Unusable(val refused: Int) : ShareSheetState
}

/**
 * The share sheet (android/DESIGN.md, Share sheet): a link goes to Downloads after the
 * person confirms; files are uploaded by a WorkManager job after the signed-in person
 * confirms ([upload]), for that person only.
 */
class ShareController(
    private val request: ShareRequest,
    private val sessions: StateFlow<SessionState>,
    /** Reads names, sizes and dates of the shared files (off the main thread). */
    private val describe: suspend (List<Uri>) -> List<SharedFile>,
    /** Keeps the sharing app's read grants past this screen, then starts the job. */
    private val enqueue: (UploadBatch) -> Unit,
    private val scope: CoroutineScope,
    private val now: () -> Long = { System.currentTimeMillis() / 1000 },
    private val loadDestinationLibraries: suspend (Session) -> List<com.thekozugroup.popcornstash.data.api.Library> = { it.api.libraries() },
) {
    private val _state = MutableStateFlow<ShareSheetState>(ShareSheetState.Checking)
    val state: StateFlow<ShareSheetState> = _state.asStateFlow()

    private var session: Session? = null

    init {
        scope.launch {
            sessions.collect { current ->
                when (current) {
                    SessionState.Restoring -> Unit
                    is SessionState.SignedOut -> {
                        session = null
                        _state.value = ShareSheetState.SignedOut
                    }
                    is SessionState.SignedIn -> if (session?.id != current.session.id) {
                        session = current.session
                        show(current.session)
                    }
                }
            }
        }
    }

    private suspend fun show(session: Session) {
        when (request) {
            is ShareRequest.Link -> {
                _state.value = ShareSheetState.Link(request.link)
                try {
                    val vpn = session.api.vpnStatus()
                    _state.update { (it as? ShareSheetState.Link)?.copy(viaVpn = !isNoVpnMode(vpn)) ?: it }
                } catch (_: ApiException) {
                    // The sentence keeps "only through the VPN", which is what happens unless the server says otherwise.
                }
            }
            is ShareRequest.Files -> {
                _state.value = ShareSheetState.Checking
                val files = describe(request.uris)
                _state.value = ShareSheetState.Files(
                    files = files,
                    refused = request.refused,
                    userName = session.user.value.displayName,
                    server = ServerAddress.display(session.server),
                )
            }
            is ShareRequest.Unusable -> _state.value = ShareSheetState.Unusable(request.refused)
        }
        if (request !is ShareRequest.Unusable) loadLibraries()
    }

    private suspend fun loadLibraries() {
        val current = session ?: return
        try {
            val libraries = loadDestinationLibraries(current)
            _state.update { when (it) {
                is ShareSheetState.Link -> it.copy(libraries = libraries)
                is ShareSheetState.Files -> it.copy(libraries = libraries)
                else -> it
            } }
        } catch (_: ApiException) { /* Automatic sorting remains available. */ }
    }

    fun chooseLibrary(id: String?) {
        _state.update { when (it) {
            is ShareSheetState.Link -> if (!it.busy) it.copy(libraryId = id) else it
            is ShareSheetState.Files -> if (!it.busy) it.copy(libraryId = id) else it
            else -> it
        } }
    }

    /** Adds the shared link to Downloads. */
    fun addLink() {
        val current = _state.value as? ShareSheetState.Link ?: return
        val api = session?.api ?: return
        if (current.busy || current.addedName != null) return
        _state.value = current.copy(busy = true, error = null)
        scope.launch {
            _state.value = try {
                val download = api.addDownload(current.link.url, libraryId = current.libraryId)
                current.copy(busy = false, addedName = download.name)
            } catch (e: ApiException) {
                val message = if (e.code == "vpn_unavailable") {
                    "Torrents are turned off on this server. Web links and uploads still work."
                } else {
                    e.message
                }
                current.copy(busy = false, error = message)
            }
        }
    }

    /** Starts uploading the shared files as the signed-in person; true when the job was started. */
    fun upload(): Boolean {
        val current = _state.value as? ShareSheetState.Files ?: return false
        val signedIn = session ?: return false
        if (current.busy || current.files.isEmpty()) return false
        _state.value = current.copy(busy = true)
        val user = signedIn.user.value
        enqueue(
            UploadBatch(
                id = UploadQueue.newBatchId(),
                userId = user.id,
                userName = user.displayName,
                server = signedIn.server.toString(),
                files = current.files.map { it.copy(libraryId = current.libraryId) },
                createdAt = now(),
            ),
        )
        return true
    }
}
