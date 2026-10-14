package com.thekozugroup.popcornstash.testing

import com.thekozugroup.popcornstash.data.auth.PasskeyAuthenticator
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonObject
import java.io.ByteArrayOutputStream
import java.math.BigInteger
import java.nio.ByteBuffer
import java.security.KeyPair
import java.security.KeyPairGenerator
import java.security.MessageDigest
import java.security.SecureRandom
import java.security.Signature
import java.security.interfaces.ECPublicKey
import java.security.spec.ECGenParameterSpec
import java.util.Base64

/**
 * A software passkey for tests, like the virtual authenticator tests/e2e uses in Chrome:
 * one P-256 key, "none" attestation, user presence and verification always given. It makes
 * the owner's passkey at setup and answers sign-in requests as Credential Manager would.
 */
class SoftPasskey(private val origin: String) : PasskeyAuthenticator {
    /** Where sign-ins come from: the web's origin, or the Android app's `android:apk-key-hash:` one. */
    var signInOrigin: String = origin

    private val keys: KeyPair = KeyPairGenerator.getInstance("EC").apply { initialize(ECGenParameterSpec("secp256r1")) }.generateKeyPair()
    private val credentialId = ByteArray(16).also { SecureRandom().nextBytes(it) }
    private var userHandle: ByteArray? = null
    private var signCount = 0

    /** Answers `navigator.credentials.create()` for [options] (the `publicKey` creation options). */
    fun register(options: JsonObject): JsonObject {
        val rpId = options.getValue("rp").jsonObject.getValue("id").jsonPrimitive.content
        userHandle = b64decode(options.getValue("user").jsonObject.getValue("id").jsonPrimitive.content)
        val clientData = clientData("webauthn.create", options.getValue("challenge").jsonPrimitive.content)
        val authData = sha256(rpId.toByteArray()) + byteArrayOf((FLAG_UP or FLAG_UV or FLAG_AT).toByte()) + int32(0) +
            ByteArray(16) + int16(credentialId.size) + credentialId + coseKey()
        val attestation = Cbor.map(
            Cbor.text("fmt") to Cbor.text("none"),
            Cbor.text("attStmt") to Cbor.map(),
            Cbor.text("authData") to Cbor.bytes(authData),
        )
        return buildJsonObject {
            put("id", b64(credentialId))
            put("rawId", b64(credentialId))
            put("type", "public-key")
            putJsonObject("response") {
                put("attestationObject", b64(attestation))
                put("clientDataJSON", b64(clientData))
            }
            putJsonObject("extensions") {}
        }
    }

    /** Answers a sign-in request, as Credential Manager's `authenticationResponseJson`. */
    override suspend fun assertion(requestJson: String): String {
        val request = Json.parseToJsonElement(requestJson).jsonObject
        val rpId = request.getValue("rpId").jsonPrimitive.content
        val clientData = clientData("webauthn.get", request.getValue("challenge").jsonPrimitive.content, signInOrigin)
        signCount++
        val authData = sha256(rpId.toByteArray()) + byteArrayOf((FLAG_UP or FLAG_UV).toByte()) + int32(signCount)
        val signature = Signature.getInstance("SHA256withECDSA").run {
            initSign(keys.private)
            update(authData + sha256(clientData))
            sign()
        }
        return buildJsonObject {
            put("id", b64(credentialId))
            put("rawId", b64(credentialId))
            put("type", "public-key")
            putJsonObject("response") {
                put("authenticatorData", b64(authData))
                put("clientDataJSON", b64(clientData))
                put("signature", b64(signature))
                put("userHandle", b64(requireNotNull(userHandle) { "register first" }))
            }
            putJsonObject("clientExtensionResults") {}
            put("authenticatorAttachment", "platform")
        }.toString()
    }

    private fun clientData(type: String, challenge: String, from: String = origin): ByteArray =
        buildJsonObject {
            put("type", type)
            put("challenge", challenge)
            put("origin", from)
            put("crossOrigin", false)
        }.toString().toByteArray()

    private fun coseKey(): ByteArray {
        val point = (keys.public as ECPublicKey).w
        return Cbor.map(
            Cbor.int(1) to Cbor.int(2),
            Cbor.int(3) to Cbor.int(-7),
            Cbor.int(-1) to Cbor.int(1),
            Cbor.int(-2) to Cbor.bytes(fixed32(point.affineX)),
            Cbor.int(-3) to Cbor.bytes(fixed32(point.affineY)),
        )
    }

    private companion object {
        const val FLAG_UP = 0x01
        const val FLAG_UV = 0x04
        const val FLAG_AT = 0x40

        fun sha256(bytes: ByteArray): ByteArray = MessageDigest.getInstance("SHA-256").digest(bytes)

        fun int32(value: Int): ByteArray = ByteBuffer.allocate(4).putInt(value).array()

        fun int16(value: Int): ByteArray = byteArrayOf((value shr 8).toByte(), value.toByte())

        fun fixed32(value: BigInteger): ByteArray {
            val raw = value.toByteArray().dropWhile { it == 0.toByte() }.toByteArray()
            return ByteArray(32 - raw.size) + raw
        }

        fun b64(bytes: ByteArray): String = Base64.getUrlEncoder().withoutPadding().encodeToString(bytes)

        fun b64decode(text: String): ByteArray = Base64.getUrlDecoder().decode(text.trimEnd('='))
    }
}

/** Just enough CBOR (RFC 8949) for an attestation object and a COSE key. */
object Cbor {
    fun int(value: Long): ByteArray = if (value >= 0) head(0, value) else head(1, -1 - value)

    fun int(value: Int): ByteArray = int(value.toLong())

    fun bytes(value: ByteArray): ByteArray = head(2, value.size.toLong()) + value

    fun text(value: String): ByteArray = value.toByteArray().let { head(3, it.size.toLong()) + it }

    fun map(vararg entries: Pair<ByteArray, ByteArray>): ByteArray {
        val out = ByteArrayOutputStream()
        out.write(head(5, entries.size.toLong()))
        entries.forEach { (key, value) ->
            out.write(key)
            out.write(value)
        }
        return out.toByteArray()
    }

    private fun head(major: Int, value: Long): ByteArray {
        val type = major shl 5
        return when {
            value < 24 -> byteArrayOf((type or value.toInt()).toByte())
            value < 0x100 -> byteArrayOf((type or 24).toByte(), value.toByte())
            value < 0x10000 -> byteArrayOf((type or 25).toByte(), (value shr 8).toByte(), value.toByte())
            else -> byteArrayOf((type or 26).toByte()) + ByteBuffer.allocate(4).putInt(value.toInt()).array()
        }
    }
}
