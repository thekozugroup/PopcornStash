package com.thekozugroup.popcornstash.testing

import com.thekozugroup.popcornstash.data.api.DownloadFolder
import com.thekozugroup.popcornstash.data.api.VideoCollection
import com.thekozugroup.popcornstash.data.api.VideoLibraryFeed

/** Synthetic feed content uses the same painted pictures as the existing screenshot goldens. */
object VideoFeedFixtures {
    val collections = (1..14).map { number ->
        VideoCollection(
            DownloadFolder("download:sample-$number", "Collection $number", 6, 900_000_000, SampleMedia.NOW - number),
            SampleMedia.recentMovies,
        )
    }
    val movies = VideoLibraryFeed(
        continueWatching = listOf(SampleMedia.paperBoats),
        suggested = SampleMedia.recentMovies.filter { it.progress == null }.take(3),
        recent = SampleMedia.recentMovies,
        collections = collections.take(12),
        nextCursor = "12",
    )
    val home = SampleMedia.home.copy(suggested = listOf(SampleMedia.northbound, SampleMedia.beachDay))
}
