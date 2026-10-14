package com.thekozugroup.popcornstash.share

import android.content.Intent
import android.net.Uri
import androidx.core.content.IntentCompat
import com.thekozugroup.popcornstash.feature.downloads.DetectedLink
import com.thekozugroup.popcornstash.feature.downloads.findLink

/** What another app shared to PopcornStash. */
sealed interface ShareRequest {
    /** A web link or a magnet: it goes to Downloads after a confirm. */
    data class Link(val link: DetectedLink) : ShareRequest

    /** Photos, videos or other files ([refused] could not be taken: not `content://`, or this app's own). */
    data class Files(val uris: List<Uri>, val refused: Int) : ShareRequest

    /** Nothing PopcornStash can use; [refused] files were offered as paths it does not read. */
    data class Unusable(val refused: Int = 0) : ShareRequest
}

/**
 * Reads an `ACTION_SEND` or `ACTION_SEND_MULTIPLE` intent (docs/architecture.md section 10,
 * Share sheet). Files win over text; text is searched for its first http(s) or magnet link.
 * Files are taken only as `content://` URIs that [shareable] accepts.
 */
fun parseShare(intent: Intent, shareable: (Uri) -> Boolean): ShareRequest {
    val streams = when (intent.action) {
        Intent.ACTION_SEND_MULTIPLE ->
            IntentCompat.getParcelableArrayListExtra(intent, Intent.EXTRA_STREAM, Uri::class.java).orEmpty()
        Intent.ACTION_SEND ->
            listOfNotNull(IntentCompat.getParcelableExtra(intent, Intent.EXTRA_STREAM, Uri::class.java))
        else -> return ShareRequest.Unusable()
    }.ifEmpty {
        // Some apps put files only in ClipData. Text shares often carry a preview picture there,
        // so ClipData counts only when the share is not text.
        if (intent.type?.startsWith("text/") == true) {
            emptyList()
        } else {
            val clip = intent.clipData
            (0 until (clip?.itemCount ?: 0)).mapNotNull { clip?.getItemAt(it)?.uri }
        }
    }
    if (streams.isNotEmpty()) {
        val accepted = streams.distinct().filter(shareable)
        val refused = streams.distinct().size - accepted.size
        return if (accepted.isEmpty()) ShareRequest.Unusable(refused) else ShareRequest.Files(accepted, refused)
    }
    val text = intent.getCharSequenceExtra(Intent.EXTRA_TEXT)
        ?: intent.clipData?.takeIf { it.itemCount > 0 }?.getItemAt(0)?.text
    val link = findLink(text) ?: findLink(intent.getStringExtra(Intent.EXTRA_SUBJECT))
    return if (link != null) ShareRequest.Link(link) else ShareRequest.Unusable()
}
