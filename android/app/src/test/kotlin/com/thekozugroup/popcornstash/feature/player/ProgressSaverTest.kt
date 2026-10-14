package com.thekozugroup.popcornstash.feature.player

import com.thekozugroup.popcornstash.data.api.ApiException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class ProgressSaverTest {
    private val sent = mutableListOf<Pair<Double, Double>>()
    private val saver = ProgressSaver { position, duration -> sent += position to duration }

    @Test
    fun nothingIsSavedBeforeThereIsAPositionAndALength() = runTest {
        assertFalse(saver.save(0.0, 3136.0, SaveReason.Interval))
        assertFalse(saver.save(12.0, 0.0, SaveReason.Pause))
        assertFalse(saver.save(Double.NaN, 3136.0, SaveReason.Leave))
        assertFalse(saver.save(12.0, Double.POSITIVE_INFINITY, SaveReason.Leave))
        assertTrue(sent.isEmpty())
    }

    @Test
    fun positionsAreRoundedToATenthAndKeptWithinTheLength() = runTest {
        saver.save(724.26, 3136.04, SaveReason.Interval)
        saver.save(4000.0, 3136.04, SaveReason.Leave)
        assertEquals(listOf(724.3 to 3136.0, 3136.0 to 3136.0), sent)
    }

    @Test
    fun aMoveOfLessThanASecondIsNotSavedAgain() = runTest {
        assertTrue(saver.save(100.0, 600.0, SaveReason.Interval))
        assertFalse(saver.save(100.6, 600.0, SaveReason.Pause))
        assertFalse(saver.save(99.2, 600.0, SaveReason.Leave))
        assertTrue(saver.save(101.0, 600.0, SaveReason.Leave))
        assertTrue("going back counts too", saver.save(40.0, 600.0, SaveReason.Interval))
        assertEquals(listOf(100.0, 101.0, 40.0), sent.map { it.first })
    }

    @Test
    fun theEndIsSavedAsTheWholeLengthOnce() = runTest {
        saver.save(595.5, 600.0, SaveReason.Interval)
        assertTrue(saver.save(599.7, 600.0, SaveReason.Ended))
        assertFalse(saver.save(600.0, 600.0, SaveReason.Ended))
        assertFalse("leaving right after the end changes nothing", saver.save(600.0, 600.0, SaveReason.Leave))
        assertEquals(listOf(595.5 to 600.0, 600.0 to 600.0), sent)
        assertEquals(600.0, saver.lastSavedPosition, 0.0)
    }

    @Test
    fun aFailedSaveIsNotRememberedSoTheNextOneTriesAgain() = runTest {
        var fail = true
        val flaky = ProgressSaver { position, duration ->
            if (fail) throw ApiException.network()
            sent += position to duration
        }
        val error = runCatching { flaky.save(50.0, 600.0, SaveReason.Pause) }.exceptionOrNull()
        assertTrue(error is ApiException)
        fail = false
        assertTrue(flaky.save(50.0, 600.0, SaveReason.Leave))
        assertEquals(listOf(50.0 to 600.0), sent)
    }

    @Test
    fun savesGoOneAtATimeInTheOrderAsked() = runTest {
        val gate = CompletableDeferred<Unit>()
        val order = mutableListOf<Double>()
        val slow = ProgressSaver { position, _ ->
            if (order.isEmpty()) gate.await()
            order += position
        }
        launch { slow.save(10.0, 600.0, SaveReason.Interval) }
        runCurrent()
        launch { slow.save(20.0, 600.0, SaveReason.Pause) }
        runCurrent()
        assertTrue("the second waits for the first", order.isEmpty())
        gate.complete(Unit)
        runCurrent()
        assertEquals(listOf(10.0, 20.0), order)
    }
}
