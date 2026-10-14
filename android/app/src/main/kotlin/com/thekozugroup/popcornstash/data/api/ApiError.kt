package com.thekozugroup.popcornstash.data.api

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject

/**
 * A request that did not work, with a sentence the app can show as it is. Server errors
 * carry the server's own message (`{"error": {"code", "message", "field"}}`, complete
 * sentences meant for people, docs/architecture.md section 8) and the request field it
 * names, so a form can show it under that field.
 */
class ApiException(
    /** HTTP status, or 0 when the server did not answer at all. */
    val status: Int,
    /** The server's code (`not_found`, `conflict`, ...), or `network` and `unexpected` from the app. */
    val code: String,
    override val message: String,
    val field: String? = null,
) : Exception(message) {
    val isNetwork: Boolean get() = code == CODE_NETWORK
    val isUnauthorized: Boolean get() = status == 401
    val isNotFound: Boolean get() = status == 404

    companion object {
        const val CODE_NETWORK = "network"
        const val CODE_UNEXPECTED = "unexpected"

        fun network() = ApiException(0, CODE_NETWORK, "PopcornStash did not answer. Check the connection and try again.")

        fun unexpected(status: Int) =
            ApiException(status, CODE_UNEXPECTED, "PopcornStash answered in a way this app does not understand. Update the app or the server.")
    }
}

@Serializable
internal data class ErrorBody(val code: String = "", val message: String = "", val field: String? = null)

/** Turns an error response into an [ApiException] with a sentence a person can act on. */
internal fun errorFrom(status: Int, body: String, json: Json): ApiException {
    val envelope = runCatching { json.parseToJsonElement(body).jsonObject["error"] as? JsonObject }.getOrNull()
    val parsed = envelope?.let { runCatching { json.decodeFromJsonElement(ErrorBody.serializer(), it) }.getOrNull() }
    if (parsed != null && parsed.message.isNotBlank()) {
        return ApiException(status, parsed.code.ifBlank { codeFor(status) }, parsed.message, parsed.field)
    }
    return ApiException(status, codeFor(status), fallbackMessage(status))
}

private fun codeFor(status: Int): String = when (status) {
    400 -> "bad_request"
    401 -> "unauthorized"
    403 -> "forbidden"
    404 -> "not_found"
    405 -> "method_not_allowed"
    409 -> "conflict"
    413 -> "payload_too_large"
    429 -> "rate_limited"
    in 500..599 -> "internal"
    else -> ApiException.CODE_UNEXPECTED
}

/** For answers without PopcornStash's error body (a proxy, an old server). */
internal fun fallbackMessage(status: Int): String = when (status) {
    400 -> "PopcornStash could not use that request."
    401 -> "You are signed out. Sign in again."
    403 -> "Your account cannot do that."
    404 -> "That no longer exists."
    409 -> "That changed meanwhile. Look again and retry."
    413 -> "That file is too big for this server."
    429 -> "Too many tries. Wait a minute and try again."
    in 500..599 -> "PopcornStash ran into a problem. Try again in a moment."
    else -> "PopcornStash answered in a way this app does not understand. Update the app or the server."
}
