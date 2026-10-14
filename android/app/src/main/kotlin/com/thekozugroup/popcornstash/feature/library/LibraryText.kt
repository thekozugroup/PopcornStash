package com.thekozugroup.popcornstash.feature.library

import com.thekozugroup.popcornstash.data.api.Category
import com.thekozugroup.popcornstash.data.api.Layout
import com.thekozugroup.popcornstash.data.api.Library
import com.thekozugroup.popcornstash.ui.components.TileShape

/** The shape a library's tiles take (android/DESIGN.md, Tiles and shelves): its server-given layout. */
fun Layout.tileShape(): TileShape = when (this) {
    Layout.Frames -> TileShape.Frame
    Layout.Timeline -> TileShape.Square
    else -> TileShape.Poster
}

/** The count beside a library's name, as on the web: "214 videos", "12 shows", "3,214 photos". */
fun MediaFormat.libraryCountLine(library: Library): String = when (library.layout) {
    Layout.Series -> count(library.showCount ?: 0, "show")
    Layout.Timeline -> count(library.itemCount, "photo")
    else -> count(library.itemCount, "video")
}

/** The owner's words for each kind of new file (web/src/lib/libraries.ts). */
private fun Category.claimWords(): String? = when (this) {
    Category.Movie -> "movies"
    Category.Episode -> "TV episodes"
    Category.HomeVideo -> "camera videos"
    Category.Photo -> "photos"
    else -> null
}

/** "a, b and c". */
fun joinWords(words: List<String>): String = when (words.size) {
    0 -> ""
    1 -> words.first()
    else -> words.dropLast(1).joinToString(", ") + " and " + words.last()
}

/** How an empty library fills, for its empty state: the web's sentence. */
fun emptyLibraryLine(library: Library): String {
    val words = library.claims.mapNotNull { it.claimWords() }
    if (words.isEmpty()) return "Files you accept from Downloads into ${library.name} show up here."
    return "New ${joinWords(words)} land here on their own."
}
