package com.thekozugroup.popcornstash.privacy

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
    class Screen : PrivateActivity() {
        var available = true
        var result: ((Boolean) -> Unit)? = null
        override fun hasStrongBiometrics() = available && getSystemService(KeyguardManager::class.java).isDeviceSecure
        override fun confirmBiometric(signal: android.os.CancellationSignal, onResult: (Boolean) -> Unit) { result = onResult }
    }

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
        val challenge = activity.result
        assertNotNull(challenge)
        assertNull(shadowOf(activity).nextStartedActivityForResult)
        assertFalse(app.appLock.unlocked.value)
        challenge!!(false)
        assertFalse(app.appLock.unlocked.value)
        controller.pause().stop().destroy()
    }

    @Test fun onlySuccessfulBiometricsUnlocksAndReturnRequiresAnother() {
        val controller = Robolectric.buildActivity(Screen::class.java).setup()
        val activity = controller.get()
        val challenge = activity.result
        challenge!!(true)
        assertTrue(app.appLock.unlocked.value)
        assertTrue(activity.window.attributes.flags and WindowManager.LayoutParams.FLAG_SECURE != 0)
        controller.pause().stop()
        assertFalse(app.appLock.unlocked.value)
        controller.start().resume()
        assertNotNull(activity.result)
        assertFalse(app.appLock.unlocked.value)
        controller.pause().stop().destroy()
    }

    @Test fun screenOffRejectsALateSuccessfulBiometricResult() {
        val controller = Robolectric.buildActivity(Screen::class.java).setup()
        val activity = controller.get()
        val challenge = activity.result
        app.sendBroadcast(android.content.Intent(android.content.Intent.ACTION_SCREEN_OFF))
        shadowOf(android.os.Looper.getMainLooper()).idle()
        challenge!!(true)
        assertFalse(app.appLock.unlocked.value)
        controller.pause().stop().destroy()
    }

    @Test fun homeHintLocksBeforeTheStopAnimationCompletes() {
        val controller = Robolectric.buildActivity(Screen::class.java).setup()
        val activity = controller.get()
        val challenge = activity.result
        challenge!!(true)
        assertTrue(app.appLock.unlocked.value)
        org.robolectric.util.ReflectionHelpers.callInstanceMethod<Unit>(activity, "onUserLeaveHint")
        assertFalse(app.appLock.unlocked.value)
        controller.pause().stop().destroy()
    }

    @Test fun noDeviceLockNeverGrantsAccess() {
        keyguard.setIsDeviceSecure(false)
        val controller = Robolectric.buildActivity(Screen::class.java).setup()
        assertNull(controller.get().result)
        assertFalse(app.appLock.unlocked.value)
        controller.pause().stop().destroy()
    }
}
