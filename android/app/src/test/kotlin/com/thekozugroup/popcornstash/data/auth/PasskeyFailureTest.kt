package com.thekozugroup.popcornstash.data.auth

import androidx.credentials.exceptions.GetCredentialCancellationException
import androidx.credentials.exceptions.GetCredentialInterruptedException
import androidx.credentials.exceptions.GetCredentialProviderConfigurationException
import androidx.credentials.exceptions.GetCredentialUnknownException
import androidx.credentials.exceptions.GetCredentialUnsupportedException
import androidx.credentials.exceptions.NoCredentialException
import androidx.credentials.exceptions.domerrors.NotAllowedError
import androidx.credentials.exceptions.domerrors.SecurityError
import androidx.credentials.exceptions.publickeycredential.GetPublicKeyCredentialDomException
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PasskeyFailureTest {
    @Test
    fun securityFailureExplainsVerificationAndPairing() {
        val error = GetPublicKeyCredentialDomException(SecurityError(), "private-account-data")
        val failure = passkeyFailure(error, "stash.example.com")
        assertTrue(failure.message!!.contains("stash.example.com"))
        assertTrue(failure.message!!.contains("Pair with a code"))
        assertEquals("GetPublicKeyCredentialDomException; DOM=SecurityError", passkeyDiagnostic(error))
        assertFalse(passkeyDiagnostic(error).contains("private-account-data"))
        assertFalse(failure.message!!.contains("private-account-data"))
    }

    @Test
    fun eachKnownFailureNamesTheRecovery() {
        assertTrue(passkeyFailure(GetPublicKeyCredentialDomException(NotAllowedError()), null).message!!.contains("screen lock"))
        assertTrue(passkeyFailure(GetCredentialProviderConfigurationException(), null).message!!.contains("passkey service"))
        assertTrue(passkeyFailure(GetCredentialUnsupportedException(), null).message!!.contains("passkey service"))
        assertEquals("Sign-in was interrupted. Try again.", passkeyFailure(GetCredentialInterruptedException(), null).message)
        assertTrue(passkeyFailure(NoCredentialException(), null).message!!.contains("no passkey"))
        assertTrue(passkeyFailure(GetCredentialCancellationException(), null).cancelled)
        val unknown = passkeyFailure(GetCredentialUnknownException("private-account-data"), null)
        assertTrue(unknown.message!!.contains("GetCredentialUnknownException"))
        assertFalse(unknown.message!!.contains("private-account-data"))
    }
}
