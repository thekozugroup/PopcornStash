package com.thekozugroup.popcornstash.feature.downloads

import android.content.ClipDescription
import android.content.Context
import android.os.Build
import androidx.compose.material3.SnackbarHostState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.platform.ClipEntry
import androidx.compose.ui.platform.LocalClipboard
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.thekozugroup.popcornstash.data.session.Session
import com.thekozugroup.popcornstash.navigation.TabPage
import com.thekozugroup.popcornstash.share.UploadBatchProgress
import com.thekozugroup.popcornstash.share.UploadQueue
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.job
import kotlinx.coroutines.launch

/**
 * Downloads, the left tab: what is downloading, the files ready to import into a library,
 * and where finished downloads went (android/DESIGN.md, Downloads).
 *
 * [onOpenPlace] opens a library or Other files from a finished row; without it the row
 * opens that page of the web app in a Custom Tab.
 */
@Composable
fun DownloadsTab(
    page: TabPage,
    session: Session,
    uploads: UploadQueue,
    openUrl: (String) -> Unit,
    onOpenPlace: ((OutcomePlace) -> Unit)? = null,
) {
    val held = remember(session) { DownloadsHolders.forSession(session, uploads.progress) { id -> session.scope.launch { uploads.stop(id, session.api) } } }
    val controller = held.controller
    val state by controller.state.collectAsStateWithLifecycle()
    val snackbar = remember { SnackbarHostState() }
    val scope = rememberCoroutineScope()
    val clipboard = LocalClipboard.current
    val context = LocalContext.current
    LaunchedEffect(controller) { controller.messages.collect { snackbar.showSnackbar(it) } }
    val now by produceState(System.currentTimeMillis() / 1000) {
        // "Finished 3 min ago" stays true while the page is open.
        while (true) {
            delay(30_000)
            value = System.currentTimeMillis() / 1000
        }
    }
    val web: (String) -> String = { path -> session.api.url(path).toString() }
    val actions = remember(controller) {
        DownloadsActions(
            onRefresh = controller::refreshNow,
            onOpenVpnSettings = { openUrl(web("/settings/vpn")) },
            onPauseOrResume = controller::pauseOrResume,
            onAskRemove = controller::askRemove,
            onConfirmRemove = controller::confirmRemove,
            onDismissRemove = controller::dismissRemove,
            onRetryDownload = controller::retry,
            onShowMoreFinished = controller::showMoreFinished,
            onStopUpload = controller::stopUpload,
            onOpenAdd = { scope.launch { controller.openAdd(clipboardText(clipboard.getClipEntry(), context)) } },
            onAddText = controller::onAddText,
            onSubmitAdd = controller::submitAdd,
            onCloseAdd = controller::closeAdd,
            onChoose = controller::choose,
            onAccept = controller::accept,
            onEdit = controller::openEdit,
            onIgnore = controller::ignore,
            onAskAcceptAll = controller::askAcceptAll,
            onConfirmAcceptAll = controller::confirmAcceptAll,
            onDismissAcceptAll = controller::dismissAcceptAll,
            onEditForm = controller::onEditForm,
            onSubmitEdit = controller::submitEdit,
            onCloseEdit = controller::closeEdit,
            onOpenPlace = onOpenPlace ?: { place ->
                openUrl(web(place.libraryId?.let { "/libraries/${java.net.URLEncoder.encode(it, "UTF-8")}" } ?: "/other"))
            },
        )
    }
    DownloadsPage(page = page, state = state, actions = actions, nowSeconds = now, snackbar = snackbar)
}

/**
 * The clipboard's text when it may hold a link. A clip the copying app marked sensitive (a
 * password) is never read.
 */
private fun clipboardText(entry: ClipEntry?, context: Context): CharSequence? {
    val clip = entry?.clipData ?: return null
    if (clip.itemCount == 0) return null
    val description = clip.description
    val sensitive = Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
        description.extras?.getBoolean(ClipDescription.EXTRA_IS_SENSITIVE) == true
    if (sensitive) return null
    return runCatching { clip.getItemAt(0).coerceToText(context) }.getOrNull()
}

/**
 * The Downloads controller of each session, outside composition so they
 * live while the person switches tabs, and end with the session (its scope).
 */
internal object DownloadsHolders {
    class Held(val controller: DownloadsController)

    private val held = mutableMapOf<Long, Held>()

    @Synchronized
    fun forSession(session: Session, uploads: StateFlow<List<UploadBatchProgress>>, stopUpload: (String) -> Unit): Held =
        held.getOrPut(session.id) {
            val controller = DownloadsController(
                api = session.api,
                data = session.data,
                user = session.user,
                connection = session.events.status,
                uploads = uploads,
                onStopUpload = stopUpload,
                scope = session.scope,
            )
            controller.refresh()
            session.scope.coroutineContext.job.invokeOnCompletion { drop(session.id) }
            Held(controller)
        }

    @Synchronized
    private fun drop(sessionId: Long) {
        held.remove(sessionId)
    }
}
