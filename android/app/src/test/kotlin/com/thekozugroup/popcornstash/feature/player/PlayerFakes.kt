package com.thekozugroup.popcornstash.feature.player

import com.thekozugroup.popcornstash.data.api.ApiException
import com.thekozugroup.popcornstash.data.api.AudioStream
import com.thekozugroup.popcornstash.data.api.Category
import com.thekozugroup.popcornstash.data.api.ItemDetail
import com.thekozugroup.popcornstash.data.api.MediaKind
import com.thekozugroup.popcornstash.data.api.MediaSummary
import com.thekozugroup.popcornstash.data.api.PlaybackDecision
import com.thekozugroup.popcornstash.data.api.PlaybackRequest
import com.thekozugroup.popcornstash.data.api.Progress
import com.thekozugroup.popcornstash.data.api.VideoStream
import kotlinx.coroutines.CompletableDeferred
import okhttp3.HttpUrl
import okhttp3.HttpUrl.Companion.toHttpUrl

/** Items shaped like the dev fixture's (scripts/dev-fixture.sh), with media facts as the server reads them. */
object PlayerItems {
    val episodeOne = ItemDetail(
        id = "i-lantern-1",
        kind = MediaKind.Video,
        category = Category.Episode,
        title = "The Lantern Keepers S01E01",
        showName = "The Lantern Keepers",
        season = 1,
        episode = 1,
        episodeTitle = "Pilot",
        durationS = 3136.0,
        width = 1920,
        height = 1080,
        progress = Progress(positionS = 724.0, durationS = 3136.0),
        media = MediaSummary(
            container = "matroska,webm",
            durationS = 3136.0,
            video = VideoStream(index = 0, codec = "hevc", width = 1920, height = 1080),
            audio = listOf(
                AudioStream(index = 1, codec = "eac3", channels = 6, language = "eng"),
                AudioStream(index = 2, codec = "aac", channels = 2, language = "eng", title = "Commentary"),
            ),
        ),
        nextEpisodeId = "i-lantern-2",
    )

    val episodeTwo = episodeOne.copy(
        id = "i-lantern-2",
        title = "The Lantern Keepers S01E02",
        episode = 2,
        episodeTitle = "The lantern keepers",
        progress = null,
        nextEpisodeId = null,
    )

    val film = ItemDetail(
        id = "i-paper",
        kind = MediaKind.Video,
        category = Category.Movie,
        title = "Paper Boats",
        year = 2011,
        durationS = 6.0,
        width = 320,
        height = 180,
        progress = null,
        media = MediaSummary(
            container = "mov,mp4,m4a,3gp,3g2,mj2",
            durationS = 6.0,
            video = VideoStream(index = 0, codec = "h264", width = 320, height = 180),
            audio = listOf(AudioStream(index = 1, codec = "aac", channels = 2)),
        ),
    )

    /** A phone video recorded sideways: stored 1920 x 1080 with a 90 degree turn. */
    val upright = film.copy(
        id = "i-upright",
        category = Category.HomeVideo,
        title = "IMG_0042",
        width = 1920,
        height = 1080,
        media = film.media!!.copy(video = VideoStream(index = 0, codec = "hevc", width = 1920, height = 1080, rotation = 90)),
    )

    val song = film.copy(id = "i-song", kind = MediaKind.Audio, category = Category.Other, title = "Morning Notes")
}

/** The server as the player sees it: items, playback answers, and what was asked of it. */
class FakePlayerApi(vararg items: ItemDetail) : PlayerApi {
    val items = items.associateBy { it.id }.toMutableMap()
    var recommended: List<com.thekozugroup.popcornstash.data.api.Item> = emptyList()
    override suspend fun recommendations(libraryId: String?) = recommended
    val itemReads = mutableListOf<String>()
    val playbackRequests = mutableListOf<Pair<String, PlaybackRequest>>()
    val stopped = mutableListOf<String>()
    val progress = mutableListOf<Triple<String, Double, Double>>()

    /** Answers to the next playback requests, in order; when empty, a direct answer. */
    val answers = ArrayDeque<suspend () -> PlaybackDecision>()
    var progressError: ApiException? = null
    private var sessions = 0

    fun hls(): suspend () -> PlaybackDecision = {
        sessions++
        PlaybackDecision("hls", "/api/stream/s-$sessions/index.m3u8", "s-$sessions")
    }

    fun direct(id: String): suspend () -> PlaybackDecision = { PlaybackDecision("direct", "/api/items/$id/file") }

    fun refuse(status: Int, message: String): suspend () -> PlaybackDecision = {
        throw ApiException(status, if (status == 404) "not_found" else "conflict", message)
    }

    /** An answer that waits for [gate], to catch the player moving on meanwhile. */
    fun held(gate: CompletableDeferred<Unit>, answer: suspend () -> PlaybackDecision): suspend () -> PlaybackDecision = {
        gate.await()
        answer()
    }

    override suspend fun item(id: String): ItemDetail {
        itemReads += id
        return items[id] ?: throw ApiException(404, "not_found", "That no longer exists.")
    }

    override suspend fun playback(id: String, request: PlaybackRequest): PlaybackDecision {
        playbackRequests += id to request
        if (id !in items) throw ApiException(404, "not_found", "That no longer exists.")
        val answer = answers.removeFirstOrNull() ?: direct(id)
        return answer()
    }

    override suspend fun stopStream(sessionId: String) {
        stopped += sessionId
    }

    override suspend fun saveProgress(id: String, positionS: Double, durationS: Double) {
        progressError?.let { throw it }
        progress += Triple(id, positionS, durationS)
    }

    override fun absolute(path: String): HttpUrl = "https://stash.example.com/".toHttpUrl().resolve(path)!!
}

/** ExoPlayer as the player sees it, driven by the test. Like ExoPlayer, it reports changes at once. */
class FakeEngine : PlayerEngine {
    private var listener: PlayerEngine.Listener? = null
    val loads = mutableListOf<Triple<StreamSource, Long, Boolean>>()
    var released = false
    override var positionMs: Long = 0
    override var durationMs: Long = 0
    override var bufferedMs: Long = 0
    override var playWhenReady: Boolean = false
        private set

    val lastSource: StreamSource? get() = loads.lastOrNull()?.first

    override fun setListener(listener: PlayerEngine.Listener?) {
        this.listener = listener
    }

    override fun load(source: StreamSource, startMs: Long, playWhenReady: Boolean) {
        loads += Triple(source, startMs, playWhenReady)
        positionMs = startMs
        listener?.onStateChanged(EngineState.Buffering)
        setPlayWhenReady(playWhenReady)
    }

    override fun play() = setPlayWhenReady(true)

    override fun pause() = setPlayWhenReady(false)

    override fun seekTo(positionMs: Long) {
        this.positionMs = positionMs
    }

    override fun release() {
        released = true
    }

    private fun setPlayWhenReady(value: Boolean) {
        if (playWhenReady == value) return
        playWhenReady = value
        listener?.onPlayWhenReadyChanged(value)
    }

    /** The stream loaded: [duration] is known. */
    fun ready(duration: Long = 3_136_000) {
        durationMs = duration
        listener?.onStateChanged(EngineState.Ready)
    }

    fun ended() {
        positionMs = durationMs
        listener?.onStateChanged(EngineState.Ended)
    }

    fun fail(error: EngineError) = listener?.onError(error)
}
