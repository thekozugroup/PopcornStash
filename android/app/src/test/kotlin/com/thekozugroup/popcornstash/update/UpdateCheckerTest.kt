package com.thekozugroup.popcornstash.update

import com.thekozugroup.popcornstash.data.api.baseHttpClient
import com.thekozugroup.popcornstash.testing.next
import com.thekozugroup.popcornstash.testing.respond
import com.thekozugroup.popcornstash.update.UpdateFixtures.appRelease
import com.thekozugroup.popcornstash.update.UpdateFixtures.list
import com.thekozugroup.popcornstash.update.UpdateFixtures.serverRelease
import kotlinx.coroutines.test.runTest
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Before
import org.junit.Test

/** The checker against a MockWebServer that answers as GitHub does. */
class UpdateCheckerTest {
    private val server = MockWebServer()
    private val memory = InMemoryUpdateMemory()
    private var now = 1_760_000_000_000L
    private lateinit var checker: UpdateChecker

    @Before
    fun setUp() {
        server.start()
        checker = UpdateChecker(
            http = baseHttpClient("PopcornStash-Android/0.1.0+412"),
            memory = memory,
            releasesUrl = server.url("/repos/thekozugroup/PopcornStash/releases?per_page=30"),
            clock = { now },
        )
    }

    @After
    fun tearDown() = server.close()

    private val releases = list(serverRelease("0.2.0"), appRelease(415), appRelease(414))

    @Test
    fun aListFindsTheNewestBuildAndKeepsItsEtag() = runTest {
        server.respond(releases, 200, "ETag" to "W/\"abc123\"")
        val result = checker.check() as CheckResult.Answered
        assertEquals(415, result.newest?.number)
        assertEquals(now, result.checkedAt)
        assertEquals("W/\"abc123\"", memory.etag)
        assertEquals(415, memory.newestSeen?.number)
        assertEquals(now, memory.lastChecked)

        val request = server.next()
        assertEquals("GET", request.method)
        assertEquals("/repos/thekozugroup/PopcornStash/releases?per_page=30", request.target)
        assertEquals("application/vnd.github+json", request.headers["Accept"])
        // Nothing about the person or the phone: no login, and the app's name and version only.
        assertNull(request.headers["Authorization"])
        assertNull(request.headers["If-None-Match"])
        assertEquals("PopcornStash-Android/0.1.0+412", request.headers["User-Agent"])
    }

    @Test
    fun anUnchangedListCostsA304AndKeepsTheBuildItHad() = runTest {
        server.respond(releases, 200, "ETag" to "W/\"abc123\"")
        checker.check()
        server.next()

        now += 60_000
        server.enqueue(MockResponse.Builder().code(304).addHeader("ETag", "W/\"abc123\"").build())
        val result = checker.check() as CheckResult.Answered
        assertEquals(415, result.newest?.number)
        assertEquals(now, memory.lastChecked)
        assertEquals("W/\"abc123\"", server.next().headers["If-None-Match"])
    }

    @Test
    fun aNewListReplacesTheEtagAndTheBuild() = runTest {
        server.respond(releases, 200, "ETag" to "\"one\"")
        checker.check()
        server.respond(list(appRelease(416), appRelease(415)), 200, "ETag" to "\"two\"")
        assertEquals(416, (checker.check() as CheckResult.Answered).newest?.number)
        assertEquals("\"two\"", memory.etag)
        assertEquals(416, memory.newestSeen?.number)
    }

    @Test
    fun aListWithoutAppBuildsIsAnAnswerWithNone() = runTest {
        server.respond(list(serverRelease("0.2.0")))
        assertNull((checker.check() as CheckResult.Answered).newest)
    }

    @Test
    fun theHourlyLimitSaysWhenItResets() = runTest {
        server.respond(
            """{"message":"API rate limit exceeded for 203.0.113.9."}""",
            403,
            "x-ratelimit-limit" to "60",
            "x-ratelimit-remaining" to "0",
            "x-ratelimit-reset" to "1760003600",
        )
        assertEquals(CheckResult.Failed(CheckProblem.RateLimited(1_760_003_600_000L)), checker.check())
        // Nothing was learnt, so nothing is kept.
        assertNull(memory.etag)
        assertNull(memory.lastChecked)
    }

    @Test
    fun aSecondaryLimitWithRetryAfterIsALimitToo() = runTest {
        server.respond("""{"message":"You have exceeded a secondary rate limit."}""", 429, "Retry-After" to "120")
        assertEquals(CheckResult.Failed(CheckProblem.RateLimited(now + 120_000)), checker.check())
    }

    @Test
    fun aForbiddenAnswerThatIsNotTheLimitIsAnError() = runTest {
        server.respond("""{"message":"Forbidden"}""", 403, "x-ratelimit-remaining" to "42")
        assertEquals(CheckResult.Failed(CheckProblem.HttpError(403)), checker.check())
    }

    @Test
    fun serverErrorsAndNotFoundAreErrors() = runTest {
        server.respond("""{"message":"Server Error"}""", 502)
        assertEquals(CheckResult.Failed(CheckProblem.HttpError(502)), checker.check())
        server.respond("""{"message":"Not Found"}""", 404)
        assertEquals(CheckResult.Failed(CheckProblem.HttpError(404)), checker.check())
    }

    @Test
    fun aBodyThatIsNotAReleaseListIsUnreadable() = runTest {
        server.respond("<html>Unicorn!</html>")
        assertEquals(CheckResult.Failed(CheckProblem.Unreadable), checker.check())
        server.respond("""{"message":"Moved Permanently"}""")
        assertEquals(CheckResult.Failed(CheckProblem.Unreadable), checker.check())
    }

    @Test
    fun withNoConnectionItIsOffline() = runTest {
        server.close()
        assertEquals(CheckResult.Failed(CheckProblem.Offline), checker.check())
    }
}
