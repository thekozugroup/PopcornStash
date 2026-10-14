package com.thekozugroup.popcornstash.feature.player

import com.thekozugroup.popcornstash.data.api.ApiException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class StreamSessionsTest {
    private val stopped = mutableListOf<String>()

    private fun TestScope.sessions(fail: Boolean = false) =
        StreamSessions(CoroutineScope(SupervisorJob() + StandardTestDispatcher(testScheduler))) { id ->
            if (fail) throw ApiException(404, "not_found", "That no longer exists.")
            stopped += id
        }

    @Test
    fun theSessionPlayedBeforeIsEndedWhenAnotherStarts() = runTest {
        val sessions = sessions()
        sessions.adopt("s-1")
        sessions.adopt("s-1")
        runCurrent()
        assertTrue("the same session again ends nothing", stopped.isEmpty())
        sessions.adopt("s-2")
        sessions.adopt(null)
        runCurrent()
        assertEquals(listOf("s-1", "s-2"), stopped)
        assertNull(sessions.active)
    }

    @Test
    fun leavingEndsWhatIsHeldOnceAndAnythingThatArrivesLater() = runTest {
        val sessions = sessions()
        sessions.adopt("s-1")
        sessions.close()
        sessions.close()
        runCurrent()
        assertEquals(listOf("s-1"), stopped)
        sessions.adopt("s-late")
        runCurrent()
        assertEquals(listOf("s-1", "s-late"), stopped)
        assertTrue(sessions.isClosed)
        assertNull(sessions.active)
    }

    @Test
    fun aDiscardedAnswerIsEndedButThePlayingSessionIsKept() = runTest {
        val sessions = sessions()
        sessions.adopt("s-1")
        sessions.discard("s-stale")
        sessions.discard("s-1")
        sessions.discard(null)
        runCurrent()
        assertEquals(listOf("s-stale"), stopped)
        assertEquals("s-1", sessions.active)
    }

    @Test
    fun aFailureToEndIsIgnored() = runTest {
        val sessions = sessions(fail = true)
        sessions.adopt("s-1")
        sessions.close()
        runCurrent()
        assertTrue(stopped.isEmpty())
    }
}
