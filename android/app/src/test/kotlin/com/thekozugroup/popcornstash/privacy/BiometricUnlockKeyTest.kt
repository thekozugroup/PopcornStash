package com.thekozugroup.popcornstash.privacy

import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class BiometricUnlockKeyTest {
    @Test fun rejectsMissingMismatchedAndUnusableOperations() {
        val expected = Cipher.getInstance("AES/GCM/NoPadding")
        assertFalse(BiometricUnlockKey.verify(expected, null))
        assertFalse(BiometricUnlockKey.verify(expected, Cipher.getInstance("AES/GCM/NoPadding")))
        assertFalse(BiometricUnlockKey.verify(expected, expected))
    }
    @Test fun requiresTheSameUsableCipherOperation() {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, KeyGenerator.getInstance("AES").generateKey())
        assertTrue(BiometricUnlockKey.verify(cipher, cipher))
    }
}
