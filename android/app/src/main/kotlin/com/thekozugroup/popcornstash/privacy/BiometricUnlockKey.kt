package com.thekozugroup.popcornstash.privacy

import android.os.Build
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyPermanentlyInvalidatedException
import android.security.keystore.KeyProperties
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey

/** Each unlock requires a fresh Keystore operation; device credentials cannot authorize it. */
internal object BiometricUnlockKey {
    private const val Alias = "popcornstash.biometric-unlock.v1"

    @Suppress("DEPRECATION")
    fun newCipher(): Cipher {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        fun create(): SecretKey {
            val spec = KeyGenParameterSpec.Builder(Alias, KeyProperties.PURPOSE_ENCRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setUserAuthenticationRequired(true)
                .setInvalidatedByBiometricEnrollment(true)
            if (Build.VERSION.SDK_INT >= 30) spec.setUserAuthenticationParameters(0, KeyProperties.AUTH_BIOMETRIC_STRONG)
            else spec.setUserAuthenticationValidityDurationSeconds(-1)
            return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").run { init(spec.build()); generateKey() }
        }
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        try {
            cipher.init(Cipher.ENCRYPT_MODE, (store.getKey(Alias, null) as? SecretKey) ?: create())
        } catch (_: KeyPermanentlyInvalidatedException) {
            store.deleteEntry(Alias)
            cipher.init(Cipher.ENCRYPT_MODE, create())
        }
        return cipher
    }

    fun verify(expected: Cipher, authenticated: Cipher?): Boolean = authenticated === expected && try {
        authenticated.doFinal(byteArrayOf(1)).isNotEmpty()
    } catch (_: java.security.GeneralSecurityException) { false } catch (_: IllegalStateException) { false }
}
