package com.thekozugroup.popcornstash.feature.player

import android.content.Context
import androidx.annotation.OptIn
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.MimeTypes
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.common.VideoSize
import androidx.media3.common.util.UnstableApi
import androidx.media3.datasource.DataSource
import androidx.media3.datasource.HttpDataSource
import androidx.media3.datasource.okhttp.OkHttpDataSource
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import okhttp3.OkHttpClient
import java.util.concurrent.TimeUnit
import kotlin.math.roundToInt

/**
 * The phone's player: Media3 ExoPlayer with HLS, every request made through [http], the
 * signed-in session's OkHttp client. Its interceptor adds the bearer token to requests for
 * the server only, so the playlist, its segments and the original file (with Range
 * requests) are authorised and the token goes nowhere else. Nothing is cached on disk.
 */
@OptIn(UnstableApi::class)
class ExoPlayerEngine(context: Context, http: OkHttpClient) : PlayerEngine {
    /** The server may hold a segment request up to 30 seconds while ffmpeg starts (section 5). */
    val dataSourceFactory: DataSource.Factory = OkHttpDataSource.Factory(playerClient(http))

    val player: ExoPlayer = ExoPlayer.Builder(context.applicationContext)
        .setMediaSourceFactory(DefaultMediaSourceFactory(dataSourceFactory))
        .setAudioAttributes(
            AudioAttributes.Builder().setUsage(C.USAGE_MEDIA).setContentType(C.AUDIO_CONTENT_TYPE_MOVIE).build(),
            /* handleAudioFocus = */ true,
        )
        .setHandleAudioBecomingNoisy(true)
        .build()

    private var listener: PlayerEngine.Listener? = null

    private val events = object : Player.Listener {
        override fun onPlaybackStateChanged(playbackState: Int) {
            listener?.onStateChanged(engineState(playbackState))
        }

        override fun onPlayWhenReadyChanged(playWhenReady: Boolean, reason: Int) {
            listener?.onPlayWhenReadyChanged(playWhenReady)
        }

        override fun onPlayerError(error: PlaybackException) {
            listener?.onError(engineError(error))
        }

        override fun onVideoSizeChanged(videoSize: VideoSize) {
            if (videoSize.width > 0 && videoSize.height > 0) {
                listener?.onVideoSize((videoSize.width * videoSize.pixelWidthHeightRatio).roundToInt(), videoSize.height)
            }
        }
    }

    init {
        player.addListener(events)
    }

    override fun setListener(listener: PlayerEngine.Listener?) {
        this.listener = listener
    }

    override fun load(source: StreamSource, startMs: Long, playWhenReady: Boolean) {
        val item = MediaItem.Builder()
            .setUri(source.url.toString())
            .apply { if (source.hls) setMimeType(MimeTypes.APPLICATION_M3U8) }
            .build()
        player.setMediaItem(item, startMs.coerceAtLeast(0))
        player.playWhenReady = playWhenReady
        player.prepare()
    }

    override fun play() {
        if (player.playbackState == Player.STATE_ENDED) player.seekTo(0)
        player.play()
    }

    override fun pause() = player.pause()

    override fun seekTo(positionMs: Long) = player.seekTo(positionMs)

    override val playWhenReady: Boolean get() = player.playWhenReady

    override val positionMs: Long get() = player.currentPosition.coerceAtLeast(0)

    override val durationMs: Long get() = player.duration.takeIf { it != C.TIME_UNSET && it > 0 } ?: 0L

    override val bufferedMs: Long get() = player.bufferedPosition.coerceAtLeast(0)

    override fun release() {
        listener = null
        player.removeListener(events)
        player.release()
    }

    companion object {
        /** The session's client with a read timeout long enough for a conversion to start. */
        fun playerClient(http: OkHttpClient): OkHttpClient = http.newBuilder().readTimeout(60, TimeUnit.SECONDS).build()

        fun engineState(playbackState: Int): EngineState = when (playbackState) {
            Player.STATE_BUFFERING -> EngineState.Buffering
            Player.STATE_READY -> EngineState.Ready
            Player.STATE_ENDED -> EngineState.Ended
            else -> EngineState.Idle
        }

        /** What the player does about an ExoPlayer error depends only on this. */
        fun engineError(error: PlaybackException): EngineError {
            var cause: Throwable? = error.cause
            while (cause != null) {
                if (cause is HttpDataSource.InvalidResponseCodeException) return EngineError.Http(cause.responseCode)
                cause = cause.cause
            }
            return when (error.errorCode) {
                PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_FAILED,
                PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_TIMEOUT,
                PlaybackException.ERROR_CODE_TIMEOUT,
                -> EngineError.Network
                in 3000..3999, in 4000..4999 -> EngineError.Decoding
                else -> EngineError.Other
            }
        }
    }
}
