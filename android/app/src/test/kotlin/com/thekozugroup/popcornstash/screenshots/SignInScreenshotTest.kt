package com.thekozugroup.popcornstash.screenshots

import androidx.activity.ComponentActivity
import androidx.compose.runtime.Composable
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onRoot
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.github.takahirom.roborazzi.captureRoboImage
import com.thekozugroup.popcornstash.data.auth.PairingState
import com.thekozugroup.popcornstash.feature.signin.PairingPage
import com.thekozugroup.popcornstash.feature.signin.ServerAddressPage
import com.thekozugroup.popcornstash.feature.signin.ServerInfo
import com.thekozugroup.popcornstash.feature.signin.SignInMethodPage
import com.thekozugroup.popcornstash.feature.signin.SignInStep
import com.thekozugroup.popcornstash.feature.signin.SignInUiState
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import okhttp3.HttpUrl.Companion.toHttpUrl
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/** The sign-in steps as a phone shows them, light and dark. */
@RunWith(AndroidJUnit4::class)
@Config(qualifiers = PHONE)
class SignInScreenshotTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val server = ServerInfo(
        url = "https://stash.example.com/".toHttpUrl(),
        display = "stash.example.com",
        version = "0.2.0",
        passkeysOffered = true,
        setupRequired = false,
    )

    private fun shoot(name: String, dark: Boolean, content: @Composable () -> Unit) {
        compose.setContent { PopcornTheme(darkTheme = dark, reduceMotion = false, content = content) }
        compose.waitForIdle()
        compose.onRoot().captureRoboImage(screenshot(name))
    }

    private fun serverPage(state: SignInUiState, dark: Boolean, name: String) =
        shoot(name, dark) { ServerAddressPage(state, {}, {}) }

    private fun methodPage(state: SignInUiState, dark: Boolean, name: String) =
        shoot(name, dark) { SignInMethodPage(state, {}, {}, {}, {}, {}) }

    private fun pairingPage(state: SignInUiState, dark: Boolean, name: String) =
        shoot(name, dark) { PairingPage(state, {}, {}, {}) }

    @Test
    fun serverLight() = serverPage(SignInUiState(), dark = false, name = "signin-server-light")

    @Test
    fun serverDark() = serverPage(SignInUiState(address = "stash.example.com"), dark = true, name = "signin-server-dark")

    @Test
    fun serverErrorLight() = serverPage(
        SignInUiState(address = "http://192.168.1.20:8788", addressError = "Use an https address."),
        dark = false,
        name = "signin-server-error-light",
    )

    @Test
    fun signedOutNoticeLight() = serverPage(
        SignInUiState(address = "stash.example.com", notice = "You were signed out. Sign in again."),
        dark = false,
        name = "signin-server-signed-out-light",
    )

    @Test
    fun checkingDark() = serverPage(
        SignInUiState(address = "stash.example.com", checking = true),
        dark = true,
        name = "signin-server-checking-dark",
    )

    @Test
    fun passkeyOrCodeLight() = methodPage(SignInUiState(step = SignInStep.Method, server = server), false, "signin-method-light")

    @Test
    fun passkeyOrCodeDark() = methodPage(SignInUiState(step = SignInStep.Method, server = server), true, "signin-method-dark")

    @Test
    fun codeOnlyLight() = methodPage(
        SignInUiState(step = SignInStep.Method, server = server.copy(passkeysOffered = false)),
        false,
        "signin-method-code-only-light",
    )

    @Test
    fun verificationProblemLight() = verificationProblem(false, "signin-verification-light")

    @Test
    fun verificationProblemDark() = verificationProblem(true, "signin-verification-dark")

    private fun verificationProblem(dark: Boolean, name: String) = methodPage(
        SignInUiState(step = SignInStep.Method, server = server.copy(
            passkeysOffered = false,
            passkeyProblem = "The server lists a different build of this app. Ask its owner to add this build's signing certificate, or pair with a code.",
        )),
        dark,
        name,
    )

    @Test
    fun passkeyFailedDark() = methodPage(
        SignInUiState(
            step = SignInStep.Method,
            server = server,
            methodError = "This phone has no passkey for this server. Pair with a code instead.",
        ),
        true,
        "signin-method-error-dark",
    )

    @Test
    fun setupRequiredLight() = methodPage(
        SignInUiState(step = SignInStep.Method, server = server.copy(passkeysOffered = false, setupRequired = true)),
        false,
        "signin-method-setup-light",
    )

    private val waiting = PairingState.Waiting("K7QX-M2PD", "https://stash.example.com/pair?code=K7QX-M2PD", 4_000_000_000)

    @Test
    fun pairingLight() = pairingPage(SignInUiState(step = SignInStep.Pairing, server = server, pairing = waiting), false, "signin-pairing-light")

    @Test
    fun pairingDark() = pairingPage(SignInUiState(step = SignInStep.Pairing, server = server, pairing = waiting), true, "signin-pairing-dark")

    @Test
    fun pairingExpiredLight() = pairingPage(
        SignInUiState(step = SignInStep.Pairing, server = server, pairing = PairingState.Expired),
        false,
        "signin-pairing-expired-light",
    )

    @Test
    fun pairingDeniedDark() = pairingPage(
        SignInUiState(step = SignInStep.Pairing, server = server, pairing = PairingState.Denied),
        true,
        "signin-pairing-denied-dark",
    )
}
