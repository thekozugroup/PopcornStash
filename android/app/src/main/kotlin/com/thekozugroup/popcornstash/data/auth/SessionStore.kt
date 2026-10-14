package com.thekozugroup.popcornstash.data.auth

import com.thekozugroup.popcornstash.data.api.StashJson
import com.thekozugroup.popcornstash.data.api.User
import kotlinx.serialization.Serializable
import java.io.File

/** What the app keeps between launches for the signed-in person. Never printed in full. */
@Serializable
data class StoredSession(val server: String, val token: String, val user: User) {
    override fun toString() = "StoredSession(server=$server, token=<redacted>, user=${user.id})"
}

/**
 * Keeps the session sealed by a [SecretCipher] in one file of the app's no-backup folder
 * (`noBackupFilesDir`, which backups and device transfer never copy; the manifest also turns
 * backups off). Clearing deletes the file and the key.
 */
class SessionStore(dir: File, private val cipher: SecretCipher) {
    private val file = File(dir, "session.bin")

    @Synchronized
    fun load(): StoredSession? {
        if (!file.isFile) return null
        return try {
            val plain = cipher.decrypt(file.readBytes())
            StashJson.decodeFromString(StoredSession.serializer(), plain.decodeToString())
        } catch (_: Exception) {
            // A key that was wiped (a restore onto another phone, a cleared keystore) or a
            // damaged file: start again from sign-in.
            clear()
            null
        }
    }

    @Synchronized
    fun save(session: StoredSession) {
        val sealed = cipher.encrypt(StashJson.encodeToString(StoredSession.serializer(), session).encodeToByteArray())
        file.parentFile?.mkdirs()
        val temporary = File(file.parentFile, "${file.name}.new")
        temporary.writeBytes(sealed)
        if (!temporary.renameTo(file)) {
            temporary.delete()
            error("Could not keep the session.")
        }
    }

    @Synchronized
    fun clear() {
        file.delete()
        File(file.parentFile, "${file.name}.new").delete()
        cipher.destroy()
    }
}
