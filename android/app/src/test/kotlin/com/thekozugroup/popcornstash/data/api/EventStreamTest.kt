package com.thekozugroup.popcornstash.data.api

import com.thekozugroup.popcornstash.testing.Fixtures
import com.thekozugroup.popcornstash.testing.next
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.take
import kotlinx.coroutines.flow.toList
import kotlinx.coroutines.async
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Before
import org.junit.Test

class EventStreamTest {
    private val server = MockWebServer()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private var unauthorized = 0
    private lateinit var api: ApiClient
    private lateinit var http: okhttp3.OkHttpClient

    @Before
    fun setUp() {
        server.start()
        val base = server.url("/")
        http = baseHttpClient("t").newBuilder().addInterceptor(AuthInterceptor(base, { Fixtures.TOKEN }, { unauthorized++ })).build()
        api = ApiClient(base, http)
    }

    @After
    fun tearDown() {
        scope.cancel()
        server.close()
    }

    private fun sse(body: String) = MockResponse.Builder()
        .code(200)
        .addHeader("Content-Type", "text/event-stream")
        .body(body)
        .build()

    @Test
    fun readsEveryKindOfEventAndSkipsUnknownOnes() = runBlocking {
        server.enqueue(
            sse(
                """
                event: download
                data: ${Fixtures.DOWNLOAD.replace("\n", " ")}

                event: download
                data: {"id":"d-9","removed":true}

                event: library
                data: {"type":"removed","item_id":"i-1","library_id":null}

                event: future
                data: {"anything":1}

                event: inbox
                data: {"count":3}

                event: libraries
                data: {}

                event: vpn
                data: ${Fixtures.VPN.replace("\n", " ")}

                event: job
                data: {"kind":"scan","state":"done","item_id":null}


                """.trimIndent(),
            ),
        )
        server.enqueue(sse(""))
        val stream = EventStream(api, http, scope, backoffMillis = listOf(50))
        val events = async { withTimeout(5_000) { stream.events.take(7).toList() } }
        stream.start()
        val list = events.await()
        stream.stop()
        assertEquals("d-1", (list[0] as StashEvent.DownloadChanged).download.id)
        assertEquals(StashEvent.DownloadRemoved("d-9"), list[1])
        assertEquals(StashEvent.LibraryChanged("removed", "i-1", null), list[2])
        assertEquals(StashEvent.InboxCount(3), list[3])
        assertEquals(StashEvent.LibrariesChanged, list[4])
        assertEquals(VpnState.Connected, (list[5] as StashEvent.VpnChanged).status.state)
        assertEquals(StashEvent.JobChanged("scan", "done", null), list[6])
        val request = server.next()
        assertEquals("/api/events", request.url.encodedPath)
        assertEquals("Bearer ${Fixtures.TOKEN}", request.headers["Authorization"])
        assertEquals("text/event-stream", request.headers["Accept"])
    }

    @Test
    fun reconnectsWhenTheStreamEndsAndStopsForGoodOn401() = runBlocking {
        server.enqueue(sse("event: inbox\ndata: {\"count\":1}\n\n"))
        server.enqueue(MockResponse.Builder().code(502).build())
        server.enqueue(sse("event: inbox\ndata: {\"count\":2}\n\n"))
        server.enqueue(MockResponse.Builder().code(401).body("""{"error":{"code":"unauthorized","message":"Sign in first."}}""").build())
        val stream = EventStream(api, http, scope, backoffMillis = listOf(20, 40))
        val second = async { withTimeout(5_000) { stream.events.first { it == StashEvent.InboxCount(2) } } }
        stream.start()
        second.await()
        withTimeout(5_000) { stream.status.first { it == EventStream.Status.Stopped } }
        assertEquals(4, server.requestCount)
        assertEquals(1, unauthorized)
    }

    @Test
    fun waitsOneTwoFiveTenThenEveryThirtySeconds() {
        val stream = EventStream(api, http, scope)
        assertEquals(listOf(1_000L, 2_000L, 5_000L, 10_000L, 30_000L, 30_000L, 30_000L), (0..6).map { stream.waitBefore(it) })
    }

    @Test
    fun malformedEventsAreDropped() {
        assertNull(parseStashEvent("download", "{not json"))
        assertNull(parseStashEvent(null, "{}"))
    }

}
