package com.thekozugroup.popcornstash.share

import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.ApiException
import com.thekozugroup.popcornstash.data.api.baseHttpClient
import kotlinx.coroutines.runBlocking
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import org.junit.After
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Before
import org.junit.Test

/** One file through the resumable upload API, chunk by chunk, resuming where the server is. */
class UploaderTest {
    private val server = MockWebServer()
    private val fake = FakeUploadServer(chunkSize = 5)
    private lateinit var uploader: Uploader
    private val bytes = "the quick brown fox".toByteArray()
    private val file = SharedFile(uri = "content://media/1", name = "fox.txt", size = bytes.size.toLong(), mime = "text/plain", lastModified = 1_700_000_000)

    @Before
    fun setUp() {
        fake.install(server)
        server.start()
        uploader = Uploader(ApiClient(server.url("/"), baseHttpClient("test")), retryDelays = listOf(0, 0, 0))
    }

    @After
    fun tearDown() = server.close()

    private fun puts() = fake.requests.filter { it.method == "PUT" }.map { it.url.queryParameter("offset")!!.toLong() }

    @Test
    fun aFileGoesUpInTheServersChunkSizeKeepingItsDate() = runBlocking {
        val created = mutableListOf<String>()
        val sent = mutableListOf<Long>()
        val download = uploader.upload(file, bytes.size.toLong(), BytesSource(bytes), { created += it }, { sent += it })
        assertEquals("d-u1", download.id)
        assertEquals(listOf("u1"), created)
        assertEquals(listOf(0L, 5L, 10L, 15L), puts())
        assertArrayEquals(bytes, fake.uploads.getValue("u1").body.readByteArray())
        assertEquals(bytes.size.toLong(), sent.last())
        val create = fake.createBodies.single()
        assertTrue(create.contains("\"last_modified\":1700000000"))
        assertTrue(create.contains("\"mime\":\"text/plain\""))
        assertTrue(create.contains("\"filename\":\"fox.txt\""))
    }

    @Test
    fun aKnownUploadContinuesFromWhatTheServerHas() = runBlocking {
        fake.uploads["u7"] = FakeUploadServer.Upload("u7", "fox.txt", bytes.size.toLong()).also { it.body.write(bytes, 0, 10) }
        uploader.upload(file.copy(uploadId = "u7"), bytes.size.toLong(), BytesSource(bytes), {}, {})
        // A resumed upload does not know the server's chunk size: it sends up to 16 MiB at once.
        assertEquals(listOf(10L), puts())
        assertTrue(fake.createBodies.isEmpty())
        assertArrayEquals(bytes, fake.uploads.getValue("u7").body.readByteArray())
    }

    @Test
    fun anUploadTheServerNoLongerKnowsStartsAgain() = runBlocking {
        val created = mutableListOf<String>()
        uploader.upload(file.copy(uploadId = "u-gone"), bytes.size.toLong(), BytesSource(bytes), { created += it }, {})
        assertEquals(listOf("u1"), created)
        assertEquals(listOf(0L, 5L, 10L, 15L), puts())
    }

    @Test
    fun aChunkThatFailsIsTriedAgainFromWhatTheServerHas() = runBlocking {
        // A proxy answered 503 to the first chunk: the status says nothing arrived, so it goes again from 0.
        fake.override(
            "PUT",
            "/api/uploads/u1",
            MockResponse.Builder().code(503).build(),
        )
        uploader.upload(file, bytes.size.toLong(), BytesSource(bytes), {}, {})
        assertEquals(listOf(0L, 0L, 5L, 10L, 15L), puts())
        assertArrayEquals(bytes, fake.uploads.getValue("u1").body.readByteArray())
    }

    @Test
    fun aMismatchedOffsetFollowsTheServersCount() = runBlocking {
        fake.uploads["u7"] = FakeUploadServer.Upload("u7", "fox.txt", bytes.size.toLong()).also { it.body.write(bytes, 0, 5) }
        // The status says 5, but the chunk at 5 already arrived from an earlier try in between.
        fake.override(
            "PUT",
            "/api/uploads/u7",
            MockResponse.Builder().code(409).addHeader("Content-Type", "application/json")
                .body("""{"error":{"code":"conflict","message":"x"},"received":10}""").build(),
        )
        fake.uploads.getValue("u7").body.write(bytes, 5, 5)
        uploader.upload(file.copy(uploadId = "u7"), bytes.size.toLong(), BytesSource(bytes), {}, {})
        assertEquals(listOf(10L, 10L), puts())
        assertArrayEquals(bytes, fake.uploads.getValue("u7").body.readByteArray())
    }

    @Test
    fun aRefusedFileStopsWithTheServersSentence() = runBlocking {
        fake.override(
            "POST",
            "/api/uploads",
            MockResponse.Builder().code(413).addHeader("Content-Type", "application/json")
                .body("""{"error":{"code":"payload_too_large","message":"There is not enough free space on the stash for this file."}}""").build(),
        )
        try {
            uploader.upload(file, bytes.size.toLong(), BytesSource(bytes), {}, {})
            fail("The upload should have stopped")
        } catch (e: ApiException) {
            assertEquals("There is not enough free space on the stash for this file.", e.message)
        }
    }

    @Test
    fun nothingIsSentOnceTheUploadIsNoLongerWanted() = runBlocking {
        var wanted = true
        try {
            uploader.upload(file, bytes.size.toLong(), BytesSource(bytes), {}, { sent -> if (sent >= 5) wanted = false }, { wanted })
            fail("The upload should have stopped")
        } catch (_: UploadStoppedException) {
        }
        assertEquals(listOf(0L), puts())
        assertTrue(fake.requests.none { it.url.encodedPath.endsWith("/complete") })
    }

    @Test
    fun aFileThatShrankStopsTheUpload() = runBlocking {
        try {
            uploader.upload(file, bytes.size.toLong() + 10, BytesSource(bytes), {}, {})
            fail("The upload should have stopped")
        } catch (_: SourceUnreadableException) {
        }
    }
}
