package com.thekozugroup.popcornstash.data.auth

import com.thekozugroup.popcornstash.data.api.Role
import com.thekozugroup.popcornstash.data.api.User
import com.thekozugroup.popcornstash.testing.Fixtures
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey

/** A software AES key standing in for the Keystore (which Robolectric and the JVM do not have). */
class SoftwareCipher : SecretCipher {
    private var key: SecretKey? = null
    var destroyed = 0

    private fun key(): SecretKey = key ?: KeyGenerator.getInstance("AES").apply { init(256) }.generateKey().also { key = it }

    override fun encrypt(plain: ByteArray) = AesGcm.seal(key(), plain)

    override fun decrypt(sealed: ByteArray) = AesGcm.open(key(), sealed)

    override fun destroy() {
        key = null
        destroyed++
    }
}

class SessionStoreTest {
    @get:Rule
    val folder = TemporaryFolder()

    private val session = StoredSession(
        server = "https://stash.example.com/",
        token = Fixtures.TOKEN,
        user = User("u-owner", "Ana Ribeiro", Role.Owner, 1),
    )

    @Test
    fun keepsTheSessionSealed() {
        val cipher = SoftwareCipher()
        val store = SessionStore(folder.root, cipher)
        store.save(session)
        assertEquals(session, store.load())
        val onDisk = folder.root.resolve("session.bin").readBytes()
        assertFalse("the token is not readable on disk", String(onDisk, Charsets.ISO_8859_1).contains(Fixtures.TOKEN))
        assertFalse("nor the person's name", String(onDisk, Charsets.ISO_8859_1).contains("Ana Ribeiro"))
        assertEquals(1, onDisk[0].toInt())
    }

    @Test
    fun clearingDeletesTheFileAndTheKey() {
        val cipher = SoftwareCipher()
        val store = SessionStore(folder.root, cipher)
        store.save(session)
        store.clear()
        assertFalse(folder.root.resolve("session.bin").exists())
        assertEquals(1, cipher.destroyed)
        assertNull(store.load())
    }

    @Test
    fun aSessionSealedWithALostKeyIsForgotten() {
        val first = SoftwareCipher()
        SessionStore(folder.root, first).save(session)
        // A restore onto another phone, or a wiped keystore: a different key.
        val store = SessionStore(folder.root, SoftwareCipher())
        assertNull(store.load())
        assertFalse(folder.root.resolve("session.bin").exists())
    }

    @Test
    fun printingASessionNeverShowsTheToken() {
        assertFalse(session.toString().contains(Fixtures.TOKEN))
        assertTrue(session.toString().contains("<redacted>"))
    }

    @Test
    fun everySealIsDifferent() {
        val cipher = SoftwareCipher()
        val a = cipher.encrypt("same".toByteArray())
        val b = cipher.encrypt("same".toByteArray())
        assertFalse(a.contentEquals(b))
        assertEquals("same", String(cipher.decrypt(a)))
    }
}
