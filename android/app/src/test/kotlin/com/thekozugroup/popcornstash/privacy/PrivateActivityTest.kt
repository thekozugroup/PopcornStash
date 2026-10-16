package com.thekozugroup.popcornstash.privacy

import android.app.Activity
import android.app.KeyguardManager
import android.content.Context
import android.view.WindowManager
import com.thekozugroup.popcornstash.PopcornStashApplication
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28])
class PrivateActivityTest {
    class Screen : PrivateActivity()

    private val app get() = (RuntimeEnvironment.getApplication() as PopcornStashApplication)
    private val keyguard get() = shadowOf(app.getSystemService(Context.KEYGUARD_SERVICE) as KeyguardManager)

    @Before fun prepare() {
        app.appLock.lock()
        keyguard.setIsDeviceSecure(true)
        keyguard.setIsDeviceLocked(false)
    }

    @Test fun cancelledChallengeAndBackgroundCannotBypassLock() {
        val controller = Robolectric.buildActivity(Screen::class.java).setup()
        val activity = controller.get()
        val challenge = shadowOf(activity).nextStartedActivityForResult
        assertNotNull(challenge)
        assertEquals("android.app.action.CONFIRM_DEVICE_CREDENTIAL", challenge.intent.action)
        assertFalse(app.appLock.unlocked.value)
        shadowOf(activity).receiveResult(challenge.intent, Activity.RESULT_CANCELED, null)
        assertFalse(app.appLock.unlocked.value)
        controller.pause().stop().destroy()
    }

    @Test fun onlySuccessfulDeviceConfirmationUnlocksAndReturnRequiresAnother() {
        val controller = Robolectric.buildActivity(Screen::class.java).setup()
        val activity = controller.get()
        val challenge = shadowOf(activity).nextStartedActivityForResult
        shadowOf(activity).receiveResult(challenge.intent, Activity.RESULT_OK, null)
        assertTrue(app.appLock.unlocked.value)
        assertTrue(activity.window.attributes.flags and WindowManager.LayoutParams.FLAG_SECURE != 0)
        controller.pause().stop()
        assertFalse(app.appLock.unlocked.value)
        controller.start().resume()
        assertNotNull(shadowOf(activity).nextStartedActivityForResult)
        assertFalse(app.appLock.unlocked.value)
        controller.pause().stop().destroy()
    }

    @Test fun screenOffRejectsALateSuccessfulCredentialResult() {
        val controller = Robolectric.buildActivity(Screen::class.java).setup()
        val activity = controller.get()
        val challenge = shadowOf(activity).nextStartedActivityForResult
        app.sendBroadcast(android.content.Intent(android.content.Intent.ACTION_SCREEN_OFF))
        shadowOf(android.os.Looper.getMainLooper()).idle()
        shadowOf(activity).receiveResult(challenge.intent, Activity.RESULT_OK, null)
        assertFalse(app.appLock.unlocked.value)
        controller.pause().stop().destroy()
    }

    @Test fun homeHintLocksBeforeTheStopAnimationCompletes() {
        val controller = Robolectric.buildActivity(Screen::class.java).setup()
        val activity = controller.get()
        val challenge = shadowOf(activity).nextStartedActivityForResult
        shadowOf(activity).receiveResult(challenge.intent, Activity.RESULT_OK, null)
        assertTrue(app.appLock.unlocked.value)
        org.robolectric.util.ReflectionHelpers.callInstanceMethod<Unit>(activity, "onUserLeaveHint")
        assertFalse(app.appLock.unlocked.value)
        controller.pause().stop().destroy()
    }

    @Test fun noDeviceLockNeverGrantsAccess() {
        keyguard.setIsDeviceSecure(false)
        val controller = Robolectric.buildActivity(Screen::class.java).setup()
        assertNull(shadowOf(controller.get()).nextStartedActivityForResult)
        assertFalse(app.appLock.unlocked.value)
        controller.pause().stop().destroy()
    }
}
