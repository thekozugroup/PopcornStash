package com.thekozugroup.popcornstash.data.api

import kotlinx.coroutines.test.runTest
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.OkHttpClient
import okhttp3.Protocol
import okhttp3.Response
import okhttp3.ResponseBody.Companion.toResponseBody
import okio.Buffer
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class TorrentFilesTest {
    @Test
    fun manifestReadsNestedPathsLargeSizesAndAllKindsWithoutChangingSelections() {
        val manifest = StashJson.decodeFromString(TorrentFiles.serializer(), """{"ready":true,"files":[
            {"index":7,"path":"Season 1/Episode.mkv","size":8000000000,"kind":"video","selected":true},
            {"index":9,"path":"Season 1/en.srt","size":25000,"kind":"subtitle","selected":false},
            {"index":10,"path":"cover.jpg","size":50000,"kind":"image","selected":true},
            {"index":14,"path":"readme.txt","size":500,"kind":"other","selected":true}
        ]}""")
        assertEquals("Season 1/Episode.mkv", manifest.files.first().path)
        assertEquals(8_000_000_000L, manifest.files.first().size)
        assertFalse(manifest.files[1].selected)
        assertEquals(listOf(TorrentFileKind.Video, TorrentFileKind.Subtitle, TorrentFileKind.Image, TorrentFileKind.Other), manifest.files.map { it.kind })
        assertEquals(listOf(7, 10, 14), manifest.files.filter { it.selected }.map { it.index })
    }

    @Test
    fun pendingMetadataAndSelectionMatchTheWireContract() {
        assertFalse(StashJson.decodeFromString(TorrentFiles.serializer(), """{"ready":false,"files":[]}""").ready)
        assertEquals("""{"indices":[7,14]}""", StashJson.encodeToString(TorrentFileSelection.serializer(), TorrentFileSelection(listOf(7, 14))))
    }

    @Test
    fun verifiedBytesStayUnknownOnOlderServersAndSelectionDoesNotImplyCompletion() {
        val manifest = StashJson.decodeFromString(TorrentFiles.serializer(), """{"ready":true,"files":[
            {"index":0,"path":"a","size":100,"kind":"other","selected":true},
            {"index":1,"path":"b","size":5000000000,"kind":"other","selected":true,"verified_bytes":4000000000,"complete":false},
            {"index":2,"path":"c","size":0,"kind":"other","selected":false,"verified_bytes":0,"complete":true},
            {"index":3,"path":"d","size":0,"kind":"other","selected":true,"verified_bytes":null,"complete":false}
        ]}""")
        assertNull(manifest.files[0].verifiedBytes)
        assertEquals(4_000_000_000L, manifest.files[1].verifiedBytes)
        assertFalse(manifest.files[1].isVerifiedComplete)
        assertTrue(manifest.files[2].isVerifiedComplete)
        assertFalse(manifest.files[3].isVerifiedComplete)
    }

    @Test
    fun skipAndFinishUseScopedPostsAndReadTheReturnedDownload() = runTest {
        val requests = mutableListOf<String>()
        val finishBodies = mutableListOf<String>()
        val http = OkHttpClient.Builder().addInterceptor { chain ->
            val request = chain.request()
            requests += "${request.method} ${request.url.encodedPath}"
            val finish = request.url.encodedPath.endsWith("/finish-completed")
            if (finish) finishBodies += Buffer().apply { request.body!!.writeTo(this) }.readUtf8()
            Response.Builder().request(request).protocol(Protocol.HTTP_1_1)
                .code(if (finish) 200 else 204).message("OK")
                .body((if (finish) """{"id":"test","name":"test","state":"completed","kind":"torrent"}""" else "").toResponseBody())
                .build()
        }.build()
        val api = ApiClient("https://example.invalid/".toHttpUrl(), http)
        api.skipTorrentFile("one/two", 31)
        val reviewed = FinishCompletedRequest(expectedSelected = listOf(7, 31), expectedComplete = listOf(7))
        assertEquals(DownloadState.Completed, api.finishCompletedTorrentFiles("one/two", reviewed).state)
        api.finishCompletedTorrentFiles("one/two")
        val finishPath = "POST /api/downloads/one%2Ftwo/finish-completed"
        assertEquals(listOf("POST /api/downloads/one%2Ftwo/files/31/skip", finishPath, finishPath), requests)
        assertEquals(listOf("""{"expected_selected":[7,31],"expected_complete":[7]}""", "{}"), finishBodies)
    }
}
