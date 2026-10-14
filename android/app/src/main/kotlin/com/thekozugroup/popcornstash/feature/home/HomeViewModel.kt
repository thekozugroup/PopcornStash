package com.thekozugroup.popcornstash.feature.home

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.thekozugroup.popcornstash.data.api.Category
import com.thekozugroup.popcornstash.data.api.Home
import com.thekozugroup.popcornstash.data.api.HomeLibrary
import com.thekozugroup.popcornstash.data.api.Item
import com.thekozugroup.popcornstash.data.api.Layout
import com.thekozugroup.popcornstash.data.repo.Loadable
import com.thekozugroup.popcornstash.data.repo.StashData
import com.thekozugroup.popcornstash.feature.library.MediaFormat
import com.thekozugroup.popcornstash.feature.library.displayTitle
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch

/**
 * Home (android/DESIGN.md, Home): `GET /api/home` through the session's data, which live
 * `library` and `libraries` events keep current. Home loads again when the events stream
 * comes back after a gap, and when the person comes back from the player.
 */
class HomeViewModel(
    private val data: StashData,
    reconnects: Flow<Unit>,
    connectionLost: Flow<Boolean>,
) : ViewModel() {
    val home: StateFlow<Loadable<Home>> = data.home

    /** True while the events stream has been away for a while: Home says it is trying again. */
    val connectionLost: StateFlow<Boolean> = connectionLost.stateIn(viewModelScope, SharingStarted.Eagerly, false)

    init {
        if (data.home.value is Loadable.Idle) data.refreshHome()
        viewModelScope.launch { reconnects.collect { data.refreshHome() } }
    }

    fun refresh() {
        data.refreshHome()
    }
}

/** One tile on a library's Home shelf: an item, or a show standing for its new episodes. */
sealed interface ShelfTile {
    val key: String

    data class Single(val item: Item) : ShelfTile {
        override val key get() = item.id
    }

    /** A show with [episodes] among the newest items; its picture is [posterId]'s (the show's poster). */
    data class Show(val name: String, val episodes: List<Item>, val posterId: String?) : ShelfTile {
        override val key get() = "show:$name"
    }
}

/**
 * The tiles of a library's shelf: its newest items in its own shape, where a series
 * library's episodes are grouped by show, newest show first, each pictured by the show's
 * poster (`show_posters`), as on the web.
 */
fun shelfTiles(entry: HomeLibrary): List<ShelfTile> {
    if (entry.library.layout != Layout.Series) return entry.recent.map { ShelfTile.Single(it) }
    val shows = LinkedHashMap<String, MutableList<Item>>()
    for (item in entry.recent) {
        val name = item.showName?.takeIf { it.isNotBlank() }
        if (name == null) continue
        shows.getOrPut(name) { mutableListOf() } += item
    }
    val loose = entry.recent.filter { it.showName.isNullOrBlank() }.map { ShelfTile.Single(it) }
    return shows.map { (name, episodes) -> ShelfTile.Show(name, episodes, entry.showPosters[name]) } + loose
}

/** The line under a show tile: "New: S01E03", or "3 new episodes". */
fun MediaFormat.showTileLine(tile: ShelfTile.Show): String {
    val only = tile.episodes.singleOrNull()
    return if (only != null) {
        "New: ${MediaFormat.episodeCode(only.season, only.episode)}"
    } else {
        count(tile.episodes.size.toLong(), "new episode")
    }
}

/** The title and line of a continue-watching card: the show and episode code for an episode, as on the web. */
fun MediaFormat.resumeTitle(item: Item): String =
    if (item.category == Category.Episode) {
        item.showName?.takeIf { it.isNotBlank() } ?: displayTitle(item)
    } else {
        displayTitle(item)
    }

fun MediaFormat.resumeLine(item: Item): String {
    val code = if (item.category == Category.Episode) {
        MediaFormat.episodeCode(item.season, item.episode)
    } else {
        null
    }
    return join(code, timeLeft(item))
}

/** What a screen reader says for a continue-watching card: "Resume Bluey S01E02, 12 minutes left". */
fun MediaFormat.resumeDescription(item: Item): String {
    val code = if (item.category == Category.Episode) {
        MediaFormat.episodeCode(item.season, item.episode)
    } else {
        ""
    }
    val name = listOf(resumeTitle(item), code).filter { it.isNotBlank() }.joinToString(" ")
    val left = secondsLeft(item)?.let { ", ${spokenDuration(it)} left" }.orEmpty()
    return "Resume $name$left"
}
