package com.thekozugroup.popcornstash.feature.player

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch

/**
 * The HLS conversion the player holds on the server (`POST /api/items/:id/playback` with
 * `mode: "hls"`), and the promise to end it: every session the player is given is ended with
 * `DELETE /api/stream/:session` once the player no longer plays it, so the server's ffmpeg
 * stops at once instead of running until the idle reaper finds it.
 *
 * - [adopt] when a stream starts playing: the session played before is ended.
 * - [discard] for an answer that came too late (the person moved on while it was on its way).
 * - [close] when the player leaves: the current session is ended, and so is any that
 *   arrives afterwards.
 *
 * [stop] runs on [scope], which outlives the player (the signed-in session's scope), so the
 * request still goes out after the screen is gone. Failures are ignored: the server ends
 * idle sessions by itself within a minute.
 */
class StreamSessions(private val scope: CoroutineScope, private val stop: suspend (sessionId: String) -> Unit) {
    private var current: String? = null
    private var closed = false

    /** The session now playing, if it is a conversion. */
    val active: String? @Synchronized get() = current

    @get:Synchronized
    val isClosed: Boolean get() = closed

    /** The player now plays [sessionId] (null for direct play); the one before is ended. */
    @Synchronized
    fun adopt(sessionId: String?) {
        if (closed) {
            sessionId?.let(::end)
            return
        }
        val previous = current
        current = sessionId
        if (previous != null && previous != sessionId) end(previous)
    }

    /** A session the player will not play: ended at once. */
    @Synchronized
    fun discard(sessionId: String?) {
        if (sessionId != null && sessionId != current) end(sessionId)
    }

    /** The player leaves: ends what it holds, and anything adopted later. */
    @Synchronized
    fun close() {
        if (closed) return
        closed = true
        current?.let(::end)
        current = null
    }

    private fun end(sessionId: String) {
        scope.launch { runCatching { stop(sessionId) } }
    }
}
