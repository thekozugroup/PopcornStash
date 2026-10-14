package com.thekozugroup.popcornstash.update

import com.thekozugroup.popcornstash.data.api.baseHttpClient
import kotlinx.coroutines.test.runTest
import mockwebserver3.Dispatcher
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import mockwebserver3.RecordedRequest
import okio.Buffer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.security.MessageDigest

/** Downloading a build and the checks before it may be installed, with a stand-in for PackageManager. */
class UpdateFetcherTest {
    @get:Rule
    val temp = TemporaryFolder()

    private val server = MockWebServer()
    private val ours = "7721ec2f9c281a3bde0e53633e947fa6e89e31ef53cb9816e4dda8ef1224fda4"
    private val theirs = "f288080fcc7845f386f1ff1911a122d6b5285636f5553d2ba5327e018d0275eb"
    private val apk = ByteArray(300_000) { (it * 31 % 251).toByte() }
    private val apkSha = MessageDigest.getInstance("SHA-256").digest(apk).toHex()

    private var shaFile = "$apkSha  popcornstash-android-415.apk\n"
    private var apkCode = 200
    private var apkBody = apk
    private var facts: ApkFacts? = ApkFacts("com.thekozugroup.popcornstash", 415, setOf(ours))
    private var installed = setOf(ours)
    private val inspected = mutableListOf<String>()

    private lateinit var fetcher: UpdateFetcher
    private lateinit var build: AppBuild

