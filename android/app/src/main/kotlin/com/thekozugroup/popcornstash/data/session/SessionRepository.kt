package com.thekozugroup.popcornstash.data.session

import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.ApiException
import com.thekozugroup.popcornstash.data.api.AuthInterceptor
import com.thekozugroup.popcornstash.data.api.EventStream
import com.thekozugroup.popcornstash.data.api.User
import com.thekozugroup.popcornstash.data.auth.SessionStore
import com.thekozugroup.popcornstash.data.auth.StoredSession
import com.thekozugroup.popcornstash.data.repo.StashData
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import okhttp3.HttpUrl
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull
import okhttp3.OkHttpClient
import java.util.concurrent.atomic.AtomicLong
import kotlin.coroutines.CoroutineContext

/**
 * One signed-in person on one server. Everything that belongs to them hangs off it: the
 * bearer client, the API, the live events, the data the tabs show, and [scope]; all of it
 * ends with the session.
 */
class Session internal constructor(
    val id: Long,
    val server: HttpUrl,
    user: User,
    /** The client that adds this session's token; also for pictures and video. */
    val http: OkHttpClient,
    val api: ApiClient,
    val events: EventStream,
    val scope: CoroutineScope,
) {
    private val _user = MutableStateFlow(user)
    val user: StateFlow<User> = _user.asStateFlow()

    /** What the tabs show, kept current by the live events. */
    val data = StashData(api, events.events, scope)

    internal fun updateUser(user: User) {
        _user.value = user
    }

    override fun toString() = "Session(id=$id, server=$server, user=${user.value.id})"
}

sealed interface SessionState {
    /** Reading the kept session at start. */
    data object Restoring : SessionState

    /**
     * Sign-in shows; [lastServer] prefills the address, [notice] says why the person is here.
     * [generation] grows with every sign-out, so each sign-in starts from a clean page.
     */
    data class SignedOut(val lastServer: String?, val notice: String? = null, val generation: Long = 0) : SessionState

    data class SignedIn(val session: Session) : SessionState
}

/** The last server address typed, kept to prefill sign-in (it holds nothing about a person). */
interface ServerMemory {
    var lastServer: String?
}

/**
 * Who is signed in, and the only place a session starts or ends
 * (docs/architecture.md section 10, Token storage and Leaving a person's data behind).
 *
 * Ending a session, by signing out, by a 401 on any request, or by signing in as someone
 * else, cancels everything it ran, deletes the kept token and its Keystore key, and runs
 * [cleaners] (caches, private copies of opened files), so nothing of the person stays.
 */
class SessionRepository(
    private val store: SessionStore,
    private val memory: ServerMemory,
    private val baseClient: OkHttpClient,
    private val appScope: CoroutineScope,
    private val sessionContext: CoroutineContext,
    private val cleaners: List<() -> Unit> = emptyList(),
) {
    private val ids = AtomicLong()
    private val signOuts = AtomicLong()
    private val _state = MutableStateFlow<SessionState>(SessionState.Restoring)
    val state: StateFlow<SessionState> = _state.asStateFlow()

    @Volatile
    private var foreground = false

    val current: Session? get() = (state.value as? SessionState.SignedIn)?.session

    /** At start: picks up the kept session, then checks it with `GET /api/me` in the background. */
    fun restore() {
        val stored = store.load()
        val server = stored?.server?.toHttpUrlOrNull()
        if (stored == null || server == null) {
            _state.value = SessionState.SignedOut(memory.lastServer, generation = signOuts.get())
            return
        }
        val session = open(server, stored.token, stored.user)
        _state.value = SessionState.SignedIn(session)
        if (foreground) session.events.start()
        session.scope.launch {
            try {
                val me = session.api.me()
                session.updateUser(me)
                store.save(stored.copy(user = me))
            } catch (_: ApiException) {
                // Offline: keep going with what was kept. A 401 already ended the session.
            }
        }
    }

    /**
     * Finishes a sign-in with [token] from pairing or a passkey: checks it with `GET /api/me`
     * (unless the sign-in already named the person, [knownUser]), keeps it, and opens the
     * session. Any earlier session ends first.
     */
    suspend fun signIn(server: HttpUrl, token: String, knownUser: User? = null): Session {
        end(notice = null)
        val user = knownUser ?: authorizedApi(server, token).me()
        store.save(StoredSession(server.toString(), token, user))
        memory.lastServer = server.toString()
        val session = open(server, token, user)
        _state.value = SessionState.SignedIn(session)
        if (foreground) session.events.start()
        return session
    }

    /** Signs out on this phone: tells the server (briefly, best effort), then forgets everything. */
    suspend fun signOut() {
        val session = current ?: return
        withContext(NonCancellable) {
            withTimeoutOrNull(5_000) { runCatching { session.api.logout() } }
        }
        end(notice = null, onlyId = session.id)
    }

    /** The app came to the foreground or left it: the live events run only in the foreground. */
    fun setForeground(value: Boolean) {
        foreground = value
        val session = current ?: return
        if (value) session.events.start() else session.events.stop()
    }

    private fun onUnauthorized(sessionId: Long) {
        end(notice = "You were signed out. Sign in again.", onlyId = sessionId)
    }

    @Synchronized
    private fun end(notice: String?, onlyId: Long? = null) {
        val signedIn = state.value as? SessionState.SignedIn
        if (signedIn != null) {
            if (onlyId != null && signedIn.session.id != onlyId) return
            signedIn.session.events.stop()
            signedIn.session.scope.cancel()
        } else if (onlyId != null) {
            return
        }
        store.clear()
        cleaners.forEach { runCatching { it() } }
        _state.value = SessionState.SignedOut(memory.lastServer, notice, signOuts.incrementAndGet())
    }

    /** An API client with [token] that is not a session yet (checking a token before keeping it). */
    private fun authorizedApi(server: HttpUrl, token: String) =
        ApiClient(server, baseClient.newBuilder().addInterceptor(AuthInterceptor(server, { token }, {})).build())

    private fun open(server: HttpUrl, token: String, user: User): Session {
        val id = ids.incrementAndGet()
        val http = baseClient.newBuilder()
            .addInterceptor(AuthInterceptor(server, { token }, { appScope.launch { onUnauthorized(id) } }))
            .build()
        val api = ApiClient(server, http)
        val scope = CoroutineScope(SupervisorJob() + sessionContext)
        return Session(id, server, user, http, api, EventStream(api, http, scope), scope)
    }
}
