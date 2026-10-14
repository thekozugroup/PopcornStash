package com.thekozugroup.popcornstash.feature.player

import okhttp3.HttpUrl

/** What the player plays: the original file (direct play) or the server's HLS playlist. */
data class StreamSource(val url: HttpUrl, val hls: Boolean)

/** Where the engine is, in ExoPlayer's terms. */
enum class EngineState { Idle, Buffering, Ready, Ended }

/** Why the engine stopped, reduced to what the player does about it. */
sealed interface EngineError {
    /** The server answered a stream or file request with [status] (404: the session or the video is gone). */
    data class Http(val status: Int) : EngineError

    /** The server could not be reached, or the connection dropped. */
    data object Network : EngineError

    /** This phone could not decode or read what it was sent. */
    data object Decoding : EngineError

    data object Other : EngineError
}

/**
 * The video engine behind the player ([ExoPlayerEngine] on a phone), kept behind this
 * small surface so the player's rules ([PlayerController]) run in plain JVM tests.
 * Every call happens on the main thread.
 */
interface PlayerEngine {
    interface Listener {
        fun onStateChanged(state: EngineState)

        /** The person's intent to play (true while buffering too), not whether frames move. */
        fun onPlayWhenReadyChanged(playWhenReady: Boolean)

        fun onError(error: EngineError)

        /** The picture's shape on screen, after pixel aspect, for picture-in-picture. */
        fun onVideoSize(width: Int, height: Int)
    }

    fun setListener(listener: Listener?)

    /** Replaces what plays with [source], starting at [startMs]. */
    fun load(source: StreamSource, startMs: Long, playWhenReady: Boolean)

    fun play()

    fun pause()

    fun seekTo(positionMs: Long)

    val playWhenReady: Boolean

    /** Milliseconds from the start of the video. */
    val positionMs: Long

    /** Milliseconds, or 0 while the length is not known yet. */
    val durationMs: Long

    /** How far ahead the video has loaded, in milliseconds from the start. */
    val bufferedMs: Long

    fun release()
}
