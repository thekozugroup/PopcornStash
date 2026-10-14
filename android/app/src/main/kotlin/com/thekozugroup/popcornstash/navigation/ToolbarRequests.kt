package com.thekozugroup.popcornstash.navigation

import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.staticCompositionLocalOf

/**
 * Pages that want the floating toolbar out of the way while they show: the photo viewer
 * (android/DESIGN.md, Floating toolbar; the player is its own activity). The toolbar leaves
 * while at least one page asks and comes back when the last one is gone.
 */
@Stable
class ToolbarRequests {
    private val hiders = mutableStateListOf<Any>()

    val hidden: Boolean get() = hiders.isNotEmpty()

    fun hide(owner: Any) {
        if (owner !in hiders) hiders += owner
    }

    fun release(owner: Any) {
        hiders -= owner
    }
}

/** The shell's [ToolbarRequests]; outside a shell, a set nobody reads. */
val LocalToolbarRequests = staticCompositionLocalOf { ToolbarRequests() }

/** Hides the floating toolbar for as long as the caller stays composed. */
@Composable
fun HideToolbarWhileShown() {
    val requests = LocalToolbarRequests.current
    val owner = remember { Any() }
    DisposableEffect(requests, owner) {
        requests.hide(owner)
        onDispose { requests.release(owner) }
    }
}
