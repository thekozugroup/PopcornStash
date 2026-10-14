package com.thekozugroup.popcornstash.feature.downloads

import com.thekozugroup.popcornstash.data.api.Category
import com.thekozugroup.popcornstash.data.api.ContentType
import com.thekozugroup.popcornstash.data.api.Destination
import com.thekozugroup.popcornstash.data.api.Download
import com.thekozugroup.popcornstash.data.api.DownloadKind
import com.thekozugroup.popcornstash.data.api.DownloadState
import com.thekozugroup.popcornstash.data.api.ItemDetail
import com.thekozugroup.popcornstash.data.api.ItemStatus
import com.thekozugroup.popcornstash.data.api.Layout
import com.thekozugroup.popcornstash.data.api.Library
import com.thekozugroup.popcornstash.data.api.MediaKind
import com.thekozugroup.popcornstash.data.api.Place
import com.thekozugroup.popcornstash.data.api.Structure
import com.thekozugroup.popcornstash.data.api.Suggestion
import com.thekozugroup.popcornstash.data.api.VpnState
import com.thekozugroup.popcornstash.data.api.VpnStatus
import com.thekozugroup.popcornstash.data.api.Waiting
import com.thekozugroup.popcornstash.share.UploadBatchProgress

/** The family's downloads as the screenshots and UI tests show them (invented names). */
object DownloadsSamples {
    /** 14 Oct 2026, 18:00 UTC. */
    const val NOW = 1_791_993_600L

    val movies = Library(
        id = "lib-movies", name = "Movies", icon = "film", contentType = ContentType.Video, structure = Structure.OneOffs,
        claims = listOf(Category.Movie), layout = Layout.Posters, position = 0, folder = "Movies",
    )
    val shows = Library(
        id = "lib-shows", name = "Shows", icon = "tv", contentType = ContentType.Video, structure = Structure.Episodes,
        claims = listOf(Category.Episode), layout = Layout.Series, position = 1, folder = "Shows",
    )
    val kids = Library(
        id = "lib-kids", name = "Kids", icon = "baby", contentType = ContentType.Video, structure = Structure.Episodes,
        claims = emptyList(), layout = Layout.Series, position = 2, folder = "Kids",
    )
    val homeVideos = Library(
        id = "lib-home-videos", name = "Home Videos", icon = "clapperboard", contentType = ContentType.Video,
        structure = Structure.OneOffs, claims = listOf(Category.HomeVideo), layout = Layout.Frames, position = 3, folder = "Home Videos",
    )
    val photos = Library(
        id = "lib-photos", name = "Photos", icon = "image", contentType = ContentType.Photos, claims = listOf(Category.Photo),
        layout = Layout.Timeline, position = 4, folder = "Photos",
    )
    val libraries = listOf(movies, shows, kids, homeVideos, photos)

    val vpn = VpnStatus(
        configured = true, reachable = true, state = VpnState.Connected, provider = "nordvpn", protocol = "wireguard",
        label = "NordVPN", server = "nl812.nordvpn.com", country = "Netherlands", publicIp = "203.0.113.5", protected = true,
        killSwitch = true,
    )

    val downloading = Download(
        id = "d-lantern", kind = DownloadKind.Torrent, name = "The.Lantern.Keepers.S02E04.1080p.WEB.h264-GRP",
        source = "magnet:?xt=urn:btih:abc", state = DownloadState.Downloading, progress = 0.64, totalBytes = 1_870_000_000,
        doneBytes = 1_196_800_000, downBps = 2_140_000, peers = 14, etaS = 712, createdAt = NOW - 900,
    )
    val paused = Download(
        id = "d-trailer", kind = DownloadKind.Http, name = "harbor-lights-trailer.mp4", source = "https://media.example.org/harbor-lights-trailer.mp4",
        state = DownloadState.Paused, progress = 0.31, totalBytes = 412_000_000, doneBytes = 127_720_000, createdAt = NOW - 1_800,
    )
    val queued = Download(
        id = "d-puddle", kind = DownloadKind.Torrent, name = "Puddle.Club.S01.Complete.720p", source = "magnet:?xt=urn:btih:def",
        state = DownloadState.Queued, error = "Waiting for a free slot (5 at once).", createdAt = NOW - 2_400,
    )

    val upload = UploadBatchProgress(
        id = "batch-1",
        names = listOf("PXL_20261012_181502.mp4", "PXL_20261012_181733.jpg", "PXL_20261012_182010.jpg"),
        doneFiles = 1,
        sentBytes = 61_000_000,
        totalBytes = 146_000_000,
        waiting = false,
    )

    private fun suggestion(category: Category, title: String, waiting: Waiting?, library: Library?, reason: String = "", place: Place? = library?.let { Place.Library }) =
        Suggestion(
            category = category, title = title, confidence = if (waiting == Waiting.LowConfidence) "low" else "high", reason = reason,
            place = place, libraryId = library?.id, waiting = waiting,
        )

