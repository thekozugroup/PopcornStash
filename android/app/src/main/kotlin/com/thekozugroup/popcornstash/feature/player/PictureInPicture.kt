package com.thekozugroup.popcornstash.feature.player

import android.app.PendingIntent
import android.app.PictureInPictureParams
import android.app.RemoteAction
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Rect
import android.graphics.drawable.Icon
import android.os.Build
import android.util.Rational
import com.thekozugroup.popcornstash.R
import kotlin.math.roundToInt

/**
 * Picture-in-picture for the player: the window keeps the video's shape and only play and
 * pause (android/DESIGN.md, Player). On Android 12 and later it opens by itself when the
 * person leaves the app while a video plays; earlier, [PlayerActivity] asks for it then.
 */
object PictureInPicture {
    /** The broadcast the window's play and pause button sends to this app only. */
    const val ACTION_CONTROL = "com.thekozugroup.popcornstash.player.PICTURE_IN_PICTURE_CONTROL"
    const val EXTRA_PLAY = "play"

    /** Android keeps the window between 2.39:1 and 1:2.39; a hair inside, so rounding never trips it. */
    private const val WIDEST = 2.38f

    fun supported(context: Context): Boolean =
        context.packageManager.hasSystemFeature(PackageManager.FEATURE_PICTURE_IN_PICTURE)

    /** The window's shape for a [width] x [height] picture, within the allowed range; null while unknown. */
    fun aspect(width: Int, height: Int): Rational? {
        if (width <= 0 || height <= 0) return null
        val ratio = width.toFloat() / height
        val clamped = ratio.coerceIn(1f / WIDEST, WIDEST)
        return if (clamped == ratio) Rational(width, height) else Rational((clamped * 1000).roundToInt(), 1000)
    }

    /**
     * Where the picture sits inside [bounds] when fitted (letterboxed or pillarboxed), so the
     * window grows out of the picture itself rather than out of the black bars.
     */
    fun fittedRect(bounds: Rect, width: Int, height: Int): Rect {
        if (width <= 0 || height <= 0 || bounds.isEmpty) return Rect(bounds)
        val scale = minOf(bounds.width().toFloat() / width, bounds.height().toFloat() / height)
        val w = (width * scale).roundToInt()
        val h = (height * scale).roundToInt()
        val left = bounds.left + (bounds.width() - w) / 2
        val top = bounds.top + (bounds.height() - h) / 2
        return Rect(left, top, left + w, top + h)
    }

    fun params(context: Context, playing: Boolean, controllable: Boolean, aspect: Rational?, sourceRect: Rect?): PictureInPictureParams {
        val builder = PictureInPictureParams.Builder()
        if (aspect != null) builder.setAspectRatio(aspect)
        // The window grows out of the picture itself (Android 12's smoother transition).
        if (sourceRect != null && !sourceRect.isEmpty) builder.setSourceRectHint(sourceRect)
        builder.setActions(if (controllable) listOf(playPause(context, playing)) else emptyList())
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            // Leaving the app while a video plays shrinks it by itself; paused, it does not.
            if (playing && controllable) builder.setAutoEnterEnabled(true) else builder.setAutoEnterEnabled(false)
            builder.setSeamlessResizeEnabled(true)
        }
        return builder.build()
    }

    private fun playPause(context: Context, playing: Boolean): RemoteAction {
        val play = !playing
        val title = if (play) "Play" else "Pause"
        val icon = if (play) R.drawable.ic_pip_play else R.drawable.ic_pip_pause
        val intent = Intent(ACTION_CONTROL).setPackage(context.packageName).putExtra(EXTRA_PLAY, play)
        val pending = PendingIntent.getBroadcast(
            context,
            if (play) REQUEST_PLAY else REQUEST_PAUSE,
            intent,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        return RemoteAction(Icon.createWithResource(context, icon), title, title, pending)
    }

    private const val REQUEST_PLAY = 1
    private const val REQUEST_PAUSE = 2
}
