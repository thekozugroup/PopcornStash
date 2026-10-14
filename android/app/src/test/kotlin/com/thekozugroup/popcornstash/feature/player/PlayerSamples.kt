package com.thekozugroup.popcornstash.feature.player

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path

/** States as the controller makes them for the dev fixture's items, for the screen's tests and screenshots. */
object PlayerSamples {
    private val audio = PlaybackRequestBuilder.audioChoices(PlayerItems.episodeOne.media)
    private val qualities = PlaybackRequestBuilder.qualities(PlayerItems.episodeOne.media, null, null, deviceMaxHeight = 1080)

    val episode = PlayerUiState(
        title = "The Lantern Keepers",
        subtitle = "S01E01 · Pilot",
        phase = PlayerPhase.Ready,
        playWhenReady = false,
        durationMs = 3_136_000,
        audio = audio,
        selectedAudio = 1,
        qualities = qualities,
        selectedQuality = null,
        converted = true,
        next = NextEpisode("i-lantern-2", "S01E02 · The lantern keepers"),
        videoWidth = 1920,
        videoHeight = 1080,
    )

    val loading = PlayerUiState(title = "The Lantern Keepers", subtitle = "S01E01 · Pilot", phase = PlayerPhase.Loading)

    val film = PlayerUiState(
        title = "Paper Boats",
        phase = PlayerPhase.Ready,
        playWhenReady = false,
        durationMs = 6_532_000,
        videoWidth = 1920,
        videoHeight = 800,
    )

    val upright = PlayerUiState(
        title = "IMG_0042",
        phase = PlayerPhase.Ready,
        playWhenReady = false,
        durationMs = 48_000,
        videoWidth = 1080,
        videoHeight = 1920,
        portrait = true,
    )

    val clock = PlayerClock(positionMs = 724_000, bufferedMs = 1_050_000)

    val uprightClock = PlayerClock(positionMs = 17_000, bufferedMs = 30_000)
}

/**
 * A stand-in for the video's frame (Robolectric cannot decode video): an evening sky over
 * hills, bright at the horizon, so the screenshots show the chrome over a real picture's
 * light rather than over black.
 */
@Composable
fun SampleFrame(modifier: Modifier = Modifier, ratio: Float = 16f / 9f) {
    Box(modifier, contentAlignment = Alignment.Center) {
        Canvas(Modifier.aspectRatio(ratio)) {
            drawRect(
                Brush.verticalGradient(
                    0f to Color(0xFF1B2A49),
                    0.45f to Color(0xFF6D5A8C),
                    0.72f to Color(0xFFF0A86E),
                    0.82f to Color(0xFFFCE3B0),
                ),
            )
            drawCircle(Color(0xFFFFF4D6), radius = size.minDimension * 0.07f, center = Offset(size.width * 0.68f, size.height * 0.7f))
            val far = Path().apply {
                moveTo(0f, size.height * 0.74f)
                cubicTo(size.width * 0.25f, size.height * 0.62f, size.width * 0.45f, size.height * 0.8f, size.width * 0.7f, size.height * 0.72f)
                cubicTo(size.width * 0.85f, size.height * 0.67f, size.width * 0.95f, size.height * 0.7f, size.width, size.height * 0.69f)
                lineTo(size.width, size.height)
                lineTo(0f, size.height)
                close()
            }
            drawPath(far, Color(0xFF3B2F4A))
            val near = Path().apply {
                moveTo(0f, size.height * 0.86f)
                cubicTo(size.width * 0.3f, size.height * 0.78f, size.width * 0.6f, size.height * 0.95f, size.width, size.height * 0.84f)
                lineTo(size.width, size.height)
                lineTo(0f, size.height)
                close()
            }
            drawPath(near, Color(0xFF17121F))
        }
    }
}
