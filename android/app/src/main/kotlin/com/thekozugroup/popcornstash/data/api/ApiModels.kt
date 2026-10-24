package com.thekozugroup.popcornstash.data.api

import kotlinx.serialization.KSerializer
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.descriptors.PrimitiveKind
import kotlinx.serialization.descriptors.PrimitiveSerialDescriptor
import kotlinx.serialization.encoding.Decoder
import kotlinx.serialization.encoding.Encoder
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject

// The JSON shapes of docs/architecture.md section 8. Property names are camelCase here and
// snake_case on the wire (StashJson's naming strategy). Enums carry an Unknown value so a
// newer server's new word does not break an older app.

@Serializable
enum class Role {
    @SerialName("owner") Owner,
    @SerialName("member") Member,
    Unknown,
}

@Serializable
data class User(
    val id: String,
    val displayName: String,
    val role: Role = Role.Unknown,
    val createdAt: Long = 0,
)

@Serializable
data class Health(val status: String, val version: String, val source: String? = null)

@Serializable
data class AuthState(
    val setupRequired: Boolean = false,
    val authenticated: Boolean = false,
    val user: User? = null,
    val publicUrl: String? = null,
    val rpId: String? = null,
    val passkeysReady: Boolean = false,
    val reason: String? = null,
)

// ------------------------------------------------------------------ sign-in ----

@Serializable
data class PairingRequest(val clientName: String, val clientKind: String = "android")

@Serializable
data class PairingStarted(
    val pairingId: String,
    val userCode: String,
    val pollSecret: String,
    val expiresAt: Long,
    val verifyUrl: String,
    /** Seconds between polls. */
    val interval: Int = 6,
) {
    override fun toString() = "PairingStarted(pairingId=$pairingId, userCode=$userCode, pollSecret=<redacted>)"
}

@Serializable
data class PairingPollRequest(val pairingId: String, val pollSecret: String) {
    override fun toString() = "PairingPollRequest(pairingId=$pairingId, pollSecret=<redacted>)"
}

@Serializable
data class PairingPollAnswer(
    val status: String,
    val token: String? = null,
    val scopes: List<String> = emptyList(),
    val serverName: String? = null,
) {
    override fun toString() = "PairingPollAnswer(status=$status, token=<redacted>, scopes=$scopes)"
}

@Serializable
data class LoginStarted(val flowId: String, val options: JsonObject)

@Serializable
data class LoginFinishRequest(
    val flowId: String,
    val credential: JsonElement,
    val wantToken: Boolean = true,
    val deviceName: String? = null,
)

@Serializable
data class LoginFinished(val user: User, val token: String? = null) {
    override fun toString() = "LoginFinished(user=$user, token=<redacted>)"
}

// ---------------------------------------------------------------- libraries ----

@Serializable
enum class ContentType {
    @SerialName("video") Video,
    @SerialName("photos") Photos,
    Unknown,
}

@Serializable
enum class Structure {
    @SerialName("episodes") Episodes,
    @SerialName("one_offs") OneOffs,
}

/**
 * A kind of file a library can claim, and an item's category. Read leniently (a word this
 * app does not know becomes [Unknown]) because it also appears inside lists.
 */
@Serializable(with = CategorySerializer::class)
enum class Category(val wire: String) {
    Movie("movie"),
    Episode("episode"),
    HomeVideo("home_video"),
    Photo("photo"),
    Other("other"),
    Unknown("unknown"),
}

object CategorySerializer : KSerializer<Category> {
    override val descriptor = PrimitiveSerialDescriptor("Category", PrimitiveKind.STRING)
    override fun serialize(encoder: Encoder, value: Category) = encoder.encodeString(value.wire)
    override fun deserialize(decoder: Decoder): Category {
        val wire = decoder.decodeString()
        return Category.entries.firstOrNull { it.wire == wire } ?: Category.Unknown
    }
}

@Serializable
enum class Layout {
    @SerialName("posters") Posters,
    @SerialName("frames") Frames,
    @SerialName("series") Series,
    @SerialName("timeline") Timeline,
    Unknown,
}

@Serializable
enum class Visibility {
    @SerialName("everyone") Everyone,
    @SerialName("selected") Selected,
}

@Serializable
enum class FolderState {
    @SerialName("ok") Ok,
    @SerialName("missing") Missing,
    @SerialName("read_only") ReadOnly,
    Unknown,
}

