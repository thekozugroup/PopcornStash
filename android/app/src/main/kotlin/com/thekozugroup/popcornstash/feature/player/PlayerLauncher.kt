package com.thekozugroup.popcornstash.feature.player

import android.content.Context
import android.content.Intent
import android.os.SystemClock
import java.util.concurrent.atomic.AtomicLong

/**
 * How the rest of the app opens the player: Home's "Continue watching", a library, a show and
 * an item page call [openPlayer] with the item's id. The player reads the item itself, so the
 * caller passes nothing else; [fromStart] is "Start from the beginning".
 */
object PlayerLauncher {
    private const val EXTRA_ITEM = "com.thekozugroup.popcornstash.player.ITEM"
    private const val EXTRA_FROM_START = "com.thekozugroup.popcornstash.player.FROM_START"
    private const val EXTRA_NONCE = "com.thekozugroup.popcornstash.player.NONCE"
    private val taps = AtomicLong()

    fun intent(context: Context, itemId: String, fromStart: Boolean = false): Intent =
        Intent(context, PlayerActivity::class.java)
            .addFlags(Intent.FLAG_ACTIVITY_NO_USER_ACTION)
            .putExtra(EXTRA_ITEM, itemId)
            .putExtra(EXTRA_FROM_START, fromStart)
            // Two taps on the same item are two requests; the same intent seen again (the
            // activity made anew) is not. The counter keeps two taps apart within one clock tick.
            .putExtra(EXTRA_NONCE, SystemClock.elapsedRealtimeNanos() + taps.incrementAndGet())

    /** The request an intent carries, or null when it names no item. */
    fun request(intent: Intent?): PlayerRequest? {
        val itemId = intent?.getStringExtra(EXTRA_ITEM)?.takeIf { it.isNotBlank() } ?: return null
        return PlayerRequest(itemId, intent.getBooleanExtra(EXTRA_FROM_START, false), intent.getLongExtra(EXTRA_NONCE, 0))
    }
}

/** Plays [itemId] full screen, from where the person stopped unless [fromStart]. */
fun Context.openPlayer(itemId: String, fromStart: Boolean = false) {
    startActivity(PlayerLauncher.intent(this, itemId, fromStart))
}
