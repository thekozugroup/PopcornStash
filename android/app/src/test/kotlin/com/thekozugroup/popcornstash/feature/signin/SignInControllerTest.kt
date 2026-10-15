package com.thekozugroup.popcornstash.feature.signin

import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.ApiException
import com.thekozugroup.popcornstash.data.api.baseHttpClient
import com.thekozugroup.popcornstash.data.auth.PairingState
import com.thekozugroup.popcornstash.data.auth.PasskeyAuthenticator
import com.thekozugroup.popcornstash.data.auth.PasskeyException
import com.thekozugroup.popcornstash.testing.Fixtures
import com.thekozugroup.popcornstash.testing.bodyText
import com.thekozugroup.popcornstash.testing.next
import com.thekozugroup.popcornstash.testing.path
import com.thekozugroup.popcornstash.testing.respond
import com.thekozugroup.popcornstash.testing.withinRealSeconds
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.runTest
import mockwebserver3.MockWebServer
import okhttp3.HttpUrl
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class SignInControllerTest {
    private val server = MockWebServer()
    private val signedIn = mutableListOf<Pair<HttpUrl, String>>()

    @Before
    fun setUp() = server.start()

    @After
    fun tearDown() = server.close()

    private fun TestScope.controller(lastServer: String? = null, notice: String? = null) = SignInController(
        SignInDependencies(
            publicApi = { ApiClient(it, baseHttpClient("t")) },
            finishSignIn = { url, token -> signedIn += url to token },
            deviceName = "Ana's Pixel",
            packageName = "com.thekozugroup.popcornstash",
            allowLocalHttp = true,
            signingFingerprints = setOf("DB:19:CF:5F:75:DB:CD:2C:BE:11:11:5E:67:51:B5:BC:B8:DD:19:FE:F7:F2:9C:89:E3:63:BE:D9:9E:87:9A:C0"),
        ),
        CoroutineScope(UnconfinedTestDispatcher(testScheduler)),
        lastServer,
        notice,
    )

    private fun serverAnswers(passkeysReady: Boolean = true, listsApp: Boolean = true, setupRequired: Boolean = false, rpId: String = "localhost", relations: String = "delegate_permission/common.get_login_creds\",\"delegate_permission/common.handle_all_urls", fingerprint: String = "DB:19:CF:5F:75:DB:CD:2C:BE:11:11:5E:67:51:B5:BC:B8:DD:19:FE:F7:F2:9C:89:E3:63:BE:D9:9E:87:9A:C0") {
        server.respond("""{"status":"ok","version":"0.2.0","source":"x"}""")
        server.respond(
            """{"setup_required":$setupRequired,"authenticated":false,"user":null,"public_url":null,"rp_id":"$rpId",
            "passkeys_ready":$passkeysReady,"reason":null}""",
        )
        if (passkeysReady && rpId == "localhost") {
            val app = if (listsApp) "com.thekozugroup.popcornstash" else "com.example.other"
            server.respond("""[{"relation":["$relations"],"target":{"namespace":"android_app","package_name":"$app","sha256_cert_fingerprints":["$fingerprint"]}}]""")
        }
    }

    private suspend fun SignInController.awaitStep(step: SignInStep) = withinRealSeconds { state.first { it.step == step && !it.checking } }

    private val address get() = "http://localhost:${server.port}"

    @Test
    fun appOnAnotherPortFetchesVerificationFromHttps443() = runTest {
        val addresses = mutableListOf<HttpUrl>()
        val controller = SignInController(
            SignInDependencies(
                publicApi = { addresses += it; ApiClient(server.url("/"), baseHttpClient("t")) },
                finishSignIn = { _, _ -> }, deviceName = "test", packageName = "com.thekozugroup.popcornstash",
                allowLocalHttp = false,
                signingFingerprints = setOf("DB19CF5F75DBCD2CBE11115E6751B5BCB8DD19FEF7F29C89E363BED99E879AC0"),
            ),
            CoroutineScope(UnconfinedTestDispatcher(testScheduler)),
        )
        serverAnswers(rpId = "stash.example.com")
        server.respond("""[{"relation":["delegate_permission/common.get_login_creds","delegate_permission/common.handle_all_urls"],
            "target":{"namespace":"android_app","package_name":"com.thekozugroup.popcornstash",
            "sha256_cert_fingerprints":["DB:19:CF:5F:75:DB:CD:2C:BE:11:11:5E:67:51:B5:BC:B8:DD:19:FE:F7:F2:9C:89:E3:63:BE:D9:9E:87:9A:C0"]}}]""")
        controller.onAddressChange("https://stash.example.com:9446")
        controller.onContinue()
        assertTrue(controller.awaitStep(SignInStep.Method).server!!.passkeysOffered)
        assertEquals(9446, addresses[0].port)
        assertEquals(443, addresses[1].port)
        assertEquals("https", addresses[1].scheme)
        assertEquals("stash.example.com", addresses[1].host)
    }

    @Test
    fun aDifferentPasskeyHostExplainsTheAddressWithoutFetchingIt() = runTest {
        val controller = controller()
        serverAnswers(rpId = "other.example.com")
        controller.onAddressChange(address)
        controller.onContinue()
        val info = controller.awaitStep(SignInStep.Method).server!!
        assertEquals(false, info.passkeysOffered)
        assertTrue(info.passkeyProblem!!.contains("other.example.com"))
        assertEquals(2, server.requestCount)
    }

    @Test
    fun missingPermissionsAndTheWrongSigningCertificateOfferPairing() = runTest {
        val controller = controller()
        serverAnswers(relations = "delegate_permission/common.get_login_creds")
        controller.onAddressChange(address)
        controller.onContinue()
        assertTrue(controller.awaitStep(SignInStep.Method).server!!.passkeyProblem!!.contains("missing a permission"))
        controller.onBack()
        serverAnswers(fingerprint = "11".repeat(32))
        controller.onContinue()
        val info = controller.awaitStep(SignInStep.Method).server!!
        assertEquals(false, info.passkeysOffered)
        assertTrue(info.passkeyProblem!!.contains("different build"))
    }

    @Test
    fun aGoodAddressLeadsToTheWaysToSignIn() = runTest {
        val controller = controller()
        serverAnswers()
        controller.onAddressChange(address)
        controller.onContinue()
        val state = controller.awaitStep(SignInStep.Method)
        val info = state.server!!
        assertTrue(info.passkeysOffered)
        assertEquals("0.2.0", info.version)
        assertEquals("http://localhost:${server.port}", info.display)
        assertEquals("/api/health", server.next().path)
        assertEquals("/api/auth/state", server.next().path)
        assertEquals("/.well-known/assetlinks.json", server.next().path)
    }

    @Test
    fun passkeysAreOfferedOnlyWhenTheServerListsTheApp() = runTest {
        val controller = controller()
        serverAnswers(listsApp = false)
        controller.onAddressChange(address)
        controller.onContinue()
        assertEquals(false, controller.awaitStep(SignInStep.Method).server!!.passkeysOffered)
    }

    @Test
    fun aServerWithoutPasskeysReadyNeedsNoAssetLinks() = runTest {
        val controller = controller()
        serverAnswers(passkeysReady = false)
        controller.onAddressChange(address)
        controller.onContinue()
        assertEquals(false, controller.awaitStep(SignInStep.Method).server!!.passkeysOffered)
        assertEquals(2, server.requestCount)
    }

    @Test
    fun aServerNobodySetUpSaysSo() = runTest {
        val controller = controller()
        serverAnswers(passkeysReady = false, setupRequired = true)
        controller.onAddressChange(address)
        controller.onContinue()
        assertTrue(controller.awaitStep(SignInStep.Method).server!!.setupRequired)
    }

    @Test
    fun addressProblemsShowUnderTheField() = runTest {
        val controller = controller()
        controller.onAddressChange("http://stash.example.com")
        controller.onContinue()
        assertEquals("Use an https address.", controller.state.value.addressError)
        controller.onAddressChange(address)
        assertNull(controller.state.value.addressError)
        server.respond("<html>a router page</html>")
        controller.onContinue()
        val state = withinRealSeconds { controller.state.first { it.addressError != null } }
        assertEquals("PopcornStash did not answer at this address.", state.addressError)
        assertEquals(SignInStep.Server, state.step)
    }

    @Test
    fun theLastServerIsFilledIn() = runTest {
        assertEquals("stash.example.com", controller(lastServer = "https://stash.example.com/").state.value.address)
        assertEquals("You were signed out. Sign in again.", controller(notice = "You were signed out. Sign in again.").state.value.notice)
    }

    @Test
    fun pairingShowsTheCodeAndSignsInWhenApproved() = runTest {
        val controller = controller()
        serverAnswers(passkeysReady = false)
        controller.onAddressChange(address)
        controller.onContinue()
        controller.awaitStep(SignInStep.Method)
        server.respond(
            """{"pairing_id":"p-1","user_code":"K7QX-M2PD","poll_secret":"s","expires_at":4000000000,
            "verify_url":"$address/pair?code=K7QX-M2PD","interval":1}""",
        )
        server.respond("""{"status":"approved","token":"${Fixtures.TOKEN}","scopes":["full"]}""")
        controller.onPair()
        assertEquals(SignInStep.Pairing, controller.state.value.step)
        val waiting = withinRealSeconds { controller.state.first { it.pairing is PairingState.Waiting } }.pairing as PairingState.Waiting
        assertEquals("K7QX-M2PD", waiting.userCode)
        withinRealSeconds { while (signedIn.isEmpty()) kotlinx.coroutines.delay(20) }
        assertEquals(Fixtures.TOKEN, signedIn.single().second)
        assertEquals(server.url("/"), signedIn.single().first)
        repeat(2) { server.next() }
        assertTrue(server.next().bodyText.contains("\"client_kind\":\"android\""))
    }

    @Test
    fun backLeavesPairingAndThenTheServer() = runTest {
        val controller = controller()
        serverAnswers(passkeysReady = false)
        controller.onAddressChange(address)
        controller.onContinue()
        controller.awaitStep(SignInStep.Method)
        server.respond("""{"error":{"code":"rate_limited","message":"Too many tries. Wait a minute and try again."}}""", code = 429)
        controller.onPair()
        withinRealSeconds { controller.state.first { it.pairing is PairingState.Failed } }
        assertTrue(controller.onBack())
        assertEquals(SignInStep.Method, controller.state.value.step)
        assertNull(controller.state.value.pairing)
        assertTrue(controller.onBack())
        assertEquals(SignInStep.Server, controller.state.value.step)
        assertTrue(!controller.onBack())
    }

    @Test
    fun aPasskeySignsInWithABearerSession() = runTest {
        val controller = controller()
        serverAnswers()
        controller.onAddressChange(address)
        controller.onContinue()
        controller.awaitStep(SignInStep.Method)
        server.respond("""{"flow_id":"f-1","options":{"publicKey":{"challenge":"Y2hhbGxlbmdl","rpId":"localhost"}}}""")
        server.respond("""{"user":${Fixtures.OWNER},"token":"${Fixtures.TOKEN}"}""")
        var asked: String? = null
        controller.onPasskey(
            PasskeyAuthenticator { request ->
                asked = request
                """{"id":"cred","rawId":"cred","type":"public-key","response":{"clientDataJSON":"e30","authenticatorData":"AA","signature":"AA"}}"""
            },
        )
        withinRealSeconds { while (signedIn.isEmpty()) kotlinx.coroutines.delay(20) }
        assertEquals("""{"challenge":"Y2hhbGxlbmdl","rpId":"localhost"}""", asked)
        repeat(3) { server.next() }
        assertEquals("/api/auth/login/start", server.next().path)
        val finish = server.next()
        assertTrue(finish.bodyText.contains("\"want_token\":true"))
        assertTrue(finish.bodyText.contains("\"device_name\":\"Ana's Pixel\""))
    }

    @Test
    fun aCancelledPasskeySheetSaysNothingAndAFailureSaysWhy() = runTest {
        val controller = controller()
        serverAnswers()
        controller.onAddressChange(address)
        controller.onContinue()
        controller.awaitStep(SignInStep.Method)
        server.respond("""{"flow_id":"f-1","options":{"publicKey":{"challenge":"x"}}}""")
        controller.onPasskey { throw PasskeyException("Sign-in was cancelled.", cancelled = true) }
        withinRealSeconds { controller.state.first { !it.passkeyBusy } }
        assertNull(controller.state.value.methodError)
        server.respond("""{"flow_id":"f-2","options":{"publicKey":{"challenge":"x"}}}""")
        server.respond("""{"error":{"code":"unauthorized","message":"That passkey is not registered here."}}""", code = 401)
        controller.onPasskey { """{"id":"c","type":"public-key","response":{}}""" }
        val failed = withinRealSeconds { controller.state.first { it.methodError != null } }
        assertEquals("That passkey is not registered here.", failed.methodError)
        assertTrue(signedIn.isEmpty())
        assertTrue(ApiException::class.java.isInstance(ApiException.network()))
    }
}
