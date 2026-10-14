package com.thekozugroup.popcornstash.ui.theme

import androidx.compose.runtime.Immutable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/**
 * The player's chrome (android/DESIGN.md, Player): controls on `mediaControl` discs (circles)
 * and plates (16dp panels) over the black stage. The player is always dark, whatever the
 * system theme, because the picture decides the light in the room.
 */
@Immutable
data class PlayerTokens(
    /**
     * Close, the audio and quality menu, and the height of every plate in the top and bottom
     * rows, so discs and plates line up (the floating toolbar's square size).
     */
    val disc: Dp = 52.dp,
    /** Play and pause in the centre. */
    val playDisc: Dp = 72.dp,
    val playIcon: Dp = 32.dp,
    /** The loading ring inside the play disc. */
    val loadingRing: Dp = 28.dp,
    /** The waiting ring's disc in picture-in-picture, where nothing else shows. */
    val pictureInPictureDisc: Dp = 32.dp,
    /** Distance from the screen's safe edges to the chrome. */
    val edge: Dp = 16.dp,
    /** Between a disc and the plate beside it. */
    val gap: Dp = 8.dp,
    val plateHorizontalPadding: Dp = 16.dp,
    /** The seek bar: track height and thumb size; the touch target stays 48dp. */
    val seekTrack: Dp = 4.dp,
    val seekThumb: Dp = 16.dp,
    /** Widest the end and error messages grow, so they stay a short column on a wide screen. */
    val messageWidth: Dp = 420.dp,
    /** Controls fade away this long after the last touch while playing (TalkBack keeps them). */
    val hideAfterMs: Long = 3_000,
    /** How often the seek bar and times move while they show. */
    val tickMs: Long = 250,
)

val LocalPlayerTokens = staticCompositionLocalOf { PlayerTokens() }
