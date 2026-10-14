package com.thekozugroup.popcornstash.data.auth

import android.app.Activity
import androidx.credentials.CredentialManager
import androidx.credentials.GetCredentialRequest
import androidx.credentials.GetPublicKeyCredentialOption
import androidx.credentials.PublicKeyCredential
import androidx.credentials.exceptions.GetCredentialCancellationException
import androidx.credentials.exceptions.GetCredentialException
import androidx.credentials.exceptions.NoCredentialException

/** Passkeys through Android's Credential Manager, shown over [activity]. */
class CredentialManagerAuthenticator(private val activity: Activity) : PasskeyAuthenticator {
    override suspend fun assertion(requestJson: String): String {
        val manager = CredentialManager.create(activity)
        val request = GetCredentialRequest(listOf(GetPublicKeyCredentialOption(requestJson)))
        val result = try {
            manager.getCredential(activity, request)
        } catch (_: GetCredentialCancellationException) {
            throw PasskeyException("Sign-in was cancelled.", cancelled = true)
        } catch (_: NoCredentialException) {
            throw PasskeyException("This phone has no passkey for this server. Pair with a code instead.")
        } catch (_: GetCredentialException) {
            throw PasskeyException("The passkey did not work. Try again, or pair with a code.")
        }
        val credential = result.credential as? PublicKeyCredential
            ?: throw PasskeyException("The passkey did not work. Try again, or pair with a code.")
        return credential.authenticationResponseJson
    }
}
