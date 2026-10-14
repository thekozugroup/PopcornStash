package com.thekozugroup.popcornstash.share

import android.content.ContentResolver
import android.content.Context
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.provider.DocumentsContract
import android.provider.MediaStore
import android.provider.OpenableColumns
import java.io.FileInputStream
import java.io.FileNotFoundException
import java.io.IOException
import java.io.InputStream

/**
 * Whether PopcornStash may read [uri] for an upload: only `content://` URIs from another
 * app's provider. A `file://` path, or one of this app's own providers, could make the app
 * upload its own private files (a token, someone's cached pictures), so those are refused.
 */
fun isShareableUri(context: Context, uri: Uri): Boolean {
    if (uri.scheme != ContentResolver.SCHEME_CONTENT) return false
    val authority = uri.authority ?: return false
    val own = context.packageName
    if (authority == own || authority.startsWith("$own.")) return false
    val packages = context.packageManager
    val provider = runCatching {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            packages.resolveContentProvider(authority, PackageManager.ComponentInfoFlags.of(0))
        } else {
            @Suppress("DEPRECATION")
            packages.resolveContentProvider(authority, 0)
        }
    }.getOrNull()
    return provider?.packageName != own
}

/** Reads a shared file's name, size, type and last change from its provider. */
fun describeSharedFile(resolver: ContentResolver, uri: Uri, fallbackName: String): SharedFile {
    var name: String? = null
    var size: Long? = null
    var modified: Long? = null
    runCatching {
        resolver.query(uri, null, null, null, null)?.use { cursor ->
            if (cursor.moveToFirst()) {
                fun column(name: String) = cursor.getColumnIndex(name).takeIf { it >= 0 && !cursor.isNull(it) }
                column(OpenableColumns.DISPLAY_NAME)?.let { name = cursor.getString(it) }
                column(OpenableColumns.SIZE)?.let { size = cursor.getLong(it).takeIf { bytes -> bytes >= 0 } }
                // Documents say milliseconds; MediaStore says seconds.
                modified = column(DocumentsContract.Document.COLUMN_LAST_MODIFIED)?.let { cursor.getLong(it) / 1000 }
                    ?: column(MediaStore.MediaColumns.DATE_MODIFIED)?.let { cursor.getLong(it) }
            }
        }
    }
    if (size == null) {
        size = runCatching { resolver.openAssetFileDescriptor(uri, "r")?.use { it.length.takeIf { length -> length >= 0 } } }.getOrNull()
    }
    return SharedFile(
        uri = uri.toString(),
        name = cleanFileName(name ?: uri.lastPathSegment ?: fallbackName, fallbackName),
        size = size,
        mime = runCatching { resolver.getType(uri) }.getOrNull(),
        lastModified = modified?.takeIf { it > 0 },
    )
}

/** A name the server can take: the last path part, no control characters, never empty. */
fun cleanFileName(raw: String, fallback: String): String {
    val last = raw.substringAfterLast('/').substringAfterLast('\\')
    val clean = last.filterNot { it.isISOControl() }.trim()
    return clean.ifEmpty { fallback }.take(200)
}

/**
 * A shared file read through its provider: one open stream read forwards, re-opened only
 * when an upload goes back (a resume from an earlier offset). A real file descriptor seeks.
 */
class ContentChunkSource(private val resolver: ContentResolver, private val uri: Uri) : ChunkSource {
    private var stream: InputStream? = null
    private var position = 0L

    override fun read(offset: Long, length: Int): ByteArray {
        val input = positionAt(offset)
        val buffer = ByteArray(length)
        var filled = 0
        while (filled < length) {
            val count = input.read(buffer, filled, length - filled)
            if (count < 0) break
            filled += count
        }
        position += filled
        return if (filled == length) buffer else buffer.copyOf(filled)
    }

    private fun positionAt(offset: Long): InputStream {
        val current = stream
        if (current != null && offset == position) return current
        if (current is FileInputStream) {
            try {
                current.channel.position(offset)
                position = offset
                return current
            } catch (_: IOException) {
                // A pipe cannot seek: open it again below.
            }
        }
        if (current != null && offset > position) {
            skipFully(current, offset - position)
            position = offset
            return current
        }
        close()
        val opened = open()
        stream = opened
        position = 0
        skipFully(opened, offset)
        position = offset
        return opened
    }

    private fun open(): InputStream = try {
        resolver.openInputStream(uri) ?: throw SourceUnreadableException("The shared file is not there any more.")
    } catch (e: SecurityException) {
        throw SourceUnreadableException("PopcornStash may no longer read the shared file.", e)
    } catch (e: FileNotFoundException) {
        throw SourceUnreadableException("The shared file is not there any more.", e)
    }

    private fun skipFully(input: InputStream, bytes: Long) {
        var left = bytes
        val scratch = ByteArray(64 * 1024)
        while (left > 0) {
            val skipped = input.skip(left)
            if (skipped > 0) {
                left -= skipped
                continue
            }
            val count = input.read(scratch, 0, minOf(scratch.size.toLong(), left).toInt())
            if (count < 0) throw SourceUnreadableException("The shared file is shorter than before.")
            left -= count
        }
    }

    /** Counts the bytes when the provider did not say how big the file is. */
    fun measure(): Long {
        close()
        val input = open()
        var total = 0L
        val scratch = ByteArray(64 * 1024)
        input.use {
            while (true) {
                val count = it.read(scratch)
                if (count < 0) break
                total += count
            }
        }
        return total
    }

    override fun close() {
        runCatching { stream?.close() }
        stream = null
        position = 0
    }
}
