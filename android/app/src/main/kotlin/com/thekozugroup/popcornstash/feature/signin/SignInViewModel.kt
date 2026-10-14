package com.thekozugroup.popcornstash.feature.signin

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.ApiException
import com.thekozugroup.popcornstash.data.auth.PairingFlow
import com.thekozugroup.popcornstash.data.auth.PairingState
import com.thekozugroup.popcornstash.data.auth.PasskeyAuthenticator
import com.thekozugroup.popcornstash.data.auth.PasskeyException
import com.thekozugroup.popcornstash.data.auth.PasskeySignIn
import com.thekozugroup.popcornstash.data.auth.ServerAddress
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import okhttp3.HttpUrl
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull

/** The server the person chose, and how it lets this app sign in. */
data class ServerInfo(
    val url: HttpUrl,
    val display: String,
    val version: String,
    /** The server lists this app for passkeys and has passkeys ready. */
    val passkeysOffered: Boolean,
    /** Nobody has set the server up yet (no owner passkey). */
    val setupRequired: Boolean,
)

enum class SignInStep { Server, Method, Pairing }

data class SignInUiState(
    val step: SignInStep = SignInStep.Server,
    val address: String = "",
    val addressError: String? = null,
    val checking: Boolean = false,
    /** Why sign-in shows ("You were signed out. Sign in again."). */
    val notice: String? = null,
    val server: ServerInfo? = null,
    val pairing: PairingState? = null,
    val passkeyBusy: Boolean = false,
    val methodError: String? = null,
)

/** What sign-in needs from the app; the tests give it a MockWebServer and a fake session. */
class SignInDependencies(
    /** An API client without a token, for the public calls. */
    val publicApi: (HttpUrl) -> ApiClient,
    /** Keeps the token and opens the session (SessionRepository.signIn). */
    val finishSignIn: suspend (server: HttpUrl, token: String) -> Unit,
    val deviceName: String,
    val packageName: String,
    /** Debug builds accept http:// for this phone and the emulator's host. */
    val allowLocalHttp: Boolean,
)

/**
 * Sign-in (android/DESIGN.md, Sign-in; docs/architecture.md section 10): the server address,
 * then a passkey through Credential Manager or pairing with a code.
 */
class SignInController(
    private val deps: SignInDependencies,
    private val scope: CoroutineScope,
    lastServer: String? = null,
    notice: String? = null,
) {
    private val _state = MutableStateFlow(
        SignInUiState(address = lastServer?.toHttpUrlOrNull()?.let(ServerAddress::display).orEmpty(), notice = notice),
    )
    val state: StateFlow<SignInUiState> = _state.asStateFlow()

    private var work: Job? = null

    fun onAddressChange(text: String) = _state.update { it.copy(address = text, addressError = null) }

    /** Checks the address: `GET /api/health`, `/api/auth/state` and the server's assetlinks. */
    fun onContinue() {
        if (_state.value.checking) return
        val parsed = ServerAddress.parse(_state.value.address, deps.allowLocalHttp)
        if (parsed is ServerAddress.Result.Invalid) {
            _state.update { it.copy(addressError = parsed.message) }
            return
        }
        val url = (parsed as ServerAddress.Result.Valid).url
        _state.update { it.copy(checking = true, addressError = null, notice = null) }
        work = scope.launch {
            val api = deps.publicApi(url)
            val info = try {
                val health = api.health()
                if (health.status != "ok") throw ApiException.unexpected(200)
                val auth = api.authState()
                val listed = auth.passkeysReady && api.listsAppForPasskeys(deps.packageName)
                ServerInfo(url, ServerAddress.display(url), health.version, listed, auth.setupRequired)
            } catch (_: ApiException) {
                _state.update { it.copy(checking = false, addressError = "PopcornStash did not answer at this address.") }
                return@launch
            }
            _state.update { it.copy(checking = false, server = info, step = SignInStep.Method, methodError = null) }
        }
    }

    /** Looks at the server again (after the owner finished setting it up in a browser). */
    fun onCheckAgain() {
        _state.update { it.copy(step = SignInStep.Server) }
        onContinue()
    }

    fun onPair() {
        val server = _state.value.server ?: return
        work?.cancel()
        _state.update { it.copy(step = SignInStep.Pairing, pairing = PairingState.Requesting, methodError = null) }
        work = scope.launch {
            PairingFlow(deps.publicApi(server.url), deps.deviceName).states().collect { pairing ->
                if (pairing is PairingState.Approved) {
                    finish(server, pairing.token) { message ->
                        _state.update { it.copy(pairing = PairingState.Failed(message)) }
                    }
                } else {
                    _state.update { it.copy(pairing = pairing) }
                }
            }
        }
    }

    fun onPasskey(authenticator: PasskeyAuthenticator) {
        val server = _state.value.server ?: return
        if (_state.value.passkeyBusy) return
        _state.update { it.copy(passkeyBusy = true, methodError = null) }
        work = scope.launch {
            val token = try {
                PasskeySignIn(deps.publicApi(server.url), authenticator, deps.deviceName).signIn().token!!
            } catch (e: PasskeyException) {
                _state.update { it.copy(passkeyBusy = false, methodError = if (e.cancelled) null else e.message) }
                return@launch
            } catch (e: ApiException) {
                _state.update { it.copy(passkeyBusy = false, methodError = e.message) }
                return@launch
            }
            finish(server, token) { message -> _state.update { it.copy(passkeyBusy = false, methodError = message) } }
        }
    }

    /** Back one step; from pairing this abandons the code. */
    fun onBack(): Boolean {
        val step = _state.value.step
        if (step == SignInStep.Server) return false
        work?.cancel()
        _state.update {
            when (step) {
                SignInStep.Pairing -> it.copy(step = SignInStep.Method, pairing = null)
                else -> it.copy(step = SignInStep.Server, server = null, passkeyBusy = false, methodError = null)
            }
        }
        return true
    }

    private suspend fun finish(server: ServerInfo, token: String, failed: (String) -> Unit) {
        try {
            deps.finishSignIn(server.url, token)
        } catch (e: ApiException) {
            failed(e.message)
        }
    }
}

/** Keeps sign-in (and a pairing in progress) through rotation. */
class SignInViewModel(deps: SignInDependencies, lastServer: String?, notice: String?) : ViewModel() {
    val controller = SignInController(deps, viewModelScope, lastServer, notice)
}
