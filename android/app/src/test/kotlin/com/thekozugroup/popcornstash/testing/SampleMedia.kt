package com.thekozugroup.popcornstash.testing

import com.thekozugroup.popcornstash.data.api.AudioStream
import com.thekozugroup.popcornstash.data.api.Category
import com.thekozugroup.popcornstash.data.api.ContentType
import com.thekozugroup.popcornstash.data.api.Home
import com.thekozugroup.popcornstash.data.api.HomeLibrary
import com.thekozugroup.popcornstash.data.api.HomeShow
import com.thekozugroup.popcornstash.data.api.Item
import com.thekozugroup.popcornstash.data.api.ItemDetail
import com.thekozugroup.popcornstash.data.api.ItemStatus
import com.thekozugroup.popcornstash.data.api.Layout
import com.thekozugroup.popcornstash.data.api.Library
import com.thekozugroup.popcornstash.data.api.MediaKind
import com.thekozugroup.popcornstash.data.api.MediaSummary
import com.thekozugroup.popcornstash.data.api.PhotoMonth
import com.thekozugroup.popcornstash.data.api.Place
import com.thekozugroup.popcornstash.data.api.Progress
import com.thekozugroup.popcornstash.data.api.Season
import com.thekozugroup.popcornstash.data.api.ShowDetail
import com.thekozugroup.popcornstash.data.api.ShowSummary
import com.thekozugroup.popcornstash.data.api.Structure
import com.thekozugroup.popcornstash.data.api.SubtitleStream
import com.thekozugroup.popcornstash.data.api.VideoStream
import java.time.LocalDateTime
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter

/**
 * A family's stash as the screenshots and UI tests show it: invented titles, people and
 * dates (base time: 2026-09-14), shaped like the server's answers (docs/architecture.md
 * section 8).
 */
object SampleMedia {
    const val NOW = 1_789_400_000L
    private const val DAY = 86_400L

    val movies = Library(
        id = "lib-movies", name = "Movies", icon = "film", contentType = ContentType.Video, structure = Structure.OneOffs,
        claims = listOf(Category.Movie), layout = Layout.Posters, position = 0, itemCount = 48,
    )
    val shows = Library(
        id = "lib-shows", name = "Shows", icon = "tv", contentType = ContentType.Video, structure = Structure.Episodes,
        claims = listOf(Category.Episode), layout = Layout.Series, position = 1, itemCount = 86, showCount = 6,
    )
    val homeVideos = Library(
        id = "lib-home-videos", name = "Home Videos", icon = "clapperboard", contentType = ContentType.Video,
        structure = Structure.OneOffs, claims = listOf(Category.HomeVideo), layout = Layout.Frames, position = 2, itemCount = 212,
    )
    val photos = Library(
        id = "lib-photos", name = "Photos", icon = "image", contentType = ContentType.Photos,
        claims = listOf(Category.Photo), layout = Layout.Timeline, position = 3, itemCount = 3214,
    )
    val kids = Library(
        id = "lib-kids", name = "Kids", icon = "baby", contentType = ContentType.Video, structure = Structure.Episodes,
        claims = emptyList(), layout = Layout.Series, position = 4, itemCount = 0, showCount = 0,
    )

    private fun thumb(id: String) = "/api/items/$id/thumb?size=sm&v=7"

    fun movie(id: String, title: String, year: Int, minutes: Int, watched: Double? = null, added: Long = NOW) = Item(
        id = id, kind = MediaKind.Video, category = Category.Movie, status = ItemStatus.Ready, title = title, year = year,
        relPath = "Movies/$title ($year)/$title ($year).mkv", size = 2_400_000_000, durationS = minutes * 60.0,
        width = 1920, height = 1040, thumb = thumb(id), addedAt = added,
        progress = watched?.let { Progress(minutes * 60.0 * it, minutes * 60.0) },
        libraryId = movies.id, place = Place.Library,
    )

