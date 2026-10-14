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

/** One tile on a library's Home shelf: an item, or a show. */
sealed interface ShelfTile {
    val key: String

    data class Single(val item: Item) : ShelfTile {
        override val key get() = item.id
    }

    /**
     * A show of a series library: its picture is [posterId]'s (the show's poster), else its
     * [latest] episode's; [episodeCount] episodes in the library, [newCount] of them new.
     */
    data class Show(
        val name: String,
        val posterId: String?,
        val episodeCount: Long,
        val newCount: Long,
        val latest: Item,
    ) : ShelfTile {
        override val key get() = "show:$name"
    }
}

/**
 * The tiles of a library's shelf: its newest items in its own shape. A series library has a
 * tile per show from `shows` (docs/architecture.md section 8, HomeShow), the show with the
 * newest episode first, so a library of three shows shows three tiles whatever was added
 * last. A server older than `shows` gets the shows of the newest episodes, as before.
 */
fun shelfTiles(entry: HomeLibrary): List<ShelfTile> {
    if (entry.library.layout != Layout.Series) return entry.recent.map { ShelfTile.Single(it) }
    // Episodes with no show name have no show page; they keep a tile of their own.
    val loose = entry.recent.filter { it.showName.isNullOrBlank() }.map { ShelfTile.Single(it) }
    val shows = entry.shows ?: return showsOfRecent(entry) + loose
    return shows.map { ShelfTile.Show(it.showName, it.posterItemId, it.itemCount, it.newCount, it.latest) } + loose
}

/** An older server's series shelf: the shows among the newest episodes, every one of them new. */
private fun showsOfRecent(entry: HomeLibrary): List<ShelfTile.Show> {
    val shows = LinkedHashMap<String, MutableList<Item>>()
    for (item in entry.recent) {
        val name = item.showName?.takeIf { it.isNotBlank() } ?: continue
        shows.getOrPut(name) { mutableListOf() } += item
    }
    return shows.map { (name, episodes) ->
        ShelfTile.Show(name, entry.showPosters[name], episodes.size.toLong(), episodes.size.toLong(), episodes.first())
    }
}

/** The line under a show tile: "New: S01E03", "3 new episodes", or "12 episodes" when none is new. */
fun MediaFormat.showTileLine(tile: ShelfTile.Show): String = when {
    tile.newCount == 1L -> "New: ${MediaFormat.episodeCode(tile.latest.season, tile.latest.episode)}"
    tile.newCount > 1L -> count(tile.newCount, "new episode")
    else -> count(tile.episodeCount, "episode")
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
