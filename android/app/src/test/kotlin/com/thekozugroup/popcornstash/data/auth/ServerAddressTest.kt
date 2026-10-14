package com.thekozugroup.popcornstash.data.auth

import okhttp3.HttpUrl.Companion.toHttpUrl
import org.junit.Assert.assertEquals
import org.junit.Test

class ServerAddressTest {
    private fun valid(input: String, allowLocalHttp: Boolean = false): String =
        (ServerAddress.parse(input, allowLocalHttp) as ServerAddress.Result.Valid).url.toString()

    private fun invalid(input: String, allowLocalHttp: Boolean = false): String =
        (ServerAddress.parse(input, allowLocalHttp) as ServerAddress.Result.Invalid).message

    @Test
    fun httpsIsAssumedAndTheAddressIsReducedToSchemeHostAndPort() {
        assertEquals("https://stash.example.com/", valid("stash.example.com"))
        assertEquals("https://stash.example.com/", valid("  https://Stash.Example.com/settings/vpn?x=1#top "))
        assertEquals("https://stash.example.com:8443/", valid("stash.example.com:8443/pair"))
        assertEquals("https://stash.example.com/", valid("https://ana:secret@stash.example.com"))
    }

    @Test
    fun plainHttpOnlyForThisPhoneOrTheEmulatorHostInDebugBuilds() {
        assertEquals("Use an https address.", invalid("http://stash.example.com"))
        assertEquals("Use an https address.", invalid("http://192.168.1.20:8788", allowLocalHttp = true))
        assertEquals("Use an https address.", invalid("http://localhost:8788"))
        assertEquals("http://localhost:8788/", valid("http://localhost:8788", allowLocalHttp = true))
        assertEquals("http://10.0.2.2:8788/", valid("http://10.0.2.2:8788", allowLocalHttp = true))
        assertEquals("http://127.0.0.1:8788/", valid("http://127.0.0.1:8788", allowLocalHttp = true))
        assertEquals("Use an https address.", invalid("ftp://stash.example.com"))
    }

    @Test
    fun emptyAndNonsenseAddressesSayWhatIsWrong() {
        assertEquals("Enter the address of your PopcornStash server.", invalid("   "))
        assertEquals("That is not a web address. It looks like stash.example.com.", invalid("https://"))
        assertEquals("That is not a web address. It looks like stash.example.com.", invalid("stash example com"))
    }

    @Test
    fun displayLeavesOutWhatGoesWithoutSaying() {
        assertEquals("stash.example.com", ServerAddress.display("https://stash.example.com/".toHttpUrl()))
        assertEquals("stash.example.com:8443", ServerAddress.display("https://stash.example.com:8443/".toHttpUrl()))
        assertEquals("http://localhost:8788", ServerAddress.display("http://localhost:8788/".toHttpUrl()))
    }
}
