package com.thekozugroup.popcornstash.data.auth

import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.ApiException
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flow

/** Where device pairing stands (docs/architecture.md section 10, Pairing). */
sealed interface PairingState {
    data object Requesting : PairingState

    /** Show [userCode]; the person approves it at [verifyUrl] (`<server>/pair`). */
    data class Waiting(val userCode: String, val verifyUrl: String, val expiresAt: Long) : PairingState

    /** The token for this phone, delivered once. */
    data class Approved(val token: String, val scopes: List<String>) : PairingState {
        override fun toString() = "Approved(token=<redacted>, scopes=$scopes)"
    }

    data object Expired : PairingState

    data object Denied : PairingState

    data class Failed(val message: String) : PairingState

    val finished: Boolean get() = this is Approved || this is Expired || this is Denied || this is Failed
}

/**
 * Pairs this phone: asks for a code (`client_kind: android`, which gets the `full` scope),
 * then polls every `interval` seconds until the person approves it, turns it down or it
 * expires. A poll that does not get through (no network, rate limit, server error) is tried
 * again at the next interval; the code still expires on time.
 */
class PairingFlow(
    private val api: ApiClient,
    private val deviceName: String,
    private val nowSeconds: () -> Long = { System.currentTimeMillis() / 1000 },
) {
    fun states(): Flow<PairingState> = flow {
        emit(PairingState.Requesting)
        val started = try {
            api.requestPairing(deviceName)
        } catch (e: ApiException) {
            emit(PairingState.Failed(e.message))
            return@flow
        }
        emit(PairingState.Waiting(started.userCode, started.verifyUrl, started.expiresAt))
        val interval = started.interval.coerceIn(1, 60) * 1000L
        while (true) {
            delay(interval)
            val answer = try {
                api.pollPairing(started.pairingId, started.pollSecret)
            } catch (e: ApiException) {
                when {
                    // Unknown or purged: this request is over; start again.
                    e.isNotFound -> {
                        emit(PairingState.Expired)
                        return@flow
                    }
                    e.isNetwork || e.status == 429 || e.status >= 500 -> {
                        if (nowSeconds() > started.expiresAt) {
                            emit(PairingState.Expired)
                            return@flow
                        }
                        continue
                    }
                    else -> {
                        emit(PairingState.Failed(e.message))
                        return@flow
                    }
                }
            }
            when (answer.status) {
                "pending" -> continue
                "approved" -> {
                    val token = answer.token
                    if (token.isNullOrEmpty()) emit(PairingState.Expired) else emit(PairingState.Approved(token, answer.scopes))
                    return@flow
                }
                "denied" -> {
                    emit(PairingState.Denied)
                    return@flow
                }
                else -> {
                    emit(PairingState.Expired)
                    return@flow
                }
            }
        }
    }
}
