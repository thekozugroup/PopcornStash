package com.thekozugroup.popcornstash.data.auth

import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.baseHttpClient
import com.thekozugroup.popcornstash.testing.Fixtures
import com.thekozugroup.popcornstash.testing.bodyText
import com.thekozugroup.popcornstash.testing.next
import com.thekozugroup.popcornstash.testing.respond
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.toList
import kotlinx.coroutines.test.currentTime
import kotlinx.coroutines.test.runTest
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import mockwebserver3.SocketEffect
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

/** The pairing state machine against MockWebServer (docs/architecture.md section 10, Pairing). */
@OptIn(ExperimentalCoroutinesApi::class)
class PairingFlowTest {
    private val server = MockWebServer()
    private lateinit var api: ApiClient

    @Before
    fun setUp() {
        server.start()
        api = ApiClient(server.url("/"), baseHttpClient("t"))
    }

    @After
    fun tearDown() = server.close()

    private fun started(interval: Int = 6, expiresAt: Long = 4_000_000_000) = server.respond(
        """{"pairing_id":"p-1","user_code":"K7QX-M2PD","poll_secret":"s3cret","expires_at":$expiresAt,
        "verify_url":"https://stash.example.com/pair?code=K7QX-M2PD","interval":$interval}""",
    )

    private val waiting = PairingState.Waiting("K7QX-M2PD", "https://stash.example.com/pair?code=K7QX-M2PD", 4_000_000_000)

    @Test
    fun showsTheCodeThenPollsEveryIntervalUntilApproved() = runTest {
        started(interval = 6)
        server.respond("""{"status":"pending"}""", code = 202)
        server.respond("""{"status":"pending"}""", code = 202)
        server.respond("""{"status":"approved","token":"${Fixtures.TOKEN}","scopes":["full"],"server_name":"stash.example.com"}""")
        val states = PairingFlow(api, "Ana's Pixel").states().toList()
        assertEquals(
            listOf(PairingState.Requesting, waiting, PairingState.Approved(Fixtures.TOKEN, listOf("full"))),
            states,
        )
        // Three polls, six seconds apart, the first one six seconds after the code showed.
        assertEquals(18_000, currentTime)
        val request = server.next()
        assertTrue(request.bodyText.contains("\"client_kind\":\"android\""))
        assertTrue(request.bodyText.contains("\"client_name\":\"Ana's Pixel\""))
        repeat(3) { assertEquals("/api/pairing/poll", server.next().url.encodedPath) }
        assertFalse("the token is never printed", states.last().toString().contains(Fixtures.TOKEN))
    }

    @Test
    fun aTurnedDownRequestEndsAsDenied() = runTest {
        started()
        server.respond("""{"status":"denied"}""", code = 410)
        assertEquals(PairingState.Denied, PairingFlow(api, "Pixel").states().toList().last())
    }

    @Test
    fun anExpiredCodeEndsAsExpired() = runTest {
        started()
        server.respond("""{"status":"pending"}""", code = 202)
        server.respond("""{"status":"expired"}""", code = 410)
        assertEquals(PairingState.Expired, PairingFlow(api, "Pixel").states().toList().last())
    }

    @Test
    fun anUnknownOrPurgedRequestStartsAgain() = runTest {
        started()
        server.respond("""{"error":{"code":"not_found","message":"That pairing request does not exist. Start pairing again."}}""", code = 404)
        assertEquals(PairingState.Expired, PairingFlow(api, "Pixel").states().toList().last())
    }

    @Test
    fun pollsThatDoNotGetThroughAreTriedAgain() = runTest {
        started()
        server.enqueue(MockResponse.Builder().onResponseStart(SocketEffect.ShutdownConnection).build())
        server.respond("""{"error":{"code":"rate_limited","message":"Too many tries."}}""", code = 429)
        server.respond("<html>Bad gateway</html>", code = 502)
        server.respond("""{"status":"approved","token":"${Fixtures.TOKEN}","scopes":["full"]}""")
        val states = PairingFlow(api, "Pixel").states().toList()
        assertEquals(PairingState.Approved(Fixtures.TOKEN, listOf("full")), states.last())
        assertEquals(listOf(PairingState.Requesting, waiting), states.dropLast(1))
    }

    @Test
    fun stopsTryingOnceTheCodeHasExpired() = runTest {
        started(expiresAt = 1_000)
        server.respond("<html>Bad gateway</html>", code = 502)
        val states = PairingFlow(api, "Pixel", nowSeconds = { 2_000 }).states().toList()
        assertEquals(PairingState.Expired, states.last())
    }

    @Test
    fun aRequestThatCannotStartSaysWhy() = runTest {
        server.respond("""{"error":{"code":"rate_limited","message":"Too many tries. Wait a minute and try again."}}""", code = 429)
        assertEquals(
            listOf(PairingState.Requesting, PairingState.Failed("Too many tries. Wait a minute and try again.")),
            PairingFlow(api, "Pixel").states().toList(),
        )
    }

    @Test
    fun anApprovalWithoutATokenCountsAsExpired() = runTest {
        started()
        server.respond("""{"status":"approved","scopes":[]}""")
        assertEquals(PairingState.Expired, PairingFlow(api, "Pixel").states().toList().last())
    }
}