    fun episode(
        id: String,
        show: String,
        season: Int,
        episode: Int,
        name: String?,
        minutes: Int = 24,
        watched: Double? = null,
        finished: Boolean = false,
        added: Long = NOW,
    ) = Item(
        id = id, kind = MediaKind.Video, category = Category.Episode, status = ItemStatus.Ready,
        title = "$show S%02dE%02d".format(season, episode), showName = show, season = season, episode = episode,
        episodeTitle = name, relPath = "Shows/$show/Season %02d/$show - S%02dE%02d.mkv".format(season, season, episode),
        size = 400_000_000, durationS = minutes * 60.0, width = 1920, height = 1080, thumb = thumb(id), addedAt = added,
        progress = when {
            finished -> Progress(minutes * 60.0, minutes * 60.0, finished = true)
            watched != null -> Progress(minutes * 60.0 * watched, minutes * 60.0)
            else -> null
        },
        libraryId = shows.id, place = Place.Library,
    )

    /** The family's cameras are on New York time (EDT in September): minutes east of UTC. */
    const val HOME_OFFSET = -240

    /** The camera's clock (`taken_local`) for a moment on a camera whose clock is [offsetMinutes] east of UTC. */
    fun clock(at: Long, offsetMinutes: Int = HOME_OFFSET): String =
        LocalDateTime.ofEpochSecond(at, 0, ZoneOffset.ofTotalSeconds(offsetMinutes * 60)).format(CLOCK)

    private val CLOCK = DateTimeFormatter.ofPattern("yyyy-MM-dd'T'HH:mm:ss")

    fun homeVideo(id: String, title: String, daysAgo: Int, seconds: Int) = Item(
        id = id, kind = MediaKind.Video, category = Category.HomeVideo, status = ItemStatus.Ready, title = title,
        relPath = "Home Videos/2026/08/$title.mp4", size = 180_000_000, durationS = seconds.toDouble(), width = 1920, height = 1080,
        takenAt = NOW - daysAgo * DAY, takenLocal = clock(NOW - daysAgo * DAY), takenOffset = HOME_OFFSET,
        camera = "Pixel 8", thumb = thumb(id), addedAt = NOW - daysAgo * DAY,
        libraryId = homeVideos.id, place = Place.Library,
    )

    fun photo(id: String, daysAgo: Int, hour: Int = 10, video: Boolean = false) = Item(
        id = id, kind = if (video) MediaKind.Video else MediaKind.Photo, category = Category.Photo, status = ItemStatus.Ready,
        title = "PXL_${id.uppercase()}", relPath = "Photos/2026/08/PXL_${id.uppercase()}.${if (video) "mp4" else "jpg"}",
        size = 3_400_000, durationS = if (video) 14.0 else null, width = 4032, height = 3024,
        takenAt = NOW - daysAgo * DAY + hour * 3600L, takenLocal = clock(NOW - daysAgo * DAY + hour * 3600L), takenOffset = HOME_OFFSET,
        camera = "Pixel 8", thumb = thumb(id), addedAt = NOW - daysAgo * DAY,
        libraryId = photos.id, place = Place.Library,
    )

    /**
     * A photo from a trip to Lisbon: its camera read 18:05 on 12 September (UTC+01:00), which
     * was 13:05 in New York. Everyone sees 18:05, whatever zone their phone is in.
     */
    val lisbonSunset = photo("s2", daysAgo = 2).copy(
        takenAt = LocalDateTime.of(2026, 9, 12, 18, 5).toEpochSecond(ZoneOffset.ofHours(1)),
        takenLocal = "2026-09-12T18:05:00",
        takenOffset = 60,
        camera = "Pixel 8 Pro",
    )

    val paperBoats = movie("m-paper", "Paper Boats", 2011, 104, watched = 0.38)
    val harborLights = movie("m-harbor", "Harbor Lights", 2019, 118)
    val longField = movie("m-field", "The Long Field", 2016, 97)
    val northbound = movie("m-north", "Northbound", 2022, 126)
    val saltwater = movie("m-salt", "Saltwater Kids", 2014, 92)
    val orchard = movie("m-orchard", "A Quiet Orchard", 2018, 111)
    val recentMovies = listOf(northbound, harborLights, paperBoats, orchard, longField, saltwater)

