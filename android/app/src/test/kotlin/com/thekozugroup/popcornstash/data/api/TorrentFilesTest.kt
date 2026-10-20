package com.thekozugroup.popcornstash.data.api

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
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
}
