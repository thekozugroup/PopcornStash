package com.thekozugroup.popcornstash.data.session

import com.thekozugroup.popcornstash.data.api.ApiException
import com.thekozugroup.popcornstash.data.api.Role
import com.thekozugroup.popcornstash.data.api.User
import com.thekozugroup.popcornstash.data.api.baseHttpClient
import com.thekozugroup.popcornstash.data.auth.SessionStore
import com.thekozugroup.popcornstash.data.auth.SoftwareCipher
import com.thekozugroup.popcornstash.data.auth.StoredSession
import com.thekozugroup.popcornstash.testing.Fixtures
import com.thekozugroup.popcornstash.testing.next
import com.thekozugroup.popcornstash.testing.path
import com.thekozugroup.popcornstash.testing.respond
import com.thekozugroup.popcornstash.testing.withinRealSeconds
import com.thekozugroup.popcornstash.testing.respondEmpty
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.runTest
import mockwebserver3.MockWebServer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder

@OptIn(ExperimentalCoroutinesApi::class)
class SessionRepositoryTest {
    @get:Rule
    val folder = TemporaryFolder()

    private val server = MockWebServer()
    private val cipher = SoftwareCipher()
    private lateinit var store: SessionStore
    private var cleaned = 0
    private val memory = object : ServerMemory {
        override var lastServer: String? = null
    }

    @Before
    fun setUp() {
        server.start()
        store = SessionStore(folder.root, cipher)
    }

    @After
    fun tearDown() = server.close()

    private fun TestScope.repository(): SessionRepository {
        val dispatcher = UnconfinedTestDispatcher(testScheduler)
        return SessionRepository(
            store = store,
            memory = memory,
            baseClient = baseHttpClient("t"),
            appScope = CoroutineScope(dispatcher),
            sessionContext = dispatcher,
            cleaners = listOf({ cleaned++ }),
        )
    }

    private suspend fun SessionRepository.awaitSignedOut(): SessionState.SignedOut =
        withinRealSeconds { state.first { it is SessionState.SignedOut } as SessionState.SignedOut }

    @Test
    fun signingInChecksTheTokenKeepsItAndOpensTheSession() = runTest {
        val repository = repository()
        server.respond(Fixtures.OWNER)
        val session = repository.signIn(server.url("/"), Fixtures.TOKEN)
        assertEquals("Ana Ribeiro", session.user.value.displayName)
        assertEquals(SessionState.SignedIn(session), repository.state.value)
        val me = server.next()
        assertEquals("/api/me", me.path)
        assertEquals("Bearer ${Fixtures.TOKEN}", me.headers["Authorization"])
        assertEquals(Fixtures.TOKEN, store.load()!!.token)
        assertEquals(server.url("/").toString(), memory.lastServer)
    }

    @Test
    fun aTokenTheServerRefusesIsNotKept() = runTest {
        val repository = repository()
        repository.restore()
        server.respond("""{"error":{"code":"unauthorized","message":"Sign in first."}}""", code = 401)
        try {
            repository.signIn(server.url("/"), Fixtures.TOKEN)
            fail("expected the sign-in to fail")
        } catch (e: ApiException) {
            assertEquals(401, e.status)
        }
        assertNull(store.load())
        assertTrue(repository.state.value is SessionState.SignedOut)
    }

    @Test
    fun startsSignedInFromTheKeptSessionAndRefreshesThePerson() = runTest {
        store.save(StoredSession(server.url("/").toString(), Fixtures.TOKEN, User("u-owner", "Ana", Role.Owner)))
        server.respond(Fixtures.OWNER)
        val repository = repository()
        repository.restore()
        val session = (repository.state.value as SessionState.SignedIn).session
        withinRealSeconds { session.user.first { it.displayName == "Ana Ribeiro" } }
        assertEquals("Ana Ribeiro", store.load()!!.user.displayName)
    }

    @Test
    fun startsSignedOutWithNothingKept() = runTest {
        memory.lastServer = "https://stash.example.com/"
        val repository = repository()
        repository.restore()
        assertEquals("https://stash.example.com/", (repository.state.value as SessionState.SignedOut).lastServer)
    }

    @Test
    fun a401AnywhereEndsTheSessionAndForgetsEverything() = runTest {
        val repository = repository()
        server.respond(Fixtures.OWNER)
        val session = repository.signIn(server.url("/"), Fixtures.TOKEN)
        val keysBefore = cipher.destroyed
        val cleanedBefore = cleaned
        server.respond("""{"error":{"code":"unauthorized","message":"Sign in first."}}""", code = 401)
        runCatching { session.api.home() }
        val signedOut = repository.awaitSignedOut()
        assertEquals("You were signed out. Sign in again.", signedOut.notice)
        assertNull(store.load())
        assertEquals("the Keystore key is deleted", keysBefore + 1, cipher.destroyed)
        assertEquals("the caches are cleared", cleanedBefore + 1, cleaned)
    }

    @Test
    fun signingOutTellsTheServerAndForgetsEverything() = runTest {
        val repository = repository()
        server.respond(Fixtures.OWNER)
        repository.signIn(server.url("/"), Fixtures.TOKEN)
        val cleanedBefore = cleaned
        server.respondEmpty()
        withContext(Dispatchers.Default) { repository.signOut() }
        server.next()
        val logout = server.next()
        assertEquals("POST /api/auth/logout", "${logout.method} ${logout.path}")
        assertEquals("Bearer ${Fixtures.TOKEN}", logout.headers["Authorization"])
        val state = repository.state.value as SessionState.SignedOut
        assertNull(state.notice)
        assertNull(store.load())
        assertEquals(cleanedBefore + 1, cleaned)
    }

    @Test
    fun signingOutWithoutTheServerStillForgetsEverything() = runTest {
        val repository = repository()
        server.respond(Fixtures.OWNER)
        repository.signIn(server.url("/"), Fixtures.TOKEN)
        server.close()
        withContext(Dispatchers.Default) { repository.signOut() }
        assertTrue(repository.state.value is SessionState.SignedOut)
        assertNull(store.load())
    }

    @Test
    fun anOldSessionsLate401DoesNotSignOutTheNextPerson() = runTest {
        val repository = repository()
        server.respond(Fixtures.OWNER)
        val first = repository.signIn(server.url("/"), Fixtures.TOKEN)
        server.respond(Fixtures.MEMBER)
        val second = repository.signIn(server.url("/"), "pst_second")
        server.respond("""{"error":{"code":"unauthorized","message":"Sign in first."}}""", code = 401)
        runCatching { first.api.home() }
        assertEquals(SessionState.SignedIn(second), repository.state.value)
        assertEquals("Tomas", second.user.value.displayName)
        assertNotNull(store.load())
    }

    @Test
    fun liveEventsRunOnlyInTheForeground() = runTest {
        val repository = repository()
        server.respond(Fixtures.OWNER)
        val session = repository.signIn(server.url("/"), Fixtures.TOKEN)
        server.next()
        server.respond("", headers = arrayOf("Content-Type" to "text/event-stream"))
        repository.setForeground(true)
        val events = server.next()
        assertEquals("/api/events", events.path)
        assertEquals("Bearer ${Fixtures.TOKEN}", events.headers["Authorization"])
        repository.setForeground(false)
        assertEquals(com.thekozugroup.popcornstash.data.api.EventStream.Status.Stopped, session.events.status.value)
    }
}