    val lantern = listOf(
        episode("e-l101", "The Lantern Keepers", 1, 1, "Pilot", finished = true, added = NOW - 9 * DAY),
        episode("e-l102", "The Lantern Keepers", 1, 2, "Fog over the pier", watched = 0.55, added = NOW - 9 * DAY),
        episode("e-l103", "The Lantern Keepers", 1, 3, "The lantern keepers", added = NOW - 9 * DAY),
        episode("e-l104", "The Lantern Keepers", 1, 4, "Night shift", added = NOW - 9 * DAY),
        episode("e-l105", "The Lantern Keepers", 1, 5, null, added = NOW - 9 * DAY),
    )
    val lanternSeason2 = listOf(
        episode("e-l201", "The Lantern Keepers", 2, 1, "A new keeper", added = NOW - DAY),
        episode("e-l202", "The Lantern Keepers", 2, 2, "Storm warning", added = NOW - DAY),
    )
    val tidepool = listOf(
        episode("e-t104", "Tidepool Detectives", 1, 4, "The missing crab", minutes = 11, added = NOW - 2 * DAY),
        episode("e-t105", "Tidepool Detectives", 1, 5, "Sand castle mystery", minutes = 11, added = NOW - 2 * DAY),
    )
    val garden = episode("e-g301", "Garden Hour", 3, 1, "Autumn bulbs", minutes = 28, added = NOW - 3 * DAY)

    val beachDay = homeVideo("v-beach", "Beach day", 3, 214)
    val bikeRide = homeVideo("v-bike", "First bike ride", 12, 96)
    val birthday = homeVideo("v-birthday", "Grandma's 80th", 20, 1260)
    val lakeMorning = homeVideo("v-lake", "Lake house, morning", 31, 44)

    val recentPhotos = (1..10).map { photo("p$it", daysAgo = it / 3, hour = 8 + it, video = it == 4) }

    /**
     * The Shows library's Home shelf: all six of its shows (its heading says "6 shows"), the
     * one with the newest episode first, though the newest episodes belong to three of them.
     */
    val homeShows = listOf(
        HomeShow("The Lantern Keepers", "e-l101", itemCount = 7, newCount = 2, latest = lanternSeason2[1], lastAddedAt = NOW - DAY),
        HomeShow("Tidepool Detectives", "e-t104", itemCount = 12, newCount = 2, latest = tidepool[1], lastAddedAt = NOW - 2 * DAY),
        HomeShow("Garden Hour", "e-g301", itemCount = 24, newCount = 1, latest = garden, lastAddedAt = NOW - 3 * DAY),
        HomeShow(
            "Night Bakery", "e-n101", itemCount = 8, newCount = 0,
            latest = episode("e-n108", "Night Bakery", 1, 8, "Rye at midnight", added = NOW - 40 * DAY), lastAddedAt = NOW - 40 * DAY,
        ),
        HomeShow(
            "Small Planet", "e-s101", itemCount = 6, newCount = 0,
            latest = episode("e-s106", "Small Planet", 1, 6, "The tide pool", added = NOW - 75 * DAY), lastAddedAt = NOW - 75 * DAY,
        ),
        HomeShow(
            "Puddle Club", "e-p101", itemCount = 29, newCount = 0,
            latest = episode("e-p213", "Puddle Club", 2, 13, "Boots on", added = NOW - 120 * DAY), lastAddedAt = NOW - 120 * DAY,
        ),
    )

    val continueWatching = listOf(lantern[1], paperBoats, episode("e-t103", "Tidepool Detectives", 1, 3, "Low tide", minutes = 11, watched = 0.8))

    val home = Home(
        continueWatching = continueWatching,
        libraries = listOf(
            HomeLibrary(movies, recentMovies),
            HomeLibrary(
                shows,
                lanternSeason2 + tidepool + garden,
                showPosters = mapOf("The Lantern Keepers" to "e-l101", "Tidepool Detectives" to "e-t104", "Garden Hour" to "e-g301"),
                shows = homeShows,
            ),
            HomeLibrary(homeVideos, listOf(beachDay, bikeRide, birthday, lakeMorning)),
            HomeLibrary(photos, recentPhotos),
            HomeLibrary(kids, emptyList()),
        ),
        otherFilesCount = 12,
        inboxCount = 3,
    )

