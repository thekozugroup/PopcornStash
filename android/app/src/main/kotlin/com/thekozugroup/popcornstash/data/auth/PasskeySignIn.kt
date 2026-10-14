package com.thekozugroup.popcornstash.data.auth

import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.ApiException
import com.thekozugroup.popcornstash.data.api.LoginFinishRequest
import com.thekozugroup.popcornstash.data.api.LoginFinished
import com.thekozugroup.popcornstash.data.api.StashJson

/** Gets a passkey assertion for a WebAuthn request (Credential Manager on a phone, a software key in tests). */
fun interface PasskeyAuthenticator {
    /** [requestJson] is the `publicKey` request options; the answer is the credential JSON. */
    suspend fun assertion(requestJson: String): String
}

/** A passkey sign-in that did not finish; [cancelled] when the person closed the sheet. */
class PasskeyException(message: String, val cancelled: Boolean = false) : Exception(message)

/**
 * Signs in with a passkey and asks for a bearer session instead of a cookie
 * (docs/architecture.md section 10, Passkeys): `login/start`, the authenticator's assertion,
 * then `login/finish` with `want_token: true`.
 */
class PasskeySignIn(
    private val api: ApiClient,
    private val authenticator: PasskeyAuthenticator,
    private val deviceName: String,
) {
    suspend fun signIn(): LoginFinished {
        val started = api.startLogin()
        val publicKey = started.options["publicKey"] ?: throw ApiException.unexpected(200)
        val credential = authenticator.assertion(publicKey.toString())
        val parsed = runCatching { StashJson.parseToJsonElement(credential) }
            .getOrElse { throw PasskeyException("The passkey answer could not be read. Try again.") }
        val finished = api.finishLogin(LoginFinishRequest(started.flowId, parsed, wantToken = true, deviceName = deviceName))
        if (finished.token.isNullOrEmpty()) throw ApiException.unexpected(200)
        return finished
    }
}