@Serializable
data class Library(
    val id: String,
    val name: String,
    val icon: String,
    val contentType: ContentType = ContentType.Unknown,
    val structure: Structure? = null,
    val claims: List<Category> = emptyList(),
    val layout: Layout = Layout.Unknown,
    val position: Int = 0,
    val itemCount: Long = 0,
    val showCount: Long? = null,
    val createdAt: Long = 0,
    val updatedAt: Long = 0,
    // Full owners only; null for everyone else.
    val folder: String? = null,
    val visibility: Visibility? = null,
    val memberIds: List<String>? = null,
    val folderState: FolderState? = null,
    val isEmpty: Boolean? = null,
)

@Serializable
data class LibraryList(val libraries: List<Library>)

@Serializable
data class LibraryIconList(val icons: List<String>)

@Serializable
data class SuggestedType(val contentType: ContentType, val structure: Structure? = null)

@Serializable
data class UnassignedFolder(
    val name: String,
    val itemCount: Long = 0,
    val videoCount: Long = 0,
    val photoCount: Long = 0,
    val otherCount: Long = 0,
    val suggested: SuggestedType,
)

@Serializable
data class FolderList(val folders: List<UnassignedFolder>)

@Serializable
data class FolderChoice(
    /** `new` makes the folder; `existing` takes a folder from [FolderList]. */
    val mode: String,
    val name: String,
)

@Serializable
data class LibraryCreate(
    val name: String,
    val icon: String,
    val contentType: ContentType,
    val structure: Structure? = null,
    val claims: List<Category> = emptyList(),
    val folder: FolderChoice,
    val visibility: Visibility? = null,
    val memberIds: List<String> = emptyList(),
    val position: Int? = null,
)

/**
 * A library edit (`PATCH /api/libraries/:id`): only the fields that are set are sent.
 * [structure] is sent when [structureSet] is true, as null for a photos library.
 */
data class LibraryChanges(
    val name: String? = null,
    val icon: String? = null,
    val contentType: ContentType? = null,
    val structure: Structure? = null,
    val structureSet: Boolean = false,
    val claims: List<Category>? = null,
    val visibility: Visibility? = null,
    val memberIds: List<String>? = null,
)

@Serializable
data class LibraryOrder(val ids: List<String>)

// -------------------------------------------------------------------- items ----

@Serializable
enum class MediaKind {
    @SerialName("video") Video,
    @SerialName("photo") Photo,
    @SerialName("audio") Audio,
    @SerialName("other") Other,
    Unknown,
}

@Serializable
enum class ItemStatus {
    @SerialName("ready") Ready,
    @SerialName("needs_review") NeedsReview,
    @SerialName("missing") Missing,
    @SerialName("trashed") Trashed,
    Unknown,
}

@Serializable
enum class Place {
    @SerialName("library") Library,
    @SerialName("other_files") OtherFiles,
    @SerialName("incoming") Incoming,
    @SerialName("folder") Folder,
    @SerialName("trash") Trash,
    Unknown,
}

@Serializable
enum class Waiting {
    @SerialName("choose_library") ChooseLibrary,
    @SerialName("folder_unavailable") FolderUnavailable,
    @SerialName("low_confidence") LowConfidence,
    @SerialName("confirm") Confirm,
    Unknown,
}

@Serializable
data class Progress(val positionS: Double, val durationS: Double, val finished: Boolean = false)

@Serializable
data class Item(
    val id: String,
    val kind: MediaKind = MediaKind.Unknown,
    val category: Category = Category.Unknown,
    val status: ItemStatus = ItemStatus.Unknown,
    val title: String,
    val year: Int? = null,
    val showName: String? = null,
    val season: Int? = null,
    val episode: Int? = null,
    val episodeTitle: String? = null,
    val relPath: String = "",
    val size: Long = 0,
    val durationS: Double? = null,
    val width: Int? = null,
    val height: Int? = null,
    /** The capture moment, unix seconds: for ordering and "how long ago", never shown as a date (use [takenLocal]). */
    val takenAt: Long? = null,
    /** The camera's clock, `YYYY-MM-DDTHH:MM:SS`: what everyone sees, with no time zone conversion. */
    val takenLocal: String? = null,
    /** Minutes east of UTC that the file wrote beside the clock; null when it wrote none. */
    val takenOffset: Int? = null,
    val camera: String? = null,
    /** A server-relative thumbnail URL. */
    val thumb: String? = null,
    val favorite: Boolean = false,
    val addedAt: Long = 0,
    val progress: Progress? = null,
    val libraryId: String? = null,
    val place: Place = Place.Unknown,
)