    val showSummaries = listOf(
        ShowSummary("Garden Hour", itemCount = 24, seasonCount = 3, posterItemId = "e-g301"),
        ShowSummary("The Lantern Keepers", itemCount = 7, seasonCount = 2, posterItemId = "e-l101"),
        ShowSummary("Tidepool Detectives", itemCount = 12, seasonCount = 1, posterItemId = "e-t104"),
        ShowSummary("Night Bakery", itemCount = 8, seasonCount = 1, posterItemId = "e-n101"),
        ShowSummary("Small Planet", itemCount = 6, seasonCount = 1, posterItemId = "e-s101"),
    )

    val lanternShow = ShowDetail(
        showName = "The Lantern Keepers",
        posterItemId = "e-l101",
        seasons = listOf(Season(1, lantern), Season(2, lanternSeason2)),
    )

    val timeline = listOf(
        PhotoMonth("2026-09", 7, (1..7).map { if (it == 2) lisbonSunset else photo("s$it", daysAgo = it, video = it == 3) }),
        PhotoMonth("2026-08", 9, (8..16).map { photo("a$it", daysAgo = it + 10) }),
    )

    val paperBoatsDetail = ItemDetail(
        id = paperBoats.id, kind = MediaKind.Video, category = Category.Movie, status = ItemStatus.Ready,
        title = paperBoats.title, year = 2011, relPath = paperBoats.relPath, size = 2_400_000_000, durationS = 104 * 60.0,
        width = 1920, height = 1040, thumb = paperBoats.thumb, favorite = false, addedAt = NOW,
        progress = paperBoats.progress, libraryId = movies.id, place = Place.Library,
        media = MediaSummary(
            container = "matroska,webm",
            durationS = 104 * 60.0,
            video = VideoStream(0, "h264", width = 1920, height = 1040),
            audio = listOf(AudioStream(1, "eac3", channels = 6, language = "eng"), AudioStream(2, "aac", channels = 2, language = "por")),
            subtitles = listOf(SubtitleStream(3, "subrip", language = "eng"), SubtitleStream(4, "subrip", language = "spa")),
        ),
    )

    val episodeDetail = ItemDetail(
        id = "e-l102", kind = MediaKind.Video, category = Category.Episode, status = ItemStatus.Ready,
        title = "The Lantern Keepers S01E02", showName = "The Lantern Keepers", season = 1, episode = 2,
        episodeTitle = "Fog over the pier", relPath = lantern[1].relPath, size = 400_000_000, durationS = 24 * 60.0,
        width = 1920, height = 1080, thumb = lantern[1].thumb, favorite = true, addedAt = NOW,
        progress = lantern[1].progress, libraryId = shows.id, place = Place.Library,
        media = MediaSummary(video = VideoStream(0, "hevc", width = 3840, height = 2160, hdr = "hdr10"), audio = listOf(AudioStream(1, "aac", channels = 2, language = "eng"))),
        nextEpisodeId = "e-l103",
    )

    val otherFiles = listOf(
        Item(id = "o-1", kind = MediaKind.Other, category = Category.Other, status = ItemStatus.Ready, title = "School trip permission",
            relPath = "Other/School trip permission.pdf", size = 192_400, addedAt = NOW, place = Place.OtherFiles),
        Item(id = "o-2", kind = MediaKind.Audio, category = Category.Other, status = ItemStatus.Ready, title = "Lullabies for the car",
            relPath = "Other/Lullabies for the car.mp3", size = 8_100_000, durationS = 214.0, addedAt = NOW, place = Place.OtherFiles),
        Item(id = "o-3", kind = MediaKind.Other, category = Category.Other, status = ItemStatus.Ready, title = "Holiday photos 2019",
            relPath = "Other/Holiday photos 2019.zip", size = 1_240_000_000, addedAt = NOW, place = Place.OtherFiles),
        Item(id = "o-4", kind = MediaKind.Other, category = Category.Other, status = ItemStatus.Ready, title = "Recipes",
            relPath = "Other/Recipes.docx", size = 592, addedAt = NOW, place = Place.OtherFiles),
    )
}
