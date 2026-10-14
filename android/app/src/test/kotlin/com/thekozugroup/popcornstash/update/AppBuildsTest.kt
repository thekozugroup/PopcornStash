package com.thekozugroup.popcornstash.update

import com.thekozugroup.popcornstash.update.UpdateFixtures.Asset
import com.thekozugroup.popcornstash.update.UpdateFixtures.appRelease
import com.thekozugroup.popcornstash.update.UpdateFixtures.list
import com.thekozugroup.popcornstash.update.UpdateFixtures.release
import com.thekozugroup.popcornstash.update.UpdateFixtures.serverRelease
import kotlinx.serialization.SerializationException
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** Reading GitHub's release list and choosing the build to offer. */
class AppBuildsTest {
    private fun newest(vararg releases: String) = AppBuilds.newest(AppBuilds.parse(list(*releases)))

    @Test
    fun readsAnAppBuildWithItsFiles() {
        val build = newest(appRelease(415))!!
        assertEquals(415, build.number)
        assertEquals("https://github.com/thekozugroup/PopcornStash/releases/download/android-415/popcornstash-android-415.apk", build.apkUrl)
        assertEquals("https://github.com/thekozugroup/PopcornStash/releases/download/android-415/popcornstash-android-415.apk.sha256", build.sha256Url)
        assertEquals("https://github.com/thekozugroup/PopcornStash/releases/tag/android-415", build.pageUrl)
        assertEquals(8538, build.size)
    }

    @Test
    fun picksTheHighestBuildWhateverTheOrder() {
        assertEquals(415, newest(appRelease(412), appRelease(415), appRelease(413))?.number)
        // Numerically, not as text: 1000 is newer than 999.
        assertEquals(1000, newest(appRelease(999), appRelease(1000))?.number)
    }

    @Test
    fun serverReleasesAndOtherTagsAreNotAppBuilds() {
        assertNull(newest(serverRelease("0.2.0")))
        val oddTags = listOf("android-", "android-abc", "android-0", "android-012", "Android-415", "android-415-rc1", "app-415", "v415")
        for (tag in oddTags) {
            val release = release(tag, listOf(Asset("popcornstash-android-415.apk", "https://x.test/a.apk"), Asset("popcornstash-android-415.apk.sha256", "https://x.test/a.sha256")))
            assertNull(tag, newest(release))
        }
        assertEquals(412, newest(serverRelease("0.3.0"), appRelease(412), serverRelease("0.2.0"))?.number)
    }

    @Test
    fun draftsAreSkipped() {
        assertEquals(414, newest(appRelease(415, draft = true), appRelease(414))?.number)
        assertNull(newest(appRelease(415, draft = true)))
    }

    @Test
    fun aReleaseWithoutBothFilesIsSkipped() {
        val apkOnly = release("android-416", listOf(Asset("popcornstash-android-416.apk", "https://x.test/416.apk")))
        val shaOnly = release("android-417", listOf(Asset("popcornstash-android-417.apk.sha256", "https://x.test/417.sha256")))
        val wrongNumber = release(
            "android-418",
            listOf(Asset("popcornstash-android-417.apk", "https://x.test/417.apk"), Asset("popcornstash-android-417.apk.sha256", "https://x.test/417.sha256")),
        )
        val stillUploading = release(
            "android-419",
            listOf(
                Asset("popcornstash-android-419.apk", "https://x.test/419.apk", state = "open"),
                Asset("popcornstash-android-419.apk.sha256", "https://x.test/419.sha256"),
            ),
        )
        val noAddress = release(
            "android-420",
            listOf(Asset("popcornstash-android-420.apk", "file:///sdcard/420.apk"), Asset("popcornstash-android-420.apk.sha256", "https://x.test/420.sha256")),
        )
        assertEquals(415, newest(apkOnly, shaOnly, wrongNumber, stillUploading, noAddress, appRelease(415))?.number)
    }

    @Test
    fun offersOnlyAHigherBuild() {
        val newest = UpdateFixtures.build(415)
        assertEquals(newest, AppBuilds.offer(newest, installed = 412))
        assertNull(AppBuilds.offer(newest, installed = 415))
        assertNull(AppBuilds.offer(newest, installed = 416))
        assertNull(AppBuilds.offer(null, installed = 1))
    }

    @Test
    fun anEmptyListHasNoBuild() {
        assertNull(newest())
    }

    @Test(expected = SerializationException::class)
    fun somethingElseIsNotAReleaseList() {
        AppBuilds.parse("""{"message":"Not Found","documentation_url":"https://docs.github.com/rest"}""")
    }

    @Test
    fun readsTheChecksumFileAsSha256sumWritesIt() {
        val hex = "7721ec2f9c281a3bde0e53633e947fa6e89e31ef53cb9816e4dda8ef1224fda4"
        assertEquals(hex, AppBuilds.parseSha256("$hex  popcornstash-android-415.apk\n"))
        assertEquals(hex, AppBuilds.parseSha256(hex.uppercase()))
        assertEquals(hex, AppBuilds.parseSha256("  $hex\r\n"))
        assertNull(AppBuilds.parseSha256(""))
        assertNull(AppBuilds.parseSha256("not a checksum"))
        assertNull(AppBuilds.parseSha256(hex.dropLast(1)))
        assertNull(AppBuilds.parseSha256("<html>404</html>"))
    }

    @Test
    fun theVersionLeavesOutTheBuild() {
        assertEquals("0.1.0", versionOf("0.1.0+412"))
        assertEquals("0.1.0", versionOf("0.1.0"))
    }
}
