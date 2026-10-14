package com.thekozugroup.popcornstash.feature.signin

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performImeAction
import androidx.compose.ui.test.performTextInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.thekozugroup.popcornstash.data.auth.PairingState
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import okhttp3.HttpUrl.Companion.toHttpUrl
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

@RunWith(AndroidJUnit4::class)
@Config(qualifiers = "w411dp-h891dp-xxhdpi")
class SignInScreensTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val server = ServerInfo("https://stash.example.com/".toHttpUrl(), "stash.example.com", "0.2.0", true, false)

    @Test
    fun theAddressFieldSendsWhatIsTypedAndGoContinues() {
        var typed = ""
        var continued = 0
        compose.setContent {
            PopcornTheme(darkTheme = false, reduceMotion = true) {
                ServerAddressPage(SignInUiState(address = typed), { typed = it }, { continued++ })
            }
        }
        compose.onNodeWithTag("server address").performTextInput("stash.example.com")
        assertEquals("stash.example.com", typed)
        compose.onNodeWithTag("server address").performImeAction()
        compose.onNodeWithText("Continue").performClick()
        assertEquals(2, continued)
    }

    @Test
    fun anAddressErrorShowsUnderTheField() {
        compose.setContent {
            PopcornTheme(darkTheme = false, reduceMotion = true) {
                ServerAddressPage(SignInUiState(address = "http://x", addressError = "Use an https address."), {}, {})
            }
        }
        compose.onNodeWithText("Use an https address.").assertIsDisplayed()
    }

    @Test
    fun passkeyAndCodeAreBothOffered() {
        var passkey = 0
        var pair = 0
        compose.setContent {
            PopcornTheme(darkTheme = false, reduceMotion = true) {
                SignInMethodPage(SignInUiState(step = SignInStep.Method, server = server), { passkey++ }, { pair++ }, {}, {}, {})
            }
        }
        compose.onNodeWithText("Sign in with a passkey").performClick()
        compose.onNodeWithText("Pair with a code").performClick()
        assertEquals(1, passkey)
        assertEquals(1, pair)
        compose.onNodeWithContentDescription("Back").assertIsDisplayed()
    }

    @Test
    fun withoutPasskeysTheSentenceSaysWhy() {
        compose.setContent {
            PopcornTheme(darkTheme = false, reduceMotion = true) {
                SignInMethodPage(SignInUiState(step = SignInStep.Method, server = server.copy(passkeysOffered = false)), {}, {}, {}, {}, {})
            }
        }
        compose.onNodeWithText("Passkeys work in the app once the server lists it. Pair with a code instead.").assertIsDisplayed()
        compose.onNodeWithText("Sign in with a passkey").assertDoesNotExist()
    }

    @Test
    fun theCodeIsReadOutLetterByLetter() {
        var opened = 0
        compose.setContent {
            PopcornTheme(darkTheme = false, reduceMotion = true) {
                PairingPage(
                    SignInUiState(step = SignInStep.Pairing, server = server, pairing = PairingState.Waiting("K7QX-M2PD", "https://x/pair", 0)),
                    { opened++ },
                    {},
                    {},
                )
            }
        }
        compose.onNodeWithContentDescription("Code K 7 Q X, M 2 P D").assertIsDisplayed()
        compose.onNodeWithText("Waiting for approval").assertIsDisplayed()
        compose.onNodeWithText("Open PopcornStash to approve").performClick()
        assertEquals(1, opened)
    }

    @Test
    fun anExpiredCodeOffersToStartAgain() {
        var again = 0
        compose.setContent {
            PopcornTheme(darkTheme = false, reduceMotion = true) {
                PairingPage(SignInUiState(step = SignInStep.Pairing, server = server, pairing = PairingState.Expired), {}, { again++ }, {})
            }
        }
        compose.onNodeWithText("This code expired.").assertIsDisplayed()
        compose.onNodeWithText("Start again").performClick()
        assertEquals(1, again)
    }
}
