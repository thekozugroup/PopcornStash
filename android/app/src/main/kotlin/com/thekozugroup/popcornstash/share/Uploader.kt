package com.thekozugroup.popcornstash.share

import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.ApiException
import com.thekozugroup.popcornstash.data.api.Download
import com.thekozugroup.popcornstash.data.api.UploadCreate
import kotlinx.coroutines.delay
import kotlinx.coroutines.ensureActive
import okhttp3.MediaType
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.RequestBody
import okio.BufferedSink
import java.io.Closeable
import java.io.IOException
import kotlin.coroutines.coroutineContext

/** The bytes of one shared file, read at any offset (a resumed upload starts mid-file). */
interface ChunkSource : Closeable {
    /** Reads up to [length] bytes from [offset]; fewer only at the end of the file. */
    @Throws(IOException::class)
    fun read(offset: Long, length: Int): ByteArray
}

/** The sharing app's read grant is gone (the app was killed, or the file was deleted). */
class SourceUnreadableException(message: String, cause: Throwable? = null) : IOException(message, cause)

/** The upload was stopped here (sign-out, or the person stopped it): nothing more may be sent. */
class UploadStoppedException : IOException("The upload was stopped.")

/**
 * Sends one file through the resumable upload API (docs/architecture.md section 8):
 * `POST /api/uploads`, `PUT /api/uploads/:id?offset=` in the server's chunk size (16 MiB),
 * then `/complete`. A chunk that did not get through is tried again after 1, 2, 5, 10 and
 * 30 seconds; when the server holds a different amount (an earlier try did arrive) it goes
 * on from what the server has. After an interruption, `GET /api/uploads/:id` says where.
 */
class Uploader(
    private val api: ApiClient,
    private val retryDelays: List<Long> = listOf(1_000, 2_000, 5_000, 10_000, 30_000),
) {
    /**
     * Uploads [file] of [size] bytes from [source] and returns the download it became.
     * [onCreated] gets the upload id as soon as the server made it (kept so a restart
     * continues); [onSent] how many bytes the server holds. [stillWanted] is asked before
     * every request: false stops the upload with [UploadStoppedException].
     */
    suspend fun upload(
        file: SharedFile,
        size: Long,
        source: ChunkSource,
        onCreated: (String) -> Unit,
        onSent: (Long) -> Unit,
        stillWanted: () -> Boolean = { true },
    ): Download {
        fun check() {
            if (!stillWanted()) throw UploadStoppedException()
        }
        check()
        var id: String? = null
        var received = 0L
        var chunk = DEFAULT_CHUNK
        file.uploadId?.let { known ->
            val status = try {
                api.uploadStatus(known)
            } catch (e: ApiException) {
                if (e.isNetwork) throw e
                null
            }
            if (status != null && status.state == "receiving" && status.size == size) {
                id = known
                received = status.received
            }
        }
        if (id == null) {
            check()
            val started = api.startUpload(UploadCreate(file.name, size, file.mime, file.lastModified))
            id = started.id
            received = started.received
            if (started.chunkSize > 0) chunk = started.chunkSize.coerceAtMost(MAX_CHUNK)
            onCreated(started.id)
        }
        val uploadId = id!!
        onSent(received)

        var failures = 0
        while (true) {
            while (received < size) {
                coroutineContext.ensureActive()
                check()
                val length = (size - received).coerceAtMost(chunk).toInt()
                val bytes = try {
                    source.read(received, length)
                } catch (e: SourceUnreadableException) {
                    throw e
                } catch (e: IOException) {
                    throw SourceUnreadableException("The shared file could not be read.", e)
                }
                if (bytes.size < length) throw SourceUnreadableException("The shared file changed while it was uploading.")
                val start = received
                try {
                    received = api.uploadChunk(uploadId, start, CountingBody(bytes) { sent -> onSent(start + sent) }).received
                    failures = 0
                } catch (e: ApiException) {
                    if (!e.isNetwork && e.status !in RETRY_STATUSES) throw e
                    if (failures >= retryDelays.size) throw e
                    delay(retryDelays[failures++])
                    // The server may hold part of the chunk that was cut off: continue from its count.
                    received = try {
                        api.uploadStatus(uploadId).received
                    } catch (_: ApiException) {
                        start
                    }
                }
                onSent(received)
            }
            coroutineContext.ensureActive()
            check()
            try {
                return api.completeUpload(uploadId)
            } catch (e: ApiException) {
                // The server holds less than the whole file (a chunk it dropped): send the rest.
                if (e.status != 409 || failures >= retryDelays.size) throw e
                failures++
                received = api.uploadStatus(uploadId).received
                if (received >= size) throw e
            }
        }
    }

    private class CountingBody(private val bytes: ByteArray, private val onSent: (Long) -> Unit) : RequestBody() {
        override fun contentType(): MediaType = OCTET_STREAM

        override fun contentLength(): Long = bytes.size.toLong()

        override fun writeTo(sink: BufferedSink) {
            var written = 0
            while (written < bytes.size) {
                val count = minOf(SLICE, bytes.size - written)
                sink.write(bytes, written, count)
                sink.flush()
                written += count
                onSent(written.toLong())
            }
        }
    }

    companion object {
        const val DEFAULT_CHUNK = 16L * 1024 * 1024
        const val MAX_CHUNK = 64L * 1024 * 1024
        private const val SLICE = 64 * 1024
        private val OCTET_STREAM = "application/octet-stream".toMediaType()

        /** Answers worth another try: a timeout, too many requests, or the server or a proxy failing. */
        private val RETRY_STATUSES = setOf(408, 429, 500, 502, 503, 504)
    }
}
