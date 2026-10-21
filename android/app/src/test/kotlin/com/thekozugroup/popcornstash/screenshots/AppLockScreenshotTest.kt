package com.thekozugroup.popcornstash.screenshots

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onRoot
import com.github.takahirom.roborazzi.captureRoboImage
import com.thekozugroup.popcornstash.privacy.AppLockScreen
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.robolectric.annotation.Config

@RunWith(AndroidJUnit4::class)
@Config(qualifiers = PHONE)
class AppLockScreenshotTest {
    @get:Rule val compose = createAndroidComposeRule<ComponentActivity>()
    private fun shoot(name: String, dark: Boolean, secure: Boolean, problem: String? = null) {
        compose.setContent { PopcornTheme(darkTheme = dark, reduceMotion = true) { AppLockScreen(secure, false, problem, {}, {}) } }
        compose.waitForIdle()
        compose.onRoot().captureRoboImage(screenshot(name))
    }
    @Test fun lockedLight() = shoot("app-lock-light", false, true)
    @Test fun lockedDark() = shoot("app-lock-dark", true, true)
    @Test fun noCredential() = shoot("app-lock-setup-light", false, false)
    @Test fun verificationError() = shoot("app-lock-error-dark", true, true, "Biometric verification is unavailable. Check your biometrics in Settings.")
}
