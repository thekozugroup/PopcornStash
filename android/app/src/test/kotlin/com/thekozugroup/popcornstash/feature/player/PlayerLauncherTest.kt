package com.thekozugroup.popcornstash.feature.player

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.pm.ActivityInfo
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric

/** How the rest of the app opens the player, and how the player is declared. */
@RunWith(AndroidJUnit4::class)
class PlayerLauncherTest {
    private val context = ApplicationProvider.getApplicationContext<Context>()

    @Test fun openingAWideVideoDoesNotForceLandscape() {
        val activity = Robolectric.buildActivity(PlayerActivity::class.java, PlayerLauncher.intent(context, "i-lantern-1")).create()
        assertEquals(ActivityInfo.SCREEN_ORIENTATION_USER, activity.get().requestedOrientation)
        activity.destroy()
    }

    @Test
    fun anIntentCarriesTheItemAndWhereToStart() {
        val resume = PlayerLauncher.intent(context, "i-lantern-1")
        assertEquals(ComponentName(context, PlayerActivity::class.java), resume.component)
        val request = PlayerLauncher.request(resume)!!
        assertEquals("i-lantern-1", request.itemId)
        assertFalse(request.fromStart)
        assertTrue(PlayerLauncher.request(PlayerLauncher.intent(context, "i-lantern-1", fromStart = true))!!.fromStart)
    }

    @Test
    fun twoTapsAreTwoRequestsButTheSameIntentReadTwiceIsOne() {
        val first = PlayerLauncher.intent(context, "i-paper")
        val second = PlayerLauncher.intent(context, "i-paper")
        assertEquals(PlayerLauncher.request(first), PlayerLauncher.request(Intent(first)))
        assertNotEquals(PlayerLauncher.request(first), PlayerLauncher.request(second))
    }

    @Test
    fun anIntentWithoutAnItemOpensNothing() {
        assertNull(PlayerLauncher.request(Intent(context, PlayerActivity::class.java)))
        assertNull(PlayerLauncher.request(null))
    }

    @Test
    fun thePlayerIsPrivateSingleAndKeepsPlayingThroughSizeChanges() {
        val info = context.packageManager.getActivityInfo(ComponentName(context, PlayerActivity::class.java), 0)
        assertFalse("no other app can open the player", info.exported)
        assertEquals(ActivityInfo.LAUNCH_SINGLE_TASK, info.launchMode)
        val handled = ActivityInfo.CONFIG_ORIENTATION or ActivityInfo.CONFIG_SCREEN_SIZE or
            ActivityInfo.CONFIG_SMALLEST_SCREEN_SIZE or ActivityInfo.CONFIG_SCREEN_LAYOUT or ActivityInfo.CONFIG_UI_MODE
        assertEquals(handled, info.configChanges and handled)
    }
}
