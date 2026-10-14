package com.thekozugroup.popcornstash.feature.downloads

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** The same rules and sentences as the web's linkDetect.ts. */
class LinkDetectTest {
    private val magnet = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Harbor+Lights+2019"

    @Test
    fun aMagnetIsReadWithItsName() {
        val check = checkLink(magnet) as LinkCheck.Ok
        assertEquals(LinkKind.Magnet, check.link.kind)
        assertEquals("Harbor Lights 2019", check.link.name)
        assertEquals(magnet, check.link.url)
        assertEquals(false, check.extracted)
        assertEquals("Magnet link. Downloads as a torrent, only through the VPN.", describeLink(check.link))
        assertEquals("Magnet link. Downloads as a torrent.", describeLink(check.link, viaVpn = false))
    }

    @Test
    fun aMagnetWithoutItsHashIsTurnedDown() {
        assertEquals(
            LinkCheck.Invalid("This magnet link is missing its torrent hash. Copy the whole link and paste it again."),
            checkLink("magnet:?dn=Harbor"),
        )
    }

    @Test
    fun webLinksAndTorrentFilesAreToldApart() {
        val web = (checkLink("https://www.example.org/media/harbor-lights.mp4") as LinkCheck.Ok).link
        assertEquals(LinkKind.Http, web.kind)
        assertEquals("example.org", web.host)
        assertEquals("harbor-lights.mp4", web.name)
        assertEquals("Web link from example.org. Downloads straight to PopcornStash.", describeLink(web))
        val torrent = (checkLink("https://tracker.example.org/files/puddle-club.torrent") as LinkCheck.Ok).link
        assertEquals(LinkKind.TorrentUrl, torrent.kind)
        assertTrue(torrent.isTorrent())
        assertEquals("Link to a .torrent file on tracker.example.org. Downloads as a torrent, only through the VPN.", describeLink(torrent))
    }

    @Test
    fun theFirstLinkInLongerTextIsUsedWithoutTrailingPunctuation() {
        val check = checkLink("Look at this (https://example.org/clip.mp4).") as LinkCheck.Ok
        assertEquals("https://example.org/clip.mp4", check.link.url)
        assertTrue(check.extracted)
        assertEquals("https://example.org/a", findLink("see https://example.org/a, ok")?.url)
        assertNull(findLink("no link here"))
        assertNull(findLink(null))
    }

    @Test
    fun otherInputIsTurnedDownWithOneSentence() {
        assertEquals(LinkCheck.Empty, checkLink("   "))
        assertEquals(LinkCheck.Invalid("Add https:// to the start of the web address."), checkLink("example.org/file.mp4"))
        assertEquals(LinkCheck.Invalid(ONLY_LINKS_MESSAGE), checkLink("ftp://example.org/file"))
        assertEquals(
            LinkCheck.Invalid("This does not look like a link. Paste a web link or a magnet link."),
            checkLink("harbor lights"),
        )
    }
}