    @Before
    fun setUp() {
        server.dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse = when (request.url.encodedPath) {
                "/android-415/popcornstash-android-415.apk.sha256" -> MockResponse.Builder().body(shaFile).build()
                "/android-415/popcornstash-android-415.apk" ->
                    MockResponse.Builder().code(apkCode).body(Buffer().write(apkBody)).build()
                else -> MockResponse.Builder().code(404).build()
            }
        }
        server.start()
        build = UpdateFixtures.build(415, server.url("/android-415").toString())
        fetcher = UpdateFetcher(
            http = baseHttpClient("PopcornStash-Android/test"),
            dir = temp.newFolder("updates"),
            packageName = "com.thekozugroup.popcornstash",
            inspector = { file ->
                inspected += file.name
                facts
            },
            installedSigners = { installed },
        )
    }

    @After
    fun tearDown() = server.close()

    private fun files() = fetcher.dir.list().orEmpty().sorted()

    @Test
    fun aGoodDownloadIsVerifiedAndKept() = runTest {
        val progress = mutableListOf<Pair<Long, Long?>>()
        var verifying = false
        val outcome = fetcher.fetch(build, onProgress = { done, total -> progress += done to total }, onVerifying = { verifying = true })
        outcome as FetchOutcome.Verified
        assertEquals(apkSha, outcome.sha256)
        assertEquals("popcornstash-android-415.apk", outcome.file.name)
        assertTrue(outcome.file.readBytes().contentEquals(apk))
        assertEquals(listOf("popcornstash-android-415.apk"), files())
        assertEquals(listOf("popcornstash-android-415.apk"), inspected)
        assertTrue(verifying)
        // Progress grows to the whole file, about once per percent, not once per buffer.
        assertEquals(apk.size.toLong() to apk.size.toLong(), progress.last())
        assertTrue(progress.zipWithNext().all { (a, b) -> a.first <= b.first })
        assertTrue(progress.size in 2..102)
    }

    @Test
    fun aChecksumThatDoesNotMatchDeletesTheFile() = runTest {
        shaFile = "${"0".repeat(64)}  popcornstash-android-415.apk\n"
        assertEquals(FetchOutcome.Failed(FetchProblem.ChecksumMismatch), fetcher.fetch(build))
        assertEquals(emptyList<String>(), files())
        assertTrue("an unverified file is never inspected", inspected.isEmpty())
    }

    @Test
    fun aChangedFileDoesNotMatchItsChecksum() = runTest {
        apkBody = apk.copyOf().also { it[1000] = (it[1000] + 1).toByte() }
        assertEquals(FetchOutcome.Failed(FetchProblem.ChecksumMismatch), fetcher.fetch(build))
        assertEquals(emptyList<String>(), files())
    }

    @Test
    fun aChecksumFileThatIsNotOneStopsBeforeTheDownload() = runTest {
        shaFile = "<html>Not here</html>"
        assertEquals(FetchOutcome.Failed(FetchProblem.ChecksumMismatch), fetcher.fetch(build))
        assertEquals(1, server.requestCount)
        shaFile = "a".repeat(10_000)
        assertEquals(FetchOutcome.Failed(FetchProblem.ChecksumMismatch), fetcher.fetch(build))
    }

    @Test
    fun aMissingFileIsAStoppedDownload() = runTest {
        apkCode = 404
        assertEquals(FetchOutcome.Failed(FetchProblem.DownloadStopped), fetcher.fetch(build))
        assertEquals(emptyList<String>(), files())
        val gone = build.copy(sha256Url = server.url("/nothing").toString())
        assertEquals(FetchOutcome.Failed(FetchProblem.DownloadStopped), fetcher.fetch(gone))
    }

    @Test
    fun withNoConnectionTheDownloadStops() = runTest {
        server.close()
        assertEquals(FetchOutcome.Failed(FetchProblem.DownloadStopped), fetcher.fetch(build))
        assertEquals(emptyList<String>(), files())
    }

    @Test
    fun anApkSignedWithAnotherKeyIsDeleted() = runTest {
        facts = ApkFacts("com.thekozugroup.popcornstash", 415, setOf(theirs))
        assertEquals(FetchOutcome.Failed(FetchProblem.OtherSigner), fetcher.fetch(build))
        assertEquals(emptyList<String>(), files())
    }

    @Test
    fun anAppWithNoKnownSignerNeverUpdatesItself() = runTest {
        installed = emptySet()
        assertEquals(FetchOutcome.Failed(FetchProblem.OtherSigner), fetcher.fetch(build))
        assertEquals(emptyList<String>(), files())
    }

    @Test
    fun aRotatedKeyWhoseLineageHoldsOursIsAccepted() = runTest {
        facts = ApkFacts("com.thekozugroup.popcornstash", 415, signers = setOf(theirs), lineage = setOf(ours, theirs))
        assertTrue(fetcher.fetch(build) is FetchOutcome.Verified)
    }

    @Test
    fun anotherAppOrAnotherBuildIsNotThisApp() = runTest {
        facts = ApkFacts("com.example.other", 415, setOf(ours))
        assertEquals(FetchOutcome.Failed(FetchProblem.NotThisApp), fetcher.fetch(build))
        facts = ApkFacts("com.thekozugroup.popcornstash", 414, setOf(ours))
        assertEquals(FetchOutcome.Failed(FetchProblem.NotThisApp), fetcher.fetch(build))
        facts = null
        assertEquals(FetchOutcome.Failed(FetchProblem.NotThisApp), fetcher.fetch(build))
        assertEquals(emptyList<String>(), files())
    }

    @Test
    fun anEarlierDownloadIsReplaced() = runTest {
        fetcher.dir.resolve("popcornstash-android-414.apk").writeText("old")
        fetcher.dir.resolve("popcornstash-android-415.apk.part").writeText("half")
        assertTrue(fetcher.fetch(build) is FetchOutcome.Verified)
        assertEquals(listOf("popcornstash-android-415.apk"), files())
        fetcher.clear()
        assertFalse(fetcher.dir.resolve("popcornstash-android-415.apk").exists())
    }

    @Test
    fun theSameSignerRule() {
        val apk = ApkFacts("p", 1, setOf(ours))
        assertTrue(UpdateFetcher.sameSigner(setOf(ours), apk))
        assertFalse(UpdateFetcher.sameSigner(setOf(theirs), apk))
        assertFalse(UpdateFetcher.sameSigner(emptySet(), apk))
        assertFalse(UpdateFetcher.sameSigner(setOf(ours, theirs), apk))
    }
}
