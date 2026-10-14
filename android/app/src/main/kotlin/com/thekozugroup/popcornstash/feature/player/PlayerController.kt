package com.thekozugroup.popcornstash.feature.player

import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.ApiException
import com.thekozugroup.popcornstash.data.api.ItemDetail
import com.thekozugroup.popcornstash.data.api.MediaKind
import com.thekozugroup.popcornstash.data.api.PlaybackDecision
import com.thekozugroup.popcornstash.data.api.PlaybackRequest
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import okhttp3.HttpUrl

/** The calls the player makes (docs/architecture.md section 8), so tests can stand in for the server. */
interface PlayerApi {
    suspend fun item(id: String): ItemDetail

    suspend fun playback(id: String, request: PlaybackRequest): PlaybackDecision

    suspend fun stopStream(sessionId: String)

    suspend fun saveProgress(id: String, positionS: Double, durationS: Double)

    /** A server-relative URL from an answer, made absolute on the signed-in server. */
    fun absolute(path: String): HttpUrl
}

/** The signed-in person's [ApiClient] as the player uses it. */
class ApiPlayerApi(private val api: ApiClient) : PlayerApi {
    override suspend fun item(id: String) = api.item(id)

    override suspend fun playback(id: String, request: PlaybackRequest) = api.playback(id, request)

    override suspend fun stopStream(sessionId: String) = api.stopStream(sessionId)

    override suspend fun saveProgress(id: String, positionS: Double, durationS: Double) = api.saveProgress(id, positionS, durationS)

    override fun absolute(path: String): HttpUrl = api.absolute(path)
}

/** What opened the player: an item, from its resume point or from the beginning. [nonce] tells two taps apart. */
data class PlayerRequest(val itemId: String, val fromStart: Boolean = false, val nonce: Long = 0)

sealed interface PlayerPhase {
    /** Reading the item or asking the server how to play it. */
    data object Loading : PlayerPhase

    /** A stream is loaded: playing, paused or buffering. */
    data object Ready : PlayerPhase

    /** The video played to its end. */
    data object Ended : PlayerPhase

    /** Playback stopped. [gone]: the video is no longer there for this person, so only Back makes sense. */
    data class Failed(val message: String, val gone: Boolean) : PlayerPhase
}

/** The episode after this one in the same library; [label] ("S01E03 · The lantern keepers") once it is read. */
data class NextEpisode(val itemId: String, val label: String? = null)

data class PlayerUiState(
    val title: String = "",
    val subtitle: String? = null,
    val phase: PlayerPhase = PlayerPhase.Loading,
    /** The person wants it playing (true while buffering too). */
    val playWhenReady: Boolean = true,
    val buffering: Boolean = false,
    /** A new stream is on its way after a change of audio or quality. */
    val switching: Boolean = false,
    val durationMs: Long = 0,
    val audio: List<AudioChoice> = emptyList(),
    /** The stream index of the audio track playing. */
    val selectedAudio: Int? = null,
    val qualities: List<QualityChoice> = emptyList(),
    /** Null: original. */
    val selectedQuality: Int? = null,
    /** The server converts this video while it plays (HLS). */
    val converted: Boolean = false,
    val next: NextEpisode? = null,
    /** The picture's shape, for picture-in-picture; 0 while unknown. */
    val videoWidth: Int = 0,
    val videoHeight: Int = 0,
    /** The video is taller than wide (a phone video held upright). */
    val portrait: Boolean = false,
) {
    val hasMenu: Boolean get() = audio.isNotEmpty() || qualities.isNotEmpty()
}

/**
 * The player's rules (docs/architecture.md section 10, Item and player), apart from Android
 * so they run in plain JVM tests:
 *
 * - `POST /api/items/:id/playback` with this phone's decoders ([deviceCapabilities]), the
 *   resume point and the person's audio and quality choices; plays `direct` or `hls`.
 * - Saves progress every 10 seconds while playing, on pause, when leaving and at the end ([ProgressSaver]).
 * - Ends every HLS session it is given once it stops playing it ([StreamSessions]).
 * - When the server forgets an HLS session (idle for a minute while paused, or evicted), the
 *   next segment answers 404: the player asks for a new stream at the same place, once. A
 *   404 for the video itself (access removed, file gone) stops with "This video is no
 *   longer available."
 * - A file this phone fails to decode after claiming it could is asked for once more with
 *   the most widely played codecs ([PlaybackRequestBuilder.SafeDevice]), as the web does.
 * - At the end, the next episode is offered; nothing starts on its own.
 *
 * Runs on the main thread. [scope] is the player's own (cancelled when it leaves); [outlive]
 * is the signed-in session's, for the last progress save and ending the stream after the
 * screen is gone.
 */