    val lowConfidence = ItemDetail(
        id = "i-clip", kind = MediaKind.Video, category = Category.Other, status = ItemStatus.NeedsReview, title = "clip final v2",
        relPath = "Downloads/Completed/clip_final_v2.mkv", size = 734_000_000, durationS = 5_400.0, place = Place.Incoming,
        thumb = "/api/items/i-clip/thumb?v=1",
        suggestion = suggestion(Category.Movie, "clip final v2", Waiting.LowConfidence, movies, "Not sure this is a movie."),
    )
    val episode = ItemDetail(
        id = "i-puddle", kind = MediaKind.Video, category = Category.Episode, status = ItemStatus.NeedsReview,
        title = "Puddle Club S01E01", showName = "Puddle Club", season = 1, episode = 1,
        relPath = "Downloads/Completed/Puddle.Club.S01E01.720p.mkv", size = 287_000_000, durationS = 1_320.0, place = Place.Incoming,
        thumb = "/api/items/i-puddle/thumb?v=1",
        suggestion = Suggestion(
            category = Category.Episode, title = "Puddle Club S01E01", showName = "Puddle Club", season = 1, episode = 1,
            place = Place.Library, libraryId = shows.id, waiting = Waiting.Confirm,
        ),
    )
    val photo = ItemDetail(
        id = "i-photo", kind = MediaKind.Photo, category = Category.Photo, status = ItemStatus.NeedsReview, title = "IMG_4821",
        relPath = "Inbox/IMG_4821.HEIC", size = 2_840_000, takenAt = 1_790_870_400, place = Place.Incoming,
        thumb = "/api/items/i-photo/thumb?v=1",
        suggestion = suggestion(Category.Photo, "IMG_4821", Waiting.FolderUnavailable, photos),
    )
    val document = ItemDetail(
        id = "i-doc", kind = MediaKind.Other, category = Category.Other, status = ItemStatus.NeedsReview, title = "School trip permission",
        relPath = "Inbox/School trip permission.pdf", size = 592_000, place = Place.Incoming,
        suggestion = suggestion(Category.Other, "School trip permission", Waiting.Confirm, null, place = Place.OtherFiles),
    )
    val ignoredOne = ItemDetail(
        id = "i-sample", kind = MediaKind.Video, category = Category.Other, status = ItemStatus.NeedsReview, title = "sample",
        relPath = "Downloads/Completed/Harbor.Lights.2019/sample.mkv", size = 48_000_000, place = Place.Incoming,
        suggestion = suggestion(Category.Movie, "sample", null, movies),
    )

    val sorted = Download(
        id = "d-harbor", kind = DownloadKind.Torrent, name = "Harbor.Lights.2019.1080p.BluRay.x264", source = "magnet:?xt=urn:btih:ghi",
        state = DownloadState.Sorted, progress = 1.0, totalBytes = 8_420_000_000, doneBytes = 8_420_000_000,
        createdAt = NOW - 7_200, completedAt = NOW - 180, itemIds = listOf("i-harbor"),
        destinations = listOf(Destination(Place.Library, movies.id, 1)),
    )
    val party = Download(
        id = "d-party", kind = DownloadKind.Upload, name = "Grandma 80th birthday.zip", state = DownloadState.Sorted, progress = 1.0,
        totalBytes = 1_240_000_000, doneBytes = 1_240_000_000, createdAt = NOW - 90_000, completedAt = NOW - 86_000,
        itemIds = listOf("a", "b", "c"),
        destinations = listOf(Destination(Place.Library, homeVideos.id, 12), Destination(Place.OtherFiles, null, 1)),
    )
    val waitingDownload = Download(
        id = "d-puddle-1", kind = DownloadKind.Torrent, name = "Puddle.Club.S01E01.720p", source = "magnet:?xt=urn:btih:jkl",
        state = DownloadState.Sorted, progress = 1.0, totalBytes = 287_000_000, doneBytes = 287_000_000,
        createdAt = NOW - 4_000, completedAt = NOW - 2_100, itemIds = listOf("i-puddle", "i-sub"),
        destinations = listOf(Destination(Place.Incoming, null, 2)),
    )
    val failed = Download(
        id = "d-iso", kind = DownloadKind.Http, name = "lantern-keepers-extras.zip", source = "https://media.example.org/lantern-keepers-extras.zip",
        state = DownloadState.Failed, error = "The server answered 404: the file is not there.", createdAt = NOW - 260_000,
    )

    fun rows(isOwner: Boolean = true): List<ImportRow> {
        val items = listOf(lowConfidence, episode, photo, document)
        return items.map { item -> row(item, isOwner, ignored = false) }
    }

    fun row(item: ItemDetail, isOwner: Boolean = true, ignored: Boolean = false, chosen: ImportTarget? = null): ImportRow {
        val choices = targetChoices(item, libraries, isOwner)
        val target = chosen ?: rowTarget(choices, suggestedTarget(item, libraries), null, false)
        return ImportRow(
            item = item,
            fileName = item.relPath.substringAfterLast('/'),
            reason = if (ignored) null else waitingReason(item.suggestion, isOwner),
            choices = choices,
            target = target,
            wantsDetails = target != null && needsDetails(target, item),
            busy = null,
            ignored = ignored,
        )
    }

    fun content(isOwner: Boolean = true) = DownloadsUiState(
        isOwner = isOwner,
        loading = false,
        vpn = vpnLine(vpn, unavailable = false, isOwner = isOwner),
        uploads = listOf(upload),
        inProgress = listOf(downloading, paused, queued),
        imports = rows(isOwner),
        ignored = listOf(row(ignoredOne, isOwner, ignored = true)),
        finished = listOf(sorted, waitingDownload, party, failed).map { FinishedRow(it, it.finishedOutcome(libraries, isOwner)) },
    )

    val loading = DownloadsUiState(isOwner = true, loading = true)

    val empty = DownloadsUiState(isOwner = true, loading = false, vpn = vpnLine(vpn, unavailable = false, isOwner = true))

    val failedToLoad = DownloadsUiState(
        isOwner = false,
        loading = false,
        loadError = "PopcornStash did not answer. Check the connection and try again.",
        connectionLost = true,
        vpn = vpnLine(null, unavailable = true, isOwner = false),
    )
}