/** What the classifier made of a file, and where it would go for this reader (`Classification`). */
@Serializable
data class Suggestion(
    val category: Category = Category.Unknown,
    val title: String = "",
    val year: Int? = null,
    val showName: String? = null,
    val season: Int? = null,
    val episode: Int? = null,
    val episodeTitle: String? = null,
    val takenAt: Long? = null,
    /** As on [Item]: where a photo or camera video would be filed (its `YYYY/MM/`). */
    val takenLocal: String? = null,
    val takenOffset: Int? = null,
    val confidence: String = "high",
    /** The classifier's sentence about the file ("Not sure this is a movie."). */
    val reason: String = "",
    val place: Place? = null,
    val libraryId: String? = null,
    val destination: String? = null,
    val waiting: Waiting? = null,
)

@Serializable
data class VideoStream(
    val index: Int,
    val codec: String,
    val profile: String? = null,
    val width: Int = 0,
    val height: Int = 0,
    val fps: Double? = null,
    val hdr: String? = null,
    val rotation: Int = 0,
)

@Serializable
data class AudioStream(
    val index: Int,
    val codec: String,
    val channels: Int? = null,
    val language: String? = null,
    val title: String? = null,
)

@Serializable
data class SubtitleStream(
    val index: Int,
    val codec: String,
    val language: String? = null,
    val title: String? = null,
    val external: Boolean = false,
)

@Serializable
data class MediaSummary(
    val container: String? = null,
    val durationS: Double? = null,
    val bitrate: Long? = null,
    val video: VideoStream? = null,
    val audio: List<AudioStream> = emptyList(),
    val subtitles: List<SubtitleStream> = emptyList(),
)

/** `ItemDetail`: an item with its media facts, suggestion and neighbours in the same library. */
@Serializable
data class ItemDetail(
    val id: String,
    val kind: MediaKind = MediaKind.Unknown,
    val category: Category = Category.Unknown,
    val status: ItemStatus = ItemStatus.Unknown,
    val title: String,
    val year: Int? = null,
    val showName: String? = null,
    val season: Int? = null,
    val episode: Int? = null,
    val episodeTitle: String? = null,
    val relPath: String = "",
    val size: Long = 0,
    val durationS: Double? = null,
    val width: Int? = null,
    val height: Int? = null,
    val takenAt: Long? = null,
    val takenLocal: String? = null,
    val takenOffset: Int? = null,
    val camera: String? = null,
    val thumb: String? = null,
    val favorite: Boolean = false,
    val addedAt: Long = 0,
    val progress: Progress? = null,
    val libraryId: String? = null,
    val place: Place = Place.Unknown,
    val media: MediaSummary? = null,
    val mime: String? = null,
    val suggestion: Suggestion? = null,
    val sourceDownloadId: String? = null,
    val nextEpisodeId: String? = null,
    val previousEpisodeId: String? = null,
) {
    val item: Item
        get() = Item(
            id, kind, category, status, title, year, showName, season, episode, episodeTitle, relPath, size,
            durationS, width, height, takenAt, takenLocal, takenOffset, camera, thumb, favorite, addedAt, progress, libraryId, place,
        )
}

@Serializable
data class ItemPage(val items: List<Item>, val nextCursor: String? = null)

/** `GET /api/items` filters; null ones are left out. */
data class ItemQuery(
    val downloadGroup: String? = null,
    val downloadFolder: String? = null,
    val folder: String? = null,
    val includeInbox: Boolean? = null,
    val libraryId: String? = null,
    val place: Place? = null,
    val category: Category? = null,
    val q: String? = null,
    /** `added`, `title`, `taken` or `size`. */
    val sort: String? = null,
    /** `asc` or `desc`. */
    val order: String? = null,
    val favorite: Boolean? = null,
    val year: Int? = null,
    val cursor: String? = null,
    val limit: Int? = null,
)

/** `PATCH /api/items/:id`: metadata only; null fields are left as they are. */
@Serializable
data class ItemPatch(
    val title: String? = null,
    val year: Int? = null,
    val favorite: Boolean? = null,
    val showName: String? = null,
    val season: Int? = null,
    val episode: Int? = null,
    val episodeTitle: String? = null,
)