class PlayerController(
    private val api: PlayerApi,
    private val engine: PlayerEngine,
    private val deviceCapabilities: suspend () -> PlaybackRequest,
    private val scope: CoroutineScope,
    private val outlive: CoroutineScope,
    private val progressEveryMs: Long = PROGRESS_EVERY_MS,
) : PlayerEngine.Listener {
    private val _state = MutableStateFlow(PlayerUiState())
    val state: StateFlow<PlayerUiState> = _state.asStateFlow()

    private val sessions = StreamSessions(outlive) { api.stopStream(it) }

    private var request: PlayerRequest? = null
    private var item: ItemDetail? = null
    private var saver: ProgressSaver? = null
    private var choice = StreamChoice()
    private var device: PlaybackRequest? = null
    private var fellBack = false
    private var recovering = false
    private var hls = false
    private var startS = 0.0
    private var generation = 0
    private var job: Job? = null
    private var nextJob: Job? = null
    private var ticker: Job? = null
    private var closed = false

    init {
        engine.setListener(this)
    }

    /** Milliseconds into the video, for the seek bar (read while it shows). */
    val positionMs: Long get() = engine.positionMs

    val bufferedMs: Long get() = engine.bufferedMs

    /** The session the player holds on the server, for tests. */
    internal val activeStream: String? get() = sessions.active

    /** Opens [request]; the same request again (the screen came back) changes nothing. */
    fun open(request: PlayerRequest) {
        if (closed || request == this.request) return
        this.request = request
        startItem(request.itemId, request.fromStart, keepQuality = false)
    }

    fun togglePlay() {
        when {
            _state.value.phase == PlayerPhase.Ended -> playAgain()
            engine.playWhenReady -> engine.pause()
            else -> engine.play()
        }
    }

    fun play() = engine.play()

    fun pause() = engine.pause()

    fun seekTo(positionMs: Long) {
        val duration = durationMs()
        val target = if (duration > 0) positionMs.coerceIn(0, duration) else positionMs.coerceAtLeast(0)
        engine.seekTo(target)
    }

    /** From the end: back to the start, playing. */
    fun playAgain() {
        if (_state.value.phase != PlayerPhase.Ended) return
        engine.seekTo(0)
        engine.play()
        _state.update { it.copy(phase = PlayerPhase.Ready) }
    }

    /** The next episode, from its own resume point, in the same quality. */
    fun playNext() {
        val next = _state.value.next ?: return
        saveNow(SaveReason.Leave)
        startItem(next.itemId, fromStart = false, keepQuality = true)
    }

    fun chooseQuality(height: Int?) {
        if (height == choice.qualityHeight) return
        choice = choice.copy(qualityHeight = height)
        _state.update { it.copy(selectedQuality = height) }
        switchStream()
    }

    fun chooseAudio(index: Int) {
        val current = _state.value.selectedAudio
        if (index == current) return
        choice = choice.copy(audioIndex = index)
        _state.update { it.copy(selectedAudio = index) }
        switchStream()
    }

    /** "Try again" after a failure: the item again if it never loaded, else a new stream where it stopped. */
    fun retry() {
        val request = request ?: return
        if (closed) return
        val loaded = item
        if (loaded == null) {
            startItem(request.itemId, request.fromStart, keepQuality = true)
            return
        }
        recovering = false
        val position = engine.positionMs.takeIf { it > 0 }?.div(1000.0) ?: startS
        _state.update { it.copy(phase = PlayerPhase.Loading, buffering = false, switching = false) }
        job?.cancel()
        job = scope.launch { stream(position, playWhenReady = true, device = if (fellBack) PlaybackRequestBuilder.SafeDevice else null) }
    }

    /** The screen went to the background without picture-in-picture: pause and save the place. */
    fun onBackground() {
        engine.pause()
        saveNow(SaveReason.Leave)
    }

    /** The player leaves: saves the place, ends the stream on the server and frees the engine. */
    fun close() {
        if (closed) return
        saveNow(SaveReason.Leave, on = outlive)
        closed = true
        generation++
        stopTicker()
        job?.cancel()
        nextJob?.cancel()
        sessions.close()
        engine.setListener(null)
        engine.release()
    }

    // ------------------------------------------------------------ engine ----

    override fun onStateChanged(state: EngineState) {
        if (closed) return
        when (state) {
            EngineState.Buffering -> _state.update { it.copy(buffering = true) }
            EngineState.Ready -> {
                recovering = false
                val duration = engine.durationMs
                _state.update {
                    it.copy(
                        phase = if (it.phase is PlayerPhase.Failed) it.phase else PlayerPhase.Ready,
                        buffering = false,
                        switching = false,
                        durationMs = if (duration > 0) duration else it.durationMs,
                    )
                }
            }
            EngineState.Ended -> {
                if (_state.value.phase !is PlayerPhase.Ready) return
                stopTicker()
                saveNow(SaveReason.Ended)
                _state.update { it.copy(phase = PlayerPhase.Ended, buffering = false, switching = false) }
            }
            EngineState.Idle -> Unit
        }
    }

    override fun onPlayWhenReadyChanged(playWhenReady: Boolean) {
        if (closed) return
        _state.update { it.copy(playWhenReady = playWhenReady) }
        if (playWhenReady) {
            startTicker()
        } else {
            stopTicker()
            if (_state.value.phase == PlayerPhase.Ready) saveNow(SaveReason.Pause)
        }
    }

    override fun onVideoSize(width: Int, height: Int) {
        if (width > 0 && height > 0) _state.update { it.copy(videoWidth = width, videoHeight = height) }
    }

    override fun onError(error: EngineError) {
        if (closed || item == null) return
        val position = engine.positionMs / 1000.0
        val keepPlaying = engine.playWhenReady
        when {
            error is EngineError.Http && (error.status == 404 || error.status == 410) && !recovering -> {
                // The server forgot this conversion, or the file went away: ask again. A 404
                // to that question means the video is gone for this person.
                recovering = true
                restart(position, keepPlaying, device = null)
            }
            error is EngineError.Decoding && !hls && !fellBack -> {
                // This phone claimed a codec it cannot play: once, ask for the safest stream.
                fellBack = true
                recovering = true
                restart(position, keepPlaying, device = PlaybackRequestBuilder.SafeDevice)
            }
            error is EngineError.Http && (error.status == 404 || error.status == 410) -> fail(GONE, gone = true)
            error is EngineError.Network -> fail("The stream stopped. Check the connection and try again.", gone = false)
            error is EngineError.Http && error.status >= 500 -> fail("PopcornStash ran into a problem. Try again in a moment.", gone = false)
            else -> fail("This video could not be played on this phone.", gone = false)
        }
    }

    // ----------------------------------------------------------- inside ----

    private fun startItem(itemId: String, fromStart: Boolean, keepQuality: Boolean) {
        job?.cancel()
        nextJob?.cancel()
        stopTicker()
        generation++
        sessions.adopt(null)
        engine.pause()
        item = null
        saver = null
        fellBack = false
        recovering = false
        hls = false
        choice = if (keepQuality) StreamChoice(qualityHeight = choice.qualityHeight) else StreamChoice()
        _state.value = PlayerUiState(phase = PlayerPhase.Loading, selectedQuality = choice.qualityHeight)
        job = scope.launch {
            val detail = try {
                api.item(itemId)
            } catch (e: ApiException) {
                fail(e)
                return@launch
            }
            if (detail.kind != MediaKind.Video) {
                fail("This file is not a video, so it cannot play here.", gone = false)
                return@launch
            }
            item = detail
            saver = ProgressSaver { position, duration -> api.saveProgress(detail.id, position, duration) }
            startS = if (fromStart) 0.0 else PlayerFormat.resumePosition(detail)
            val (title, subtitle) = PlayerFormat.titleLines(detail)
            val deviceMax = device().maxHeight
            val audio = PlaybackRequestBuilder.audioChoices(detail.media)
            val qualities = PlaybackRequestBuilder.qualities(detail.media, detail.width, detail.height, deviceMax)
            if (choice.qualityHeight != null && qualities.none { it.height == choice.qualityHeight }) {
                choice = choice.copy(qualityHeight = null)
            }
            val (width, height) = displaySize(detail)
            _state.update {
                it.copy(
                    title = title,
                    subtitle = subtitle,
                    durationMs = ((detail.durationS ?: detail.media?.durationS ?: 0.0) * 1000).toLong(),
                    audio = audio,
                    selectedAudio = detail.media?.audio?.firstOrNull()?.index,
                    qualities = qualities,
                    selectedQuality = choice.qualityHeight,
                    next = detail.nextEpisodeId?.let { id -> NextEpisode(id) },
                    videoWidth = width,
                    videoHeight = height,
                    portrait = height > width,
                )
            }
            detail.nextEpisodeId?.let(::readNext)
            stream(startS, playWhenReady = true, device = null)
        }
    }

    /** Names the next episode for the end of this one; drops it when this person cannot see it. */
    private fun readNext(nextId: String) {
        nextJob = scope.launch {
            try {
                val next = api.item(nextId)
                _state.update { state ->
                    if (state.next?.itemId == nextId) state.copy(next = NextEpisode(nextId, PlayerFormat.nextLabel(next))) else state
                }
            } catch (e: ApiException) {
                if (e.isNotFound) _state.update { state -> if (state.next?.itemId == nextId) state.copy(next = null) else state }
            }
        }
    }

    private fun switchStream() {
        if (item == null || closed) return
        if (_state.value.phase is PlayerPhase.Failed) return // "Try again" uses the new choice.
        val position = engine.positionMs / 1000.0
        saveNow(SaveReason.Leave)
        _state.update { it.copy(switching = true) }
        restart(position, engine.playWhenReady, device = if (fellBack) PlaybackRequestBuilder.SafeDevice else null)
    }

    private fun restart(positionS: Double, playWhenReady: Boolean, device: PlaybackRequest?) {
        job?.cancel()
        job = scope.launch { stream(positionS, playWhenReady, device) }
    }

    /** Asks the server how to play the item from [positionS] and loads the answer. */
    private suspend fun stream(positionS: Double, playWhenReady: Boolean, device: PlaybackRequest?) {
        val detail = item ?: return
        val asked = ++generation
        val body = PlaybackRequestBuilder.build(device ?: device(), choice, positionS, detail.media)
        // Never abandoned half way: a conversion the server starts for this request is either
        // played or ended, even when the person moved on (or left) while the answer was coming.
        val answer = withContext(NonCancellable) {
            try {
                val decision = api.playback(detail.id, body)
                if (asked != generation || closed) {
                    sessions.discard(decision.sessionId.takeIf { decision.mode == "hls" })
                    null
                } else {
                    Result.success(decision)
                }
            } catch (e: ApiException) {
                if (asked == generation && !closed) Result.failure(e) else null
            }
        } ?: return
        val decision = answer.getOrElse { error ->
            fail(error as ApiException)
            return
        }
        val isHls = decision.mode == "hls"
        val session = decision.sessionId.takeIf { isHls }
        val url = try {
            api.absolute(decision.url)
        } catch (e: ApiException) {
            sessions.discard(session)
            fail(e)
            return
        }
        sessions.adopt(session)
        hls = isHls
        _state.update { it.copy(converted = isHls) }
        engine.load(StreamSource(url, isHls), startMs = (positionS * 1000).toLong(), playWhenReady = playWhenReady)
    }

    private suspend fun device(): PlaybackRequest = device ?: deviceCapabilities().also { device = it }

    private fun startTicker() {
        if (ticker?.isActive == true) return
        ticker = scope.launch {
            while (isActive) {
                delay(progressEveryMs)
                if (_state.value.phase == PlayerPhase.Ready && engine.playWhenReady) saveNow(SaveReason.Interval)
            }
        }
    }

    private fun stopTicker() {
        ticker?.cancel()
        ticker = null
    }

    /** Saves where the player is now; the position is read at once, the request runs [on] a scope. */
    private fun saveNow(reason: SaveReason, on: CoroutineScope = scope) {
        if (closed) return
        val saver = saver ?: return
        val position = engine.positionMs / 1000.0
        val duration = durationMs() / 1000.0
        on.launch {
            try {
                saver.save(position, duration, reason)
            } catch (e: ApiException) {
                // The video went away for this person while they watched.
                if (e.isNotFound && on === scope && !closed) fail(GONE, gone = true)
            }
        }
    }

    private fun durationMs(): Long = engine.durationMs.takeIf { it > 0 } ?: _state.value.durationMs

    private fun fail(e: ApiException) = when {
        e.isNotFound -> fail(GONE, gone = true)
        else -> fail(e.message, gone = false)
    }

    private fun fail(message: String, gone: Boolean) {
        if (closed) return
        val wasLoaded = _state.value.phase == PlayerPhase.Ready || _state.value.phase == PlayerPhase.Ended
        if (!gone && wasLoaded) saveNow(SaveReason.Leave)
        generation++
        stopTicker()
        // Failed before pausing, so the pause saves nothing more (a gone video would answer 404).
        _state.update { it.copy(phase = PlayerPhase.Failed(message, gone), buffering = false, switching = false) }
        engine.pause()
        sessions.adopt(null)
    }

    /** The frame's shape as it shows: a phone video recorded sideways is turned upright. */
    private fun displaySize(detail: ItemDetail): Pair<Int, Int> {
        val video = detail.media?.video
        val width = video?.width?.takeIf { it > 0 } ?: detail.width ?: 0
        val height = video?.height?.takeIf { it > 0 } ?: detail.height ?: 0
        val quarterTurns = (((video?.rotation ?: 0) % 360) + 360) % 360 / 90
        return if (quarterTurns % 2 == 1) height to width else width to height
    }

    companion object {
        const val PROGRESS_EVERY_MS = 10_000L
        const val GONE = "This video is no longer available."
    }
}
