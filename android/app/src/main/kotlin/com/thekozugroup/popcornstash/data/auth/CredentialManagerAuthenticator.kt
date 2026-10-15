package com.thekozugroup.popcornstash.data.auth

import android.app.Activity
import android.util.Log
import androidx.credentials.CredentialManager
import androidx.credentials.GetCredentialRequest
import androidx.credentials.GetPublicKeyCredentialOption
import androidx.credentials.PublicKeyCredential
import androidx.credentials.exceptions.GetCredentialCancellationException
import androidx.credentials.exceptions.GetCredentialException
import androidx.credentials.exceptions.GetCredentialInterruptedException
import androidx.credentials.exceptions.GetCredentialProviderConfigurationException
import androidx.credentials.exceptions.GetCredentialUnsupportedException
import androidx.credentials.exceptions.NoCredentialException
import androidx.credentials.exceptions.publickeycredential.GetPublicKeyCredentialDomException
import androidx.credentials.exceptions.domerrors.NotAllowedError
import androidx.credentials.exceptions.domerrors.SecurityError
import com.thekozugroup.popcornstash.data.api.StashJson
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

/** Passkeys through Android's Credential Manager, shown over [activity]. */
class CredentialManagerAuthenticator(private val activity: Activity) : PasskeyAuthenticator {
    override suspend fun assertion(requestJson: String): String {
        val manager = CredentialManager.create(activity)
        val request = GetCredentialRequest(listOf(GetPublicKeyCredentialOption(requestJson)))
        val result = try {
            manager.getCredential(activity, request)
        } catch (e: GetCredentialException) {
            // Provider messages can contain account data; keep only exception classes in logs.
            Log.w("PopcornStash.Passkey", passkeyDiagnostic(e))
            val rpId = runCatching { StashJson.parseToJsonElement(requestJson).jsonObject["rpId"]?.jsonPrimitive?.content }.getOrNull()
            throw passkeyFailure(e, rpId)
        }
        val credential = result.credential as? PublicKeyCredential
            ?: throw PasskeyException("The passkey did not work. Try again, or pair with a code.")
        return credential.authenticationResponseJson
    }
}

internal fun passkeyDiagnostic(error: GetCredentialException): String =
    "${error.javaClass.simpleName}; DOM=${(error as? GetPublicKeyCredentialDomException)?.domError?.javaClass?.simpleName ?: "none"}"

internal fun passkeyFailure(error: GetCredentialException, rpId: String?): PasskeyException {
    val message = when (error) {
        is GetCredentialCancellationException -> "Sign-in was cancelled."
        is NoCredentialException -> "This phone has no passkey for this server. Pair with a code instead."
        is GetCredentialInterruptedException -> "Sign-in was interrupted. Try again."
        is GetCredentialProviderConfigurationException, is GetCredentialUnsupportedException ->
            "This phone's passkey service is not ready. Update Google Play services or turn on a passkey service in Settings."
        is GetPublicKeyCredentialDomException -> when (error.domError) {
            is SecurityError -> "Android could not verify this app for ${rpId ?: "this server"}. Google needs to reach the server's app verification file on the internet. Pair with a code instead."
            is NotAllowedError -> "The passkey was not used. Unlock with your screen lock and try again, or pair with a code."
            else -> "The passkey did not work. Try again, or pair with a code. (${error.domError.javaClass.simpleName})"
        }
        else -> "The passkey did not work. Try again, or pair with a code. (${error.javaClass.simpleName})"
    }
    return PasskeyException(message, cancelled = error is GetCredentialCancellationException)
}