@Serializable
data class HomeLibrary(
    val library: Library,
    val recent: List<Item> = emptyList(),
    /** For each show with an episode in [recent]: the item whose picture stands for the show. Kept for older apps. */
    val showPosters: Map<String, String> = emptyMap(),
    /**
     * A series library's shelf: up to 12 of its shows, the one with the most recently added
     * episode first, one per show while it has 12 or fewer. Empty for other layouts; null from a
     * server older than the field, where the shelf is built from [recent] as before.
     */
    val shows: List<HomeShow>? = null,
)

/** One show on a series library's Home shelf (`HomeShow`). */
@Serializable
data class HomeShow(
    val showName: String,
    /** The show's picture in this library, the same one the shows page uses. */
    val posterItemId: String? = null,
    /** The show's ready episodes in this library. */
    val itemCount: Long = 0,
    /** Of those, the ones added in the last 30 days. */
    val newCount: Long = 0,
    /** The show's most recently added episode. */
    val latest: Item,
    val lastAddedAt: Long = 0,
)

@Serializable
data class Home(
    val continueWatching: List<Item> = emptyList(),
    val libraries: List<HomeLibrary> = emptyList(),
    val otherFilesCount: Long = 0,
    val inboxCount: Long = 0,
    val totalBytes: Long = 0,
)

@Serializable
data class ShowSummary(
    val showName: String,
    val itemCount: Long = 0,
    val seasonCount: Long = 0,
    val posterItemId: String? = null,
    val lastAddedAt: Long = 0,
)

@Serializable
data class Season(val season: Int, val episodes: List<Item> = emptyList())

@Serializable
data class ShowDetail(val showName: String, val posterItemId: String? = null, val seasons: List<Season> = emptyList())

@Serializable
data class PhotoMonth(
    /** `YYYY-MM` */
    val month: String,
    val count: Long,
    val items: List<Item> = emptyList(),
)

@Serializable
data class PhotoTimeline(val months: List<PhotoMonth> = emptyList(), val nextCursor: String? = null)

// ---------------------------------------------------------------- playback ----

@Serializable
data class PlaybackRequest(
    val videoCodecs: List<String>,
    val audioCodecs: List<String>,
    val hdr: Boolean,
    val maxHeight: Int? = null,
    val startS: Double? = null,
    val audioIndex: Int? = null,
)

@Serializable
data class PlaybackDecision(
    /** `direct` or `hls`. */
    val mode: String,
    /** Server-relative: the file for direct play, the playlist for HLS. */
    val url: String,
    val sessionId: String? = null,
)

@Serializable
data class ProgressUpdate(val positionS: Double, val durationS: Double)

// -------------------------------------------------------------------- inbox ----

@Serializable
data class Inbox(val items: List<ItemDetail> = emptyList(), val ignored: List<ItemDetail> = emptyList())

/**
 * `POST /api/inbox/:id`. Accepting always names the [place] and [libraryId] the row showed
 * (docs/architecture.md section 8), so a file never goes somewhere the person did not see.
 */
@Serializable
data class InboxAction(
    /** `accept` or `ignore`. */
    val action: String,
    val place: Place? = null,
    val libraryId: String? = null,
    val category: Category? = null,
    val title: String? = null,
    val year: Int? = null,
    val showName: String? = null,
    val season: Int? = null,
    val episode: Int? = null,
)

/** `POST /api/items/:id/organize` (full owners): the same target fields as an Inbox accept. */
@Serializable
data class OrganizeRequest(
    val place: Place? = null,
    val libraryId: String? = null,
    val category: Category? = null,
    val title: String? = null,
    val year: Int? = null,
    val showName: String? = null,
    val season: Int? = null,
    val episode: Int? = null,
)

// ---------------------------------------------------------------- downloads ----

@Serializable
enum class DownloadKind {
    @SerialName("torrent") Torrent,
    @SerialName("http") Http,
    @SerialName("upload") Upload,
    Unknown,
}

@Serializable
enum class DownloadState {
    @SerialName("queued") Queued,
    @SerialName("downloading") Downloading,
    @SerialName("paused") Paused,
    @SerialName("seeding") Seeding,
    @SerialName("completed") Completed,
    @SerialName("sorting") Sorting,
    @SerialName("sorted") Sorted,
    @SerialName("failed") Failed,
    @SerialName("cancelled") Cancelled,
    Unknown,
}

@Serializable
data class Destination(val place: Place, val libraryId: String? = null, val count: Long = 0)

