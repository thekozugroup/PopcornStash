package com.thekozugroup.popcornstash.feature.library

import com.thekozugroup.popcornstash.data.api.EventStream.Status
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.toList
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Test

/** When pages load again after a gap in the live events, and when they say the server is out of reach. */
@OptIn(ExperimentalCoroutinesApi::class)
class LiveUpdatesTest {
    @Test
    fun aReconnectAfterAGapIsSignalledOnce() = runTest {
        val signals = flowOf(
            Status.Connecting, Status.Connected, // the first connection is no gap
            Status.Waiting, Status.Connecting, Status.Waiting, Status.Connecting, Status.Connected, // back after failures
            Status.Stopped, Status.Connecting, Status.Connected, // back from the background
        ).reconnects().toList()
        assertEquals(2, signals.size)
    }

    @Test
    fun theConnectionCountsAsLostOnlyAfterTheGraceWithoutABreak() = runTest {
        val status = MutableStateFlow(Status.Connected)
        val seen = mutableListOf<Boolean>()
        val job = launch { status.connectionLost(graceMillis = 3_000).collect { seen += it } }
        runCurrent()
        assertEquals(listOf(false), seen)

        // Failing and retrying flips between Waiting and Connecting; that is one gap.
        status.value = Status.Waiting
        advanceTimeBy(1_000)
        status.value = Status.Connecting
        advanceTimeBy(1_000)
        status.value = Status.Waiting
        runCurrent()
        assertEquals(listOf(false), seen)
        advanceTimeBy(1_100)
        assertEquals(listOf(false, true), seen)

        status.value = Status.Connected
        runCurrent()
        assertEquals(listOf(false, true, false), seen)

        // The app in the background is not a lost connection.
        status.value = Status.Stopped
        advanceTimeBy(10_000)
        assertEquals(listOf(false, true, false), seen)
        job.cancel()
    }
}
