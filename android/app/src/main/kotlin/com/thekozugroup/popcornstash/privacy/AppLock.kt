package com.thekozugroup.popcornstash.privacy

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow

/** A foreground visit only. No credential or unlocked state is written to disk. */
class AppLock {
    private val visible = mutableSetOf<Any>()
    private var generation = 0L
    private val mutableUnlocked = MutableStateFlow(false)
    val unlocked = mutableUnlocked.asStateFlow()

    fun started(screen: Any) { visible.add(screen) }

    fun stopped(screen: Any, changingConfiguration: Boolean, authenticating: Boolean = false) {
        visible.remove(screen)
        if (visible.isEmpty() && !changingConfiguration) {
            mutableUnlocked.value = false
            if (!authenticating) generation++
        }
    }

    fun beginAuthentication(): Long = generation

    fun completeAuthentication(attempt: Long, screen: Any): Boolean {
        if (attempt != generation || screen !in visible) return false
        mutableUnlocked.value = true
        return true
    }

    fun lock() { generation++; mutableUnlocked.value = false }
}
