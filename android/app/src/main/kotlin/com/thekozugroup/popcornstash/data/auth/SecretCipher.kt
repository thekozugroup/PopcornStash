package com.thekozugroup.popcornstash.data.auth

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** Seals small secrets (the session token) for storage in the app's private files. */
interface SecretCipher {
    fun encrypt(plain: ByteArray): ByteArray

    fun decrypt(sealed: ByteArray): ByteArray

    /** Deletes the key, so nothing sealed with it can be read again. */
    fun destroy()
}

/**
 * AES-256-GCM under a key that lives in the Android Keystore and can never be exported
 * (docs/architecture.md section 10, Token storage). Each seal has a fresh 12-byte IV.
 * The sealed form is `1 | iv | ciphertext and tag`.
 */
class KeystoreCipher(private val alias: String = "popcornstash.session") : SecretCipher {
    private fun keyStore(): KeyStore = KeyStore.getInstance(PROVIDER).apply { load(null) }

    private fun key(): SecretKey {
        (keyStore().getKey(alias, null) as? SecretKey)?.let { return it }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, PROVIDER)
        generator.init(
            KeyGenParameterSpec.Builder(alias, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .setRandomizedEncryptionRequired(true)
                .build(),
        )
        return generator.generateKey()
    }

    override fun encrypt(plain: ByteArray): ByteArray = AesGcm.seal(key(), plain)

    override fun decrypt(sealed: ByteArray): ByteArray = AesGcm.open(key(), sealed)

    override fun destroy() {
        runCatching { keyStore().deleteEntry(alias) }
    }

    private companion object {
        const val PROVIDER = "AndroidKeyStore"
    }
}

/** The sealed format shared by [KeystoreCipher] and the tests' software key. */
object AesGcm {
    private const val VERSION: Byte = 1
    private const val IV_BYTES = 12
    private const val TAG_BITS = 128

    fun seal(key: SecretKey, plain: ByteArray): ByteArray {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, key)
        val iv = cipher.iv
        require(iv.size == IV_BYTES) { "unexpected IV size" }
        return byteArrayOf(VERSION) + iv + cipher.doFinal(plain)
    }

    fun open(key: SecretKey, sealed: ByteArray): ByteArray {
        require(sealed.size > 1 + IV_BYTES && sealed[0] == VERSION) { "not a sealed secret" }
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(TAG_BITS, sealed, 1, IV_BYTES))
        return cipher.doFinal(sealed, 1 + IV_BYTES, sealed.size - 1 - IV_BYTES)
    }
}