@Serializable
data class Download(
    val id: String,
    val kind: DownloadKind = DownloadKind.Unknown,
    val name: String,
    val source: String = "",
    val state: DownloadState = DownloadState.Unknown,
    val progress: Double = 0.0,
    val totalBytes: Long? = null,
    val doneBytes: Long = 0,
    val downBps: Long = 0,
    val upBps: Long = 0,
    val peers: Long? = null,
    val etaS: Long? = null,
    val error: String? = null,
    val createdAt: Long = 0,
    val completedAt: Long? = null,
    val itemIds: List<String> = emptyList(),
    val destinations: List<Destination> = emptyList(),
    val libraryId: String? = null,
)

@Serializable
data class DownloadList(val items: List<Download>)

@Serializable
data class AddDownload(val url: String, val filename: String? = null, val libraryId: String? = null, val fileTypes: List<TorrentFileKind>? = null, val fileRules: TorrentFileRules? = null)

@Serializable
data class TorrentFileRules(
    val fileTypes: List<TorrentFileKind>? = null,
    val nameContains: List<String> = emptyList(),
    val nameExcludes: List<String> = emptyList(),
    val minSize: Long? = null,
    val maxSize: Long? = null,
)

@Serializable
enum class TorrentFileKind {
    @SerialName("video") Video,
    @SerialName("subtitle") Subtitle,
    @SerialName("image") Image,
    @SerialName("other") Other;

    val label: String get() = when (this) {
        Video -> "Videos"
        Subtitle -> "Subtitles"
        Image -> "Images"
        Other -> "Other files"
    }
    companion object { val Default = listOf(Video, Subtitle) }
}

// ------------------------------------------------------------------ uploads ----

@Serializable
data class UploadCreate(val filename: String, val size: Long, val mime: String? = null, val lastModified: Long? = null, val libraryId: String? = null)

@Serializable
data class UploadStarted(val id: String, val chunkSize: Long, val received: Long = 0)

@Serializable
data class UploadStatus(
    val id: String,
    val filename: String = "",
    val size: Long = 0,
    val received: Long = 0,
    val state: String = "",
)

@Serializable
data class UploadReceived(val received: Long)

// ---------------------------------------------------------------------- VPN ----

@Serializable
enum class VpnState {
    @SerialName("connected") Connected,
    @SerialName("connecting") Connecting,
    @SerialName("disconnected") Disconnected,
    @SerialName("error") Error,
    @SerialName("not_configured") NotConfigured,
    @SerialName("unreachable") Unreachable,
    Unknown,
}

@Serializable
data class VpnStatus(
    val configured: Boolean = false,
    val reachable: Boolean = false,
    val state: VpnState = VpnState.Unknown,
    val provider: String? = null,
    val protocol: String? = null,
    val label: String? = null,
    val server: String? = null,
    val country: String? = null,
    val publicIp: String? = null,
    val protected: Boolean? = null,
    val since: Long? = null,
    val error: String? = null,
    val killSwitch: Boolean = false,
    val allowNoVpn: Boolean = false,
    val torrentsOff: Boolean = false,
)


@Serializable
data class LibraryFolders(val folder: String = "", val folders: List<String> = emptyList())
@Serializable
data class FileBatchResult(val completed: List<String> = emptyList(), val failed: List<FileBatchFailure> = emptyList())
@Serializable
data class FileBatchFailure(val id: String, val message: String)
@Serializable
data class MoveFiles(val itemIds: List<String>, val folder: String)
@Serializable
data class SelectedFiles(val itemIds: List<String>)

@Serializable
data class DownloadFolder(val key: String, val name: String, @SerialName("item_count") val itemCount: Int, val size: Long, @SerialName("added_at") val addedAt: Long, val custom: Boolean = false)


@Serializable
data class FlattenPreview(val key: String, val folder: String, val files: List<FlattenFile>, val blocked: List<String>, val token: String)
@Serializable
data class FlattenFile(@SerialName("item_id") val itemId: String, val from: String, val to: String, val kind: String, val fingerprint: String)

@Serializable
data class DownloadTree(val folder:String, val folders:List<DownloadTreeFolder>)
@Serializable
data class DownloadTreeFolder(val name:String, val folder:String, val itemCount:Int, val size:Long)
@Serializable
data class RenameFolderPreview(val from:String,val to:String,val fileCount:Int,val folderCount:Int,val blocked:List<String>,val token:String)
