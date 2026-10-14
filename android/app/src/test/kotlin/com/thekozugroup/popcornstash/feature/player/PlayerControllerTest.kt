package com.thekozugroup.popcornstash.feature.player

import com.thekozugroup.popcornstash.data.api.ApiException
import com.thekozugroup.popcornstash.data.api.PlaybackRequest
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The player's rules against a stand-in server and engine: the playback decision, progress
 * saving, the HLS session's life, choices, the next episode and failures.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class PlayerControllerTest {
    private val device = PlaybackRequest(listOf("h264", "hevc"), listOf("aac", "opus"), hdr = false, maxHeight = 1080)
    private val engine = FakeEngine()
    private var playerScope: CoroutineScope? = null
    private var deviceReads = 0

    /**
     * The player's scope and the outliving one run on the test's clock as children of
     * [TestScope.backgroundScope], so the progress ticker stops when the test ends.
     */
    private fun TestScope.player(api: FakePlayerApi): PlayerController {
        val parent = backgroundScope.coroutineContext
        val scope = CoroutineScope(parent + SupervisorJob(parent[Job])).also { playerScope = it }
        val outlive = CoroutineScope(parent + SupervisorJob(parent[Job]))
        return PlayerController(api, engine, { deviceReads++; device }, scope, outlive)
    }

    private val PlayerController.phase get() = state.value.phase

    @Test
    fun opensAnItemAtItsResumePointWithThisPhonesDecoders() = runTest {
        val api = FakePlayerApi(PlayerItems.episodeOne, PlayerItems.episodeTwo)
        val player = player(api)
        player.open(PlayerRequest("i-lantern-1"))
        runCurrent()

        val (id, request) = api.playbackRequests.single()
        assertEquals("i-lantern-1", id)
        assertEquals(listOf("h264", "hevc"), request.videoCodecs)
        assertEquals(listOf("aac", "opus"), request.audioCodecs)
        assertEquals(1080, request.maxHeight)
        assertEquals(724.0, request.startS!!, 0.0)
        assertNull(request.audioIndex)

        val (source, startMs, playWhenReady) = engine.loads.single()
        assertEquals("https://stash.example.com/api/items/i-lantern-1/file", source.url.toString())
        assertFalse(source.hls)
        assertEquals(724_000L, startMs)
        assertTrue(playWhenReady)

        val state = player.state.value
        assertEquals(PlayerPhase.Loading, state.phase)
        assertEquals("The Lantern Keepers", state.title)
        assertEquals("S01E01 · Pilot", state.subtitle)
        assertEquals(listOf(1, 2), state.audio.map { it.index })
        assertEquals(1, state.selectedAudio)
        assertEquals(listOf(null, 720, 480), state.qualities.map { it.height })
        assertEquals(NextEpisode("i-lantern-2", "S01E02 · The lantern keepers"), state.next)
        assertFalse(state.converted)

        engine.ready()
        assertEquals(PlayerPhase.Ready, player.phase)
        assertEquals(3_136_000L, player.state.value.durationMs)
        assertEquals("the decoders are read once", 1, deviceReads)
    }

    @Test
    fun startFromTheBeginningSendsNoStart() = runTest {
        val api = FakePlayerApi(PlayerItems.episodeOne)
        player(api).open(PlayerRequest("i-lantern-1", fromStart = true))
        runCurrent()
        assertNull(api.playbackRequests.single().second.startS)
        assertEquals(0L, engine.loads.single().second)
    }

    @Test
    fun theSameRequestAgainChangesNothingButANewTapOpensAgain() = runTest {
        val api = FakePlayerApi(PlayerItems.film)
        val player = player(api)
        player.open(PlayerRequest("i-paper", nonce = 1))
        runCurrent()
        player.open(PlayerRequest("i-paper", nonce = 1))
        runCurrent()
        assertEquals(listOf("i-paper"), api.itemReads)
        player.open(PlayerRequest("i-paper", nonce = 2))
        runCurrent()
        assertEquals(listOf("i-paper", "i-paper"), api.itemReads)
    }

    @Test
    fun anHlsAnswerPlaysThePlaylistAndLeavingEndsTheSessionAndSavesThePlace() = runTest {
        val api = FakePlayerApi(PlayerItems.episodeOne)
        api.answers += api.hls()
        val player = player(api)
        player.open(PlayerRequest("i-lantern-1"))
        runCurrent()
        val source = engine.lastSource!!
        assertTrue(source.hls)
        assertEquals("https://stash.example.com/api/stream/s-1/index.m3u8", source.url.toString())
        assertTrue(player.state.value.converted)
        engine.ready()
        engine.positionMs = 800_400

        // The view model is cleared: its own scope ends first, then the player closes.
        playerScope!!.cancel()
        player.close()
        runCurrent()
        assertEquals(listOf("s-1"), api.stopped)
        assertEquals(listOf(Triple("i-lantern-1", 800.4, 3136.0)), api.progress)
        assertTrue(engine.released)
        player.close()
        runCurrent()
        assertEquals("closing twice ends once", listOf("s-1"), api.stopped)
    }

    @Test
    fun directPlayHasNoSessionToEnd() = runTest {
        val api = FakePlayerApi(PlayerItems.film)
        val player = player(api)
        player.open(PlayerRequest("i-paper"))
        runCurrent()
        engine.ready(6_000)
        player.close()
        runCurrent()
        assertTrue(api.stopped.isEmpty())
    }

    @Test
    fun progressIsSavedEveryTenSecondsWhilePlayingAndOnPause() = runTest {
        val api = FakePlayerApi(PlayerItems.episodeOne)
        val player = player(api)
        player.open(PlayerRequest("i-lantern-1"))
        runCurrent()
        engine.ready()

        engine.positionMs = 734_000
        advanceTimeBy(10_001)
        engine.positionMs = 744_000
        advanceTimeBy(10_000)
        assertEquals(listOf(734.0, 744.0), api.progress.map { it.second })

        engine.positionMs = 747_500
        player.pause()
        runCurrent()
        assertEquals(747.5, api.progress.last().second, 0.0)

        // Nothing while paused, however long.
        advanceTimeBy(60_000)
        assertEquals(3, api.progress.size)

        player.play()
        engine.positionMs = 757_500
        advanceTimeBy(10_001)
        assertEquals(757.5, api.progress.last().second, 0.0)
        assertTrue(api.progress.all { it.first == "i-lantern-1" && it.third == 3136.0 })
    }

    @Test
    fun theEndIsSavedAsWatchedAndOffersTheNextEpisodeWithoutStartingIt() = runTest {
        val api = FakePlayerApi(PlayerItems.episodeOne, PlayerItems.episodeTwo)
        api.answers += api.hls()
        val player = player(api)
        player.open(PlayerRequest("i-lantern-1"))
        runCurrent()
        engine.ready()
        engine.ended()
        runCurrent()

        assertEquals(PlayerPhase.Ended, player.phase)
        assertEquals(Triple("i-lantern-1", 3136.0, 3136.0), api.progress.last())
        assertEquals("nothing starts on its own", 1, api.playbackRequests.size)

        api.answers += api.hls()
        player.playNext()
        runCurrent()
        assertEquals(listOf("i-lantern-1", "i-lantern-2", "i-lantern-2"), api.itemReads)
        val (id, request) = api.playbackRequests.last()
        assertEquals("i-lantern-2", id)
        assertNull("the next episode starts from its own place", request.startS)
        assertEquals("the previous conversion is ended", listOf("s-1"), api.stopped)
        assertEquals("S01E02 · The lantern keepers", player.state.value.subtitle)
        assertNull("the last episode has no next", player.state.value.next)
        assertEquals("s-2", player.activeStream)
    }

    @Test
    fun aNextEpisodeThisPersonCannotSeeIsNotOffered() = runTest {
        val api = FakePlayerApi(PlayerItems.episodeOne)
        val player = player(api)
        player.open(PlayerRequest("i-lantern-1"))
        runCurrent()
        assertNull(player.state.value.next)
    }

    @Test
    fun playAgainFromTheEndGoesBackToTheStart() = runTest {
        val api = FakePlayerApi(PlayerItems.film)
        val player = player(api)
        player.open(PlayerRequest("i-paper"))
        runCurrent()
        engine.ready(6_000)
        engine.ended()
        runCurrent()
        assertEquals(PlayerPhase.Ended, player.phase)
        player.togglePlay()
        assertEquals(0L, engine.positionMs)
        assertTrue(engine.playWhenReady)
        assertEquals(PlayerPhase.Ready, player.phase)
    }

    @Test
    fun aQualityChoiceAsksForANewStreamWhereThePersonIsAndEndsTheOldOne() = runTest {
        val api = FakePlayerApi(PlayerItems.episodeOne)
        api.answers += api.hls()
        api.answers += api.hls()
        val player = player(api)
        player.open(PlayerRequest("i-lantern-1"))
        runCurrent()
        engine.ready()
        engine.positionMs = 1_000_900

        player.chooseQuality(720)
        assertTrue(player.state.value.switching)
        runCurrent()
        val request = api.playbackRequests.last().second
        assertEquals(720, request.maxHeight)
        assertEquals(1000.0, request.startS!!, 0.0)
        assertEquals(1_000_900L, engine.loads.last().second)
        assertEquals(listOf("s-1"), api.stopped)
        assertEquals("s-2", player.activeStream)
        assertEquals(720, player.state.value.selectedQuality)
        assertEquals("the place is saved before switching", 1000.9, api.progress.last().second, 0.0)
        engine.ready()
        assertFalse(player.state.value.switching)

        player.chooseQuality(720)
        runCurrent()
        assertEquals("the same choice again asks nothing", 2, api.playbackRequests.size)
    }

    @Test
    fun anAudioChoiceIsSentAndTheFirstTrackAgainIsTheDefault() = runTest {
        val api = FakePlayerApi(PlayerItems.episodeOne)
        val player = player(api)
        player.open(PlayerRequest("i-lantern-1"))
        runCurrent()
        engine.ready()
        engine.positionMs = 900_000

        player.chooseAudio(2)
        runCurrent()
        assertEquals(2, api.playbackRequests.last().second.audioIndex)
        assertEquals(2, player.state.value.selectedAudio)

        player.chooseAudio(1)
        runCurrent()
        assertNull(api.playbackRequests.last().second.audioIndex)
        assertEquals(1, player.state.value.selectedAudio)
        assertEquals(3, api.playbackRequests.size)
    }

    @Test
    fun anAnswerThatArrivesAfterThePersonMovedOnIsEndedNotPlayed() = runTest {
        val api = FakePlayerApi(PlayerItems.episodeOne)
        val gate = CompletableDeferred<Unit>()
        api.answers += api.hls()
        api.answers += api.held(gate, api.hls())
        api.answers += api.hls()
        val player = player(api)
        player.open(PlayerRequest("i-lantern-1"))
        runCurrent()
        engine.ready()

        player.chooseQuality(720)
        runCurrent()
        player.chooseQuality(480)
        runCurrent()
        gate.complete(Unit)
        runCurrent()

        // Sessions are numbered as the server makes them: s-1 played first, s-2 answered the
        // 480p choice, and s-3 is the 720p answer that came after the person had moved on.
        assertEquals("s-2", player.activeStream)
        assertEquals(listOf("s-1", "s-3"), api.stopped)
        assertEquals(480, api.playbackRequests.last().second.maxHeight)
        assertTrue(engine.loads.none { it.first.url.toString().contains("/s-3/") })
    }

    @Test
    fun aConversionThatArrivesAfterThePlayerClosedIsEnded() = runTest {
        val api = FakePlayerApi(PlayerItems.episodeOne)
        val gate = CompletableDeferred<Unit>()
        api.answers += api.held(gate, api.hls())
        val player = player(api)
        player.open(PlayerRequest("i-lantern-1"))
        runCurrent()
        player.close()
        gate.complete(Unit)
        runCurrent()
        assertTrue("nothing plays after the player left", engine.loads.isEmpty())
        assertEquals("the conversion the server started meanwhile is ended", listOf("s-1"), api.stopped)
    }

    @Test
    fun aForgottenSessionIsAskedForAgainAtTheSamePlace() = runTest {
        val api = FakePlayerApi(PlayerItems.episodeOne)
        api.answers += api.hls()
        api.answers += api.hls()
        val player = player(api)
        player.open(PlayerRequest("i-lantern-1"))
        runCurrent()
        engine.ready()
        engine.positionMs = 1_200_000
        player.pause()
        runCurrent()

        // Paused for over a minute: the server's reaper ended the conversion.
        player.play()
        engine.fail(EngineError.Http(404))
        runCurrent()
        assertEquals(2, api.playbackRequests.size)
        assertEquals(1200.0, api.playbackRequests.last().second.startS!!, 0.0)
        assertEquals(1_200_000L, engine.loads.last().second)
        assertEquals("s-2", player.activeStream)
        assertTrue("s-1" in api.stopped)
        engine.ready()
        assertEquals(PlayerPhase.Ready, player.phase)
    }

    @Test
    fun aVideoThatWentAwayStopsWithThePlainSentence() = runTest {
        val api = FakePlayerApi(PlayerItems.episodeOne)
        api.answers += api.hls()
        api.answers += api.refuse(404, "That no longer exists.")
        val player = player(api)
        player.open(PlayerRequest("i-lantern-1"))
        runCurrent()
        engine.ready()
        engine.fail(EngineError.Http(404))
        runCurrent()
        assertEquals(PlayerPhase.Failed("This video is no longer available.", gone = true), player.phase)
        assertFalse(engine.playWhenReady)
        assertEquals(listOf("s-1"), api.stopped)
        assertTrue("nothing is saved for a video that is gone", api.progress.isEmpty())
    }

    @Test
    fun aSecond404BeforeTheStreamRecoversIsTheEnd() = runTest {
        val api = FakePlayerApi(PlayerItems.episodeOne)
        api.answers += api.hls()
        api.answers += api.hls()
        val player = player(api)
        player.open(PlayerRequest("i-lantern-1"))
        runCurrent()
        engine.ready()
        engine.fail(EngineError.Http(404))
        runCurrent()
        engine.fail(EngineError.Http(404))
        runCurrent()
        assertEquals(PlayerPhase.Failed(PlayerController.GONE, gone = true), player.phase)
        assertEquals(2, api.playbackRequests.size)
    }

    @Test
    fun aRefusedPlaybackShowsTheServersSentenceAndTryAgainAsksAgain() = runTest {
        val api = FakePlayerApi(PlayerItems.episodeOne)
        api.answers += api.refuse(409, "This video could not be read. It may be damaged or still being copied.")
        val player = player(api)
        player.open(PlayerRequest("i-lantern-1"))
        runCurrent()
        assertEquals(
            PlayerPhase.Failed("This video could not be read. It may be damaged or still being copied.", gone = false),
            player.phase,
        )
        player.retry()
        assertEquals(PlayerPhase.Loading, player.phase)
        runCurrent()
        assertEquals(2, api.playbackRequests.size)
        assertEquals("the retry starts where the person meant to", 724.0, api.playbackRequests.last().second.startS!!, 0.0)
        engine.ready()
        assertEquals(PlayerPhase.Ready, player.phase)
    }

    @Test
    fun anItemThatCannotBeReadIsGoneAndTryAgainReadsItAgain() = runTest {
        val api = FakePlayerApi()
        val player = player(api)
        player.open(PlayerRequest("i-hidden"))
        runCurrent()
        assertEquals(PlayerPhase.Failed(PlayerController.GONE, gone = true), player.phase)
        assertTrue(api.playbackRequests.isEmpty())
        api.items["i-hidden"] = PlayerItems.film.copy(id = "i-hidden")
        player.retry()
        runCurrent()
        assertEquals(listOf("i-hidden", "i-hidden"), api.itemReads)
        assertEquals(1, api.playbackRequests.size)
    }

    @Test
    fun aLostConnectionStopsWithASentenceAndTryAgainPicksUpWhereItStopped() = runTest {
        val api = FakePlayerApi(PlayerItems.episodeOne)
        val player = player(api)
        player.open(PlayerRequest("i-lantern-1"))
        runCurrent()
        engine.ready()
        engine.positionMs = 1_500_000
        engine.fail(EngineError.Network)
        runCurrent()
        assertEquals(PlayerPhase.Failed("The stream stopped. Check the connection and try again.", gone = false), player.phase)
        assertEquals("the place is kept", 1500.0, api.progress.last().second, 0.0)
        player.retry()
        runCurrent()
        assertEquals(1500.0, api.playbackRequests.last().second.startS!!, 0.0)
    }

    @Test
    fun aFileThisPhoneCannotDecodeIsAskedForOnceMoreWithTheSafestCodecs() = runTest {
        val api = FakePlayerApi(PlayerItems.film)
        api.answers += api.direct("i-paper")
        api.answers += api.hls()
        val player = player(api)
        player.open(PlayerRequest("i-paper"))
        runCurrent()
        engine.fail(EngineError.Decoding)
        runCurrent()
        val retry = api.playbackRequests.last().second
        assertEquals(listOf("h264"), retry.videoCodecs)
        assertEquals(listOf("aac"), retry.audioCodecs)
        assertTrue(engine.lastSource!!.hls)

        engine.fail(EngineError.Decoding)
        runCurrent()
        assertEquals(PlayerPhase.Failed("This video could not be played on this phone.", gone = false), player.phase)
        assertEquals(2, api.playbackRequests.size)
    }

    @Test
    fun aProgress404MeansTheVideoIsGone() = runTest {
        val api = FakePlayerApi(PlayerItems.episodeOne)
        val player = player(api)
        player.open(PlayerRequest("i-lantern-1"))
        runCurrent()
        engine.ready()
        api.progressError = ApiException(404, "not_found", "That no longer exists.")
        engine.positionMs = 760_000
        advanceTimeBy(10_001)
        assertEquals(PlayerPhase.Failed(PlayerController.GONE, gone = true), player.phase)
    }

    @Test
    fun aFileThatIsNotAVideoIsNotPlayed() = runTest {
        val api = FakePlayerApi(PlayerItems.song)
        val player = player(api)
        player.open(PlayerRequest("i-song"))
        runCurrent()
        assertEquals(PlayerPhase.Failed("This file is not a video, so it cannot play here.", gone = false), player.phase)
        assertTrue(api.playbackRequests.isEmpty())
    }

    @Test
    fun goingToTheBackgroundPausesAndSaves() = runTest {
        val api = FakePlayerApi(PlayerItems.episodeOne)
        val player = player(api)
        player.open(PlayerRequest("i-lantern-1"))
        runCurrent()
        engine.ready()
        engine.positionMs = 812_300
        player.onBackground()
        runCurrent()
        assertFalse(engine.playWhenReady)
        assertEquals(listOf(812.3), api.progress.map { it.second })
    }

    @Test
    fun aPhoneVideoRecordedSidewaysIsShownUpright() = runTest {
        val api = FakePlayerApi(PlayerItems.upright, PlayerItems.film)
        val player = player(api)
        player.open(PlayerRequest("i-upright"))
        runCurrent()
        assertTrue(player.state.value.portrait)
        assertEquals(1080, player.state.value.videoWidth)
        assertEquals(1920, player.state.value.videoHeight)
        player.open(PlayerRequest("i-paper"))
        runCurrent()
        assertFalse(player.state.value.portrait)
    }

    @Test
    fun anotherItemWhilePlayingSavesThePlaceAndEndsTheConversion() = runTest {
        val api = FakePlayerApi(PlayerItems.episodeOne, PlayerItems.film)
        api.answers += api.hls()
        val player = player(api)
        player.open(PlayerRequest("i-lantern-1"))
        runCurrent()
        engine.ready()
        engine.positionMs = 900_000
        player.open(PlayerRequest("i-paper"))
        runCurrent()
        assertEquals(Triple("i-lantern-1", 900.0, 3136.0), api.progress.single())
        assertEquals(listOf("s-1"), api.stopped)
        assertEquals("Paper Boats", player.state.value.title)
        assertNull(player.state.value.subtitle)
    }
}
