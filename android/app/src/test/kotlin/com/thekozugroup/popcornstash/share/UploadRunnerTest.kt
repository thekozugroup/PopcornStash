package com.thekozugroup.popcornstash.share

import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.baseHttpClient
import kotlinx.coroutines.runBlocking
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import org.junit.After
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder

/** A confirmed batch, file by file, for the person who confirmed it and nobody else. */
class UploadRunnerTest {
    @get:Rule
    val folder = TemporaryFolder()

    private val server = MockWebServer()
    private val fake = FakeUploadServer(chunkSize = 4)
    private lateinit var api: ApiClient
    private lateinit var store: UploadBatchStore
    private val contents = mapOf(
        "content://media/1" to "first file".toByteArray(),
        "content://media/2" to "second, longer file".toByteArray(),
    )
    private val opened = mutableListOf<BytesSource>()

    private val batch = UploadBatch(
        id = "batch-1",
        userId = "u-ana",
        userName = "Ana Ribeiro",
        server = "https://stash.example.com/",
        files = listOf(
            SharedFile("content://media/1", "first.txt", size = 10),
            // The provider did not say how big this one is.
            SharedFile("content://media/2", "second.txt", size = null),
        ),
    )

    @Before
    fun setUp() {
        fake.install(server)
        server.start()
        api = ApiClient(server.url("/"), baseHttpClient("test"))
        store = UploadBatchStore(folder.newFolder("uploads"))
    }

    @After
    fun tearDown() = server.close()

    private fun runner(apiFor: suspend (UploadBatch) -> ApiClient? = { api }, open: (SharedFile) -> ChunkSource = ::source) =
        UploadRunner(
            store = store,
            apiFor = apiFor,
            open = open,
            measure = { file -> contents.getValue(file.uri).size.toLong() },
            uploader = { Uploader(it, retryDelays = listOf(0)) },
        )

    private fun source(file: SharedFile): ChunkSource = BytesSource(contents.getValue(file.uri)).also { opened += it }

    @Test
    fun everyFileGoesUpAndTheBatchIsForgotten() = runBlocking {
        store.save(batch)
        val progress = mutableListOf<UploadBatchProgress>()
        val outcome = runner().run("batch-1") { progress += it }
        assertEquals(UploadOutcome.Done(2), outcome)
        assertArrayEquals(contents.getValue("content://media/1"), fake.uploads.getValue("u1").body.readByteArray())
        assertArrayEquals(contents.getValue("content://media/2"), fake.uploads.getValue("u2").body.readByteArray())
        assertNull(store.load("batch-1"))
        assertEquals(2, progress.last().doneFiles)
        assertEquals(29L, progress.last().totalBytes)
        assertTrue(opened.all { it.closed })
    }

    @Test
    fun nobodyOrSomeoneElseSignedInSendsNothing() = runBlocking {
        store.save(batch)
        val outcome = runner(apiFor = { null }).run("batch-1") {}
        assertEquals(UploadOutcome.NotSignedIn, outcome)
        assertTrue(fake.requests.isEmpty())
        assertNull(store.load("batch-1"))
    }

    @Test
    fun aFileAlreadyDoneIsSkippedAfterARestart() = runBlocking {
        store.save(batch.copy(files = listOf(batch.files[0].copy(done = true, uploadId = "u-old"), batch.files[1])))
        assertEquals(UploadOutcome.Done(2), runner().run("batch-1") {})
        assertEquals(listOf("second.txt"), fake.uploads.values.map { it.filename })
    }

    @Test
    fun theUploadIdIsKeptSoAnInterruptedJobContinues() = runBlocking {
        store.save(batch)
        // The connection drops on the second file's first chunk.
        fake.override("PUT", "/api/uploads/u2", MockResponse.Builder().code(503).build())
        fake.override("PUT", "/api/uploads/u2", MockResponse.Builder().code(503).build())
        val outcome = runner().run("batch-1") {}
        assertTrue(outcome is UploadOutcome.RetryLater)
        val kept = store.load("batch-1")!!
        assertTrue(kept.files[0].done)
        assertEquals("u2", kept.files[1].uploadId)
        assertEquals(19L, kept.files[1].size)
        // The next run goes on with the same upload.
        assertEquals(UploadOutcome.Done(2), runner().run("batch-1") {})
        assertEquals(2, fake.uploads.size)
    }

    @Test
    fun aReadGrantThatIsGoneAsksToShareAgainAndTheServerDropsThePart() = runBlocking {
        store.save(batch)
        var reads = 0
        val outcome = runner(open = { file ->
            object : ChunkSource {
                override fun read(offset: Long, length: Int): ByteArray {
                    if (file.uri.endsWith("2") && ++reads > 1) throw SourceUnreadableException("gone")
                    return contents.getValue(file.uri).copyOfRange(offset.toInt(), (offset + length).toInt().coerceAtMost(contents.getValue(file.uri).size))
                }

                override fun close() = Unit
            }
        }).run("batch-1") {}
        assertEquals(UploadOutcome.SourceGone("second.txt"), outcome)
        assertTrue(fake.requests.any { it.method == "DELETE" && it.url.encodedPath == "/api/uploads/u2" })
        assertNull(store.load("batch-1"))
    }

    @Test
    fun aBatchRemovedWhileRunningStopsBeforeTheNextRequest() = runBlocking {
        store.save(batch)
        val outcome = runner().run("batch-1") { progress ->
            // Signing out removes the batch while the first file is done.
            if (progress.doneFiles == 1) store.delete("batch-1")
        }
        assertEquals(UploadOutcome.Stopped, outcome)
        assertEquals(1, fake.uploads.size)
    }

    @Test
    fun aRefusedFileEndsTheBatchWithTheServersSentence() = runBlocking {
        store.save(batch)
        fake.override(
            "POST",
            "/api/uploads",
            MockResponse.Builder().code(413).addHeader("Content-Type", "application/json")
                .body("""{"error":{"code":"payload_too_large","message":"There is not enough free space on the stash for this file."}}""").build(),
        )
        assertEquals(UploadOutcome.Failed("There is not enough free space on the stash for this file."), runner().run("batch-1") {})
        assertNull(store.load("batch-1"))
    }
}
