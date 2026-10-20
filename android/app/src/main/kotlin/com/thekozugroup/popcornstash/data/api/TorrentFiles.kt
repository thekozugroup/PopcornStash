package com.thekozugroup.popcornstash.data.api

import kotlinx.serialization.Serializable

/** A torrent's relative file manifest, including the server's saved choices. */
@Serializable
data class TorrentFiles(val ready: Boolean, val files: List<TorrentFile> = emptyList(), val rules: TorrentFileRules? = null, val reviewRequired: Boolean = false)

@Serializable
data class TorrentFile(
    val index: Int,
    val path: String,
    val size: Long,
    val kind: TorrentFileKind,
    val selected: Boolean,
)

/** `PUT /api/downloads/:id/files`: the complete set of wanted file indices. */
@Serializable
data class TorrentFileSelection(val indices: List<Int>)
