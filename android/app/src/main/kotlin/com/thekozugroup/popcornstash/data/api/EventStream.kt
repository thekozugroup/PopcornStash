package com.thekozugroup.popcornstash.data.api

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.callbackFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import okhttp3.OkHttpClient
import okhttp3.Response
import okhttp3.sse.EventSource
import okhttp3.sse.EventSourceListener
import okhttp3.sse.EventSources
import java.util.concurrent.TimeUnit

/** One event from `GET /api/events` (docs/architecture.md section 8), already filtered for this person by the server. */
sealed interface StashEvent {
    /** A download changed; the same shape `GET /api/downloads` lists. */
    data class DownloadChanged(val download: Download) : StashEvent

    /** The download left this person's view: drop its row, say nothing. */
    data class DownloadRemoved(val id: String) : StashEvent

    /** An item was added, changed or removed. A removal never names a library. */
    data class LibraryChanged(val type: String, val itemId: String, val libraryId: String?) : StashEvent

    /** This person's list of libraries changed: fetch `GET /api/libraries` (and the page shown) again. */
    data object LibrariesChanged : StashEvent

    data class VpnChanged(val status: VpnStatus) : StashEvent

    /** Waiting files this person can see. */
    data class InboxCount(val count: Long) : StashEvent

    data class JobChanged(val kind: String, val state: String, val itemId: String?) : StashEvent
}

@Serializable
internal data class LibraryEventBody(val type: String, val itemId: String, val libraryId: String? = null)

@Serializable
internal data class InboxEventBody(val count: Long)

@Serializable
internal data class JobEventBody(val kind: String = "", val state: String = "", val itemId: String? = null)

/** Turns one server-sent event into a [StashEvent]; null for an event this app does not know. */
fun parseStashEvent(name: String?, data: String): StashEvent? = runCatching {
    val json = StashJson
    when (name) {
        "download" -> {
            val obj = json.parseToJsonElement(data).jsonObject
            if (obj["removed"]?.jsonPrimitive?.content == "true") {
                StashEvent.DownloadRemoved(obj.getValue("id").jsonPrimitive.content)
            } else {
                StashEvent.DownloadChanged(json.decodeFromJsonElement(Download.serializer(), obj))
            }
        }
        "library" -> json.decodeFromString(LibraryEventBody.serializer(), data)
            .let { StashEvent.LibraryChanged(it.type, it.itemId, it.libraryId) }
        "libraries" -> StashEvent.LibrariesChanged
        "vpn" -> StashEvent.VpnChanged(json.decodeFromString(VpnStatus.serializer(), data))
        "inbox" -> StashEvent.InboxCount(json.decodeFromString(InboxEventBody.serializer(), data).count)
        "job" -> json.decodeFromString(JobEventBody.serializer(), data).let { StashEvent.JobChanged(it.kind, it.state, it.itemId) }
        else -> null
    }
}.getOrNull()

/** What one connection attempt ended with. */
internal sealed interface StreamEnd {
    /** The connection had opened; reconnecting starts again from the shortest wait. */
    data object AfterOpen : StreamEnd

    data object NeverOpened : StreamEnd

    /** The token no longer works: stop and let the session sign the person out. */
    data object Unauthorized : StreamEnd
}

/**
 * The live events stream over OkHttp's SSE client with the session's bearer client
 * (docs/architecture.md section 10, Events): open while the app is in the foreground,
 * reconnecting after 1, 2, 5, 10, then every 30 seconds. Collect [events]; call [start]
 * when the app comes to the foreground and [stop] when it leaves.
 */
class EventStream(
    private val api: ApiClient,
    sessionClient: OkHttpClient,
    private val scope: CoroutineScope,
    /** Waits before reconnecting: 1, 2, 5, 10, then every 30 seconds (docs/architecture.md section 10). */
    private val backoffMillis: List<Long> = listOf(1_000, 2_000, 5_000, 10_000, 30_000),
) {
    enum class Status { Stopped, Connecting, Connected, Waiting }

    private val client = sessionClient.newBuilder().readTimeout(0, TimeUnit.MILLISECONDS).build()
    private val _events = MutableSharedFlow<StashEvent>(extraBufferCapacity = 256)
    val events: SharedFlow<StashEvent> = _events.asSharedFlow()

    private val _status = MutableStateFlow(Status.Stopped)
    val status: StateFlow<Status> = _status.asStateFlow()

    private var job: Job? = null

    fun start() {
        if (job?.isActive == true) return
        job = scope.launch {
            var failures = 0
            while (isActive) {
                _status.value = Status.Connecting
                when (connectOnce()) {
                    StreamEnd.Unauthorized -> break
                    StreamEnd.AfterOpen -> failures = 0
                    StreamEnd.NeverOpened -> Unit
                }
                _status.value = Status.Waiting
                delay(waitBefore(failures))
                failures++
            }
            _status.value = Status.Stopped
        }
    }

    /** How long to wait before the next try, after [failures] tries in a row that did not open. */
    internal fun waitBefore(failures: Int): Long = backoffMillis[failures.coerceIn(0, backoffMillis.lastIndex)]

    fun stop() {
        job?.cancel()
        job = null
        _status.value = Status.Stopped
    }

    private suspend fun connectOnce(): StreamEnd {
        var opened = false
        var unauthorized = false
        connection().collect { signal ->
            when (signal) {
                is Signal.Open -> {
                    opened = true
                    _status.value = Status.Connected
                }
                is Signal.Event -> _events.emit(signal.event)
                is Signal.Unauthorized -> unauthorized = true
            }
        }
        return when {
            unauthorized -> StreamEnd.Unauthorized
            opened -> StreamEnd.AfterOpen
            else -> StreamEnd.NeverOpened
        }
    }

    private sealed interface Signal {
        data object Open : Signal
        data class Event(val event: StashEvent) : Signal
        data object Unauthorized : Signal
    }

    private fun connection(): Flow<Signal> = callbackFlow {
        val source = EventSources.createFactory(client).newEventSource(
            api.eventsRequest(),
            object : EventSourceListener() {
                override fun onOpen(eventSource: EventSource, response: Response) {
                    trySend(Signal.Open)
                }

                override fun onEvent(eventSource: EventSource, id: String?, type: String?, data: String) {
                    parseStashEvent(type, data)?.let { trySend(Signal.Event(it)) }
                }

                override fun onClosed(eventSource: EventSource) {
                    close()
                }

                override fun onFailure(eventSource: EventSource, t: Throwable?, response: Response?) {
                    if (response?.code == 401) trySend(Signal.Unauthorized)
                    close()
                }
            },
        )
        awaitClose { source.cancel() }
    }
}
