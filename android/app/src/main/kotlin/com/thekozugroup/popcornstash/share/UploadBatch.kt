package com.thekozugroup.popcornstash.share

import com.thekozugroup.popcornstash.data.api.StashJson
import kotlinx.serialization.Serializable
import java.io.File

/** One file someone shared to PopcornStash, as the share sheet read it from its provider. */
@Serializable
data class SharedFile(
    /** A `content://` URI from the sharing app; never a `file://` path. */
    val uri: String,
    val name: String,
    /** Bytes, when the provider says; the upload measures the file itself otherwise. */
    val size: Long? = null,
    val mime: String? = null,
    /** Unix seconds, when the provider knows when the file was last changed. */
    val lastModified: Long? = null,
    /** The server's upload id once `POST /api/uploads` answered, so a restart continues it. */
    val uploadId: String? = null,
    /** Uploaded and completed (`/complete` answered). */
    val done: Boolean = false,
    val libraryId: String? = null,
)

/**
 * Files one person confirmed in the share sheet, uploaded by one WorkManager job. It names
 * who confirmed it and on which server, so a job never continues under someone else.
 */
@Serializable
data class UploadBatch(
    val id: String,
    val userId: String,
    val userName: String,
    val server: String,
    val files: List<SharedFile>,
    val createdAt: Long = 0,
)

/** How far a batch is, for the Downloads tab and the notification. */
data class UploadBatchProgress(
    val id: String,
    val names: List<String>,
    val doneFiles: Int,
    val sentBytes: Long,
    /** Null while a file's size is still unknown. */
    val totalBytes: Long?,
    /** Queued but not running yet (waiting for a connection, or for the job to start). */
    val waiting: Boolean,
) {
    val totalFiles: Int get() = names.size

    /** "Holiday.mov", or "Holiday.mov and 2 more". */
    val label: String get() = names.singleOrNull() ?: "${names.firstOrNull().orEmpty()} and ${names.size - 1} more"

    val fraction: Double? get() = totalBytes?.takeIf { it > 0 }?.let { (sentBytes.toDouble() / it).coerceIn(0.0, 1.0) }
}

/**
 * The batches waiting or running, one JSON file each in a private folder under
 * `noBackupFilesDir` (they hold file names and the sharing apps' URIs). Everything here
 * belongs to the person signed in; signing out deletes the folder.
 */
class UploadBatchStore(private val dir: File) {
    @Synchronized
    fun save(batch: UploadBatch) {
        dir.mkdirs()
        val target = file(batch.id)
        val temporary = File(dir, "${batch.id}.tmp")
        temporary.writeText(StashJson.encodeToString(UploadBatch.serializer(), batch))
        if (!temporary.renameTo(target)) {
            target.delete()
            temporary.renameTo(target)
        }
    }

    @Synchronized
    fun load(id: String): UploadBatch? = file(id).takeIf { it.isFile }?.let { file ->
        runCatching { StashJson.decodeFromString(UploadBatch.serializer(), file.readText()) }.getOrNull()
    }

    @Synchronized
    fun all(): List<UploadBatch> =
        dir.listFiles { file -> file.name.endsWith(".json") }.orEmpty().mapNotNull { load(it.name.removeSuffix(".json")) }
            .sortedBy { it.createdAt }

    /** Records progress on one file of a batch (its upload id, or that it is done). */
    @Synchronized
    fun update(id: String, index: Int, change: (SharedFile) -> SharedFile): UploadBatch? {
        val batch = load(id) ?: return null
        val files = batch.files.toMutableList()
        if (index !in files.indices) return batch
        files[index] = change(files[index])
        return batch.copy(files = files).also(::save)
    }

    @Synchronized
    fun delete(id: String) {
        file(id).delete()
    }

    @Synchronized
    fun clear() {
        dir.deleteRecursively()
    }

    private fun file(id: String): File {
        require(id.matches(Regex("[A-Za-z0-9-]{1,64}"))) { "Not a batch id" }
        return File(dir, "$id.json")
    }
}
