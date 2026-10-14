package com.thekozugroup.popcornstash.feature.downloads

import com.thekozugroup.popcornstash.data.api.VpnState
import com.thekozugroup.popcornstash.data.api.VpnStatus
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** The VPN line says what the web says (web/src/lib/vpn.ts, components/downloads/VpnLine.tsx). */
class VpnTextTest {
    private val nord = DownloadsSamples.vpn

    @Test
    fun protectedNamesTheProviderAndCountryOnce() {
        val line = vpnLine(nord, unavailable = false, isOwner = true)
        assertEquals(VpnTone.Connected, line.tone)
        assertEquals("Protected · NordVPN · Netherlands", line.label)
        assertEquals("Torrents run only through the VPN. Public address 203.0.113.5.", line.detail)
        assertTrue(line.opensSettings)
        assertFalse(vpnLine(nord, unavailable = false, isOwner = false).opensSettings)
    }

    @Test
    fun anUploadedFileKeepsItsLabelAndTheCountryIsNotRepeated() {
        val file = VpnStatus(state = VpnState.Connected, provider = "wireguard", label = "WireGuard · home-router", country = null)
        assertEquals("WireGuard · home-router", vpnName(file))
        val withCountry = nord.copy(provider = "openvpn", label = "OpenVPN · Netherlands · nl812")
        assertEquals("OpenVPN · Netherlands · nl812", vpnName(withCountry))
    }

    @Test
    fun connectingAndDisconnectedPauseTorrents() {
        val connecting = vpnLine(nord.copy(state = VpnState.Connecting), unavailable = false, isOwner = true)
        assertEquals("Torrents paused: VPN connecting", connecting.label)
        assertEquals("Torrents wait until the VPN connects. Web links and uploads still work.", connecting.detail)
        assertFalse(connecting.opensSettings)
        assertEquals(VpnTone.Warning, connecting.tone)
        assertEquals("Torrents paused: VPN disconnected", vpnLine(nord.copy(state = VpnState.Disconnected), false, true).label)
    }

    @Test
    fun ownersAndMembersReadDifferentSentencesForAMissingVpn() {
        val missing = VpnStatus(state = VpnState.NotConfigured)
        assertEquals("Torrents blocked: no VPN", vpnLine(missing, false, true).label)
        assertEquals("Set up a VPN to start torrents. Web links and uploads still work.", vpnLine(missing, false, true).detail)
        assertEquals(
            "Torrents need a VPN, and the owner has not set one up yet. Web links and uploads still work.",
            vpnLine(missing, false, false).detail,
        )
    }

    @Test
    fun anErrorShowsTheServersReasonToTheOwnerOnly() {
        val error = nord.copy(state = VpnState.Error, error = "The handshake did not complete.")
        assertEquals("The handshake did not complete.", vpnLine(error, false, true).detail)
        assertEquals("Torrents are stopped until the owner fixes the VPN. Web links and uploads still work.", vpnLine(error, false, false).detail)
    }

    @Test
    fun aTunnelThatIsNotProtectingBlocksTorrents() {
        val line = vpnLine(nord.copy(protected = false), false, true)
        assertEquals("Torrents blocked: VPN not protecting", line.label)
        assertEquals(VpnTone.Error, line.tone)
        assertTrue(torrentsWait(nord.copy(protected = false)))
        assertFalse(torrentsWait(nord))
    }

    @Test
    fun torrentsOffByDesignSayWhyAndWhatStillWorks() {
        val off = VpnStatus(state = VpnState.Unreachable, torrentsOff = true, error = "The container has no NET_ADMIN.")
        val line = vpnLine(off, false, false)
        assertEquals("Torrents off", line.label)
        assertEquals("The container has no NET_ADMIN. Web links and uploads still work.", line.detail)
        val plain = vpnLine(off.copy(error = "Torrents are turned off on this server."), false, true)
        assertEquals("Web links and uploads still work.", plain.detail)
        assertFalse(torrentsWait(off))
    }

    @Test
    fun noVpnModeIsAPermanentWarning() {
        val noVpn = VpnStatus(state = VpnState.NotConfigured, allowNoVpn = true, configured = true)
        assertTrue(isNoVpnMode(noVpn))
        val owner = vpnLine(noVpn, false, true)
        assertEquals("No VPN: torrents use this network", owner.label)
        assertEquals("STASH_ALLOW_NO_VPN is on. Your router must protect the traffic.", owner.detail)
        assertEquals("The server is set up this way.", vpnLine(noVpn, false, false).detail)
        assertFalse(torrentsWait(noVpn))
    }

    @Test
    fun aServerThatDoesNotAnswerHasNoVpnLineUnlessTheStatusIsKnown() {
        assertEquals(null, pageVpnLine(null, asked = true, serverQuiet = true, isOwner = true))
        assertEquals("VPN status unavailable", pageVpnLine(null, asked = true, serverQuiet = false, isOwner = true)?.label)
        assertEquals("Checking VPN", pageVpnLine(null, asked = false, serverQuiet = false, isOwner = true)?.label)
        val connected = VpnStatus(state = VpnState.Connected, provider = "nordvpn", country = "Netherlands")
        assertEquals("Protected · NordVPN · Netherlands", pageVpnLine(connected, asked = true, serverQuiet = true, isOwner = true)?.label)
    }

    @Test
    fun anUnreadableStatusIsSaidPlainlyAndOnce() {
        val checking = vpnLine(null, unavailable = false, isOwner = true)
        assertEquals("Checking VPN", checking.label)
        assertEquals("The label says it all while checking.", "", checking.detail)
        val unavailable = vpnLine(null, unavailable = true, isOwner = true)
        assertEquals("VPN status unavailable", unavailable.label)
        assertEquals("Web links and uploads still work.", unavailable.detail)
    }
}
