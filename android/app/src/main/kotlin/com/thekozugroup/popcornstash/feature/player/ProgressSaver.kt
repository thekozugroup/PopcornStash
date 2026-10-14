package com.thekozugroup.popcornstash.feature.player

import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlin.math.abs
import kotlin.math.roundToLong

/** Why the player reports where it is (docs/architecture.md section 10, Item and player). */
enum class SaveReason {
    /** Every 10 seconds while playing. */
    Interval,

    /** The person paused. */
    Pause,

    /** The player is leaving, going to the background, or switching stream. */
    Leave,

    /** The video ended: saved as watched to the end. */
    Ended,
}

/**
 * Saves the playback position with `PUT /api/items/:id/progress`, so "Continue watching"
 * and resume stay right across devices.
 *
 * - Nothing is saved before there is a position and a length (a stream that never started
 *   must not overwrite the resume point with 0).
 * - A save that would move the position by less than a second is skipped, except at the end.
 * - At the end the position is the length, so the server counts it as watched.
 * - Positions are rounded to a tenth of a second, as the web does.
 * - Saves go one at a time, in the order asked.
 *
 * [send] throws when the server refuses; the caller decides what that means (a 404 means
 * the video is gone). A failed save is not remembered, so the next one tries again.
 */
class ProgressSaver(private val send: suspend (positionS: Double, durationS: Double) -> Unit) {
    private val mutex = Mutex()
    private var lastSaved = -1.0

    /** The position last saved, or -1 when nothing was saved yet. */
    val lastSavedPosition: Double get() = lastSaved

    /** Returns true when a save went to the server. */
    suspend fun save(positionS: Double, durationS: Double, reason: SaveReason): Boolean = mutex.withLock {
        if (!durationS.isFinite() || durationS <= 0.0) return@withLock false
        val position = when (reason) {
            SaveReason.Ended -> durationS
            else -> positionS.takeIf { it.isFinite() && it > 0.0 }?.coerceAtMost(durationS) ?: return@withLock false
        }
        val rounded = tenth(position)
        if (reason != SaveReason.Ended && lastSaved >= 0 && abs(rounded - lastSaved) < 1.0) return@withLock false
        if (reason == SaveReason.Ended && rounded == lastSaved) return@withLock false
        send(rounded, tenth(durationS))
        lastSaved = rounded
        true
    }

    private fun tenth(value: Double): Double = (value * 10).roundToLong() / 10.0
}
