package com.thekozugroup.popcornstash.privacy

import org.junit.Assert.*
import org.junit.Test

class AppLockTest {
    @Test fun coldLaunchAndNewProcessAreLocked() {
        assertFalse(AppLock().unlocked.value)
        val previous = AppLock()
        val screen = Any(); previous.started(screen); previous.completeAuthentication(previous.beginAuthentication(), screen)
        assertFalse(AppLock().unlocked.value)
    }

    @Test fun backgroundRelocksAndInternalNavigationDoesNot() {
        val lock = AppLock()
        val home = Any()
        val player = Any()
        lock.started(home)
        lock.completeAuthentication(lock.beginAuthentication(), home)
        lock.started(player)
        lock.stopped(home, false)
        assertTrue(lock.unlocked.value)
        lock.stopped(player, false)
        assertFalse(lock.unlocked.value)
    }

    @Test fun rotationKeepsTheVisitButScreenOffRelocksIt() {
        val lock = AppLock()
        val old = Any()
        lock.started(old)
        lock.completeAuthentication(lock.beginAuthentication(), old)
        lock.stopped(old, true)
        assertTrue(lock.unlocked.value)
        lock.started(Any())
        lock.lock()
        assertFalse(lock.unlocked.value)
    }
    @Test fun staleOrBackgroundResultsNeverUnlockAnotherVisit() {
        val lock = AppLock()
        val screen = Any()
        lock.started(screen)
        val old = lock.beginAuthentication()
        lock.lock()
        assertFalse(lock.completeAuthentication(old, screen))
        val fresh = lock.beginAuthentication()
        lock.stopped(screen, false, authenticating = true)
        assertFalse(lock.completeAuthentication(fresh, screen))
        lock.started(screen)
        assertTrue(lock.completeAuthentication(fresh, screen))
    }
}
