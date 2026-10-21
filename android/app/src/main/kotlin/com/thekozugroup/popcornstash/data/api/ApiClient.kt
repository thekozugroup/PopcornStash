package com.thekozugroup.popcornstash.data.api

import kotlinx.serialization.DeserializationStrategy
import kotlinx.serialization.KSerializer
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import okhttp3.HttpUrl
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody
import okhttp3.Response
import okhttp3.RequestBody.Companion.toRequestBody
import java.io.IOException

/**
 * Every call the app makes to a PopcornStash server (docs/architecture.md sections 8 and 10).
 *
 * [http] decides who is asking: the plain client for the public sign-in calls, or a session
 * client whose [AuthInterceptor] adds the bearer token and reports a 401. Every failure is an
 * [ApiException] with a sentence that can be shown as it is.
 */
class ApiClient(val server: HttpUrl, private val http: OkHttpClient) {
    private val json = StashJson

    // ------------------------------------------------------------- public ----

    /** `GET /api/health`: is a PopcornStash server answering at this address? */
    suspend fun health(): Health = get("/api/health", Health.serializer())

    suspend fun authState(): AuthState = get("/api/auth/state", AuthState.serializer())

    /**
     * Why the RP host's verification file does not permit this signed app, or null
     * when its package, both permissions and a signing certificate match. The caller
     * chooses the RP host address; this local fetch cannot prove Google reachability.
     */
    suspend fun appPasskeyProblem(packageName: String, signingFingerprints: Set<String>): String? {
        val body = try {
            send(request("/.well-known/assetlinks.json").get().build(), allowEmpty = true)
        } catch (_: ApiException) {
            return "The server's app verification file could not be opened. Pair with a code instead."
        }
        return assetLinksProblem(body, packageName, signingFingerprints)
    }

    suspend fun requestPairing(clientName: String): PairingStarted =
        post("/api/pairing/request", PairingRequest(clientName, "android"), PairingRequest.serializer(), PairingStarted.serializer())

    /**
     * `POST /api/pairing/poll`: 202 `pending`, 200 `approved` with the token (delivered once),
     * 410 `denied` or `expired`. An unknown or purged request (404) is an [ApiException].
     */
    suspend fun pollPairing(pairingId: String, pollSecret: String): PairingPollAnswer {
        val body = json.encodeToString(PairingPollRequest.serializer(), PairingPollRequest(pairingId, pollSecret))
        val response = execute(request("/api/pairing/poll").post(body.toRequestBody(JSON)).build())
        response.use {
            val text = it.text()
            return when (it.code) {
                200, 202, 410 -> decode(PairingPollAnswer.serializer(), text, it.code)
                else -> throw errorFrom(it.code, text, json)
            }
        }
    }

    suspend fun startLogin(): LoginStarted =
        post("/api/auth/login/start", JsonObject(emptyMap()), JsonObject.serializer(), LoginStarted.serializer())

    suspend fun finishLogin(finish: LoginFinishRequest): LoginFinished =
        post("/api/auth/login/finish", finish, LoginFinishRequest.serializer(), LoginFinished.serializer())

    // ------------------------------------------------------------ session ----

    suspend fun logout() = sendEmpty(request("/api/auth/logout").post(EMPTY_JSON).build())

    suspend fun me(): User = get("/api/me", User.serializer())

    /** Owners: everyone with an account, for a library's people picker. */
    suspend fun users(): List<User> = get("/api/users", ListSerializer(User.serializer()))

    // ---------------------------------------------------- home and libraries ----

    suspend fun home(): Home = get("/api/home", Home.serializer())

    suspend fun libraries(): List<Library> = get("/api/libraries", LibraryList.serializer()).libraries

    suspend fun libraryConversion(id: String): LibraryConversion = get("/api/libraries/${seg(id)}/conversion", LibraryConversion.serializer())

    suspend fun saveLibraryConversion(id: String, policy: ConversionPolicy): LibraryConversion = send(
        request("/api/libraries/${seg(id)}/conversion").put(json.encodeToString(ConversionPolicy.serializer(), policy).toRequestBody(JSON)).build(), LibraryConversion.serializer(),
    )

    suspend fun retryLibraryConversion(id: String): LibraryConversion = send(
        request("/api/libraries/${seg(id)}/conversion/retry").post(EMPTY_JSON).build(), LibraryConversion.serializer(),
    )

    suspend fun library(id: String): Library = get("/api/libraries/${seg(id)}", Library.serializer())

    suspend fun downloadFolders(id: String): List<DownloadFolder> = get("/api/libraries/${seg(id)}/download-folders", ListSerializer(DownloadFolder.serializer()))

    suspend fun nameDownloadFolder(id: String, name: String, key: String? = null): String = send(
        request("/api/libraries/${seg(id)}/download-folders").post(buildJsonObject { put("name", name); key?.let { put("key", it) } }.toString().toRequestBody(JSON)).build(), JsonObject.serializer(),
    ).getValue("key").jsonPrimitive.content

    suspend fun moveToDownloadFolder(id: String, key: String, itemIds: List<String>) = sendEmpty(
        request("/api/libraries/${seg(id)}/download-folders/move").post(buildJsonObject { put("key", key); put("item_ids", JsonArray(itemIds.map { JsonPrimitive(it) })) }.toString().toRequestBody(JSON)).build(),
    )

    suspend fun libraryFolders(id: String, folder: String): LibraryFolders =
        get("/api/libraries/${seg(id)}/folders", LibraryFolders.serializer(), "folder" to folder)

    suspend fun createLibraryFolder(id: String, parent: String, name: String) = sendEmpty(
        request("/api/libraries/${seg(id)}/folders").post(buildJsonObject { put("parent", parent); put("name", name) }.toString().toRequestBody(JSON)).build(),
    )

    suspend fun moveLibraryFiles(id: String, itemIds: List<String>, folder: String): FileBatchResult =
        post("/api/libraries/${seg(id)}/move", MoveFiles(itemIds, folder), MoveFiles.serializer(), FileBatchResult.serializer())

    suspend fun trashInboxFiles(itemIds: List<String>): FileBatchResult =
        post("/api/inbox/trash", SelectedFiles(itemIds), SelectedFiles.serializer(), FileBatchResult.serializer())

    suspend fun libraryIcons(): List<String> = get("/api/libraries/icons", LibraryIconList.serializer()).icons

    /** Owners: top-level folders that are not a library, by name. */
    suspend fun unassignedFolders(): List<UnassignedFolder> = get("/api/libraries/folders", FolderList.serializer()).folders

    suspend fun createLibrary(create: LibraryCreate): Library =
        post("/api/libraries", create, LibraryCreate.serializer(), Library.serializer())

    suspend fun editLibrary(id: String, changes: LibraryChanges): Library {
        val body = buildJsonObject {
            changes.name?.let { put("name", it) }
            changes.icon?.let { put("icon", it) }
            changes.contentType?.let { put("content_type", json.encodeToJsonElement(ContentType.serializer(), it)) }
            if (changes.structureSet) {
                put("structure", changes.structure?.let { json.encodeToJsonElement(Structure.serializer(), it) } ?: JsonNull)
            }
            changes.claims?.let { claims -> put("claims", JsonArray(claims.map { JsonPrimitive(it.wire) })) }
            changes.visibility?.let { put("visibility", json.encodeToJsonElement(Visibility.serializer(), it)) }
            changes.memberIds?.let { ids -> put("member_ids", JsonArray(ids.map { JsonPrimitive(it) })) }
        }
        return send(
            request("/api/libraries/${seg(id)}").patch(body.toString().toRequestBody(JSON)).build(),
            Library.serializer(),
        )
    }

    suspend fun reorderLibraries(ids: List<String>): List<Library> =
        send(
            request("/api/libraries/order")
                .put(json.encodeToString(LibraryOrder.serializer(), LibraryOrder(ids)).toRequestBody(JSON))
                .build(),
            LibraryList.serializer(),
        ).libraries

    /** Optional confirmed folder removal uses a distinct endpoint: old servers fail closed. */
    suspend fun deleteLibrary(id: String, folder: String? = null) = sendEmpty(
        if (folder == null) request("/api/libraries/${seg(id)}").delete().build()
        else request("/api/libraries/${seg(id)}/trash")
            .post(buildJsonObject { put("confirm_folder", folder) }.toString().toRequestBody(JSON)).build(),
    )

    suspend fun shows(libraryId: String): List<ShowSummary> =
        get("/api/libraries/${seg(libraryId)}/shows", ListSerializer(ShowSummary.serializer()))

    suspend fun show(libraryId: String, showName: String): ShowDetail =
        get("/api/libraries/${seg(libraryId)}/shows/${seg(showName)}", ShowDetail.serializer())

    suspend fun timeline(libraryId: String, cursor: String? = null, limit: Int? = null): PhotoTimeline =
        get(
            "/api/libraries/${seg(libraryId)}/timeline",
            PhotoTimeline.serializer(),
            "cursor" to cursor,
            "limit" to limit?.toString(),
        )

    // -------------------------------------------------------------- items ----

    suspend fun items(query: ItemQuery = ItemQuery()): ItemPage =
        get(
            "/api/items",
            ItemPage.serializer(),
            "library_id" to query.libraryId,
            "folder" to query.folder,
            "download_group" to query.downloadGroup,
            "include_inbox" to query.includeInbox?.toString(),
            "place" to query.place?.let { wire(Place.serializer(), it) },
            "category" to query.category?.wire,
            "q" to query.q,
            "sort" to query.sort,
            "order" to query.order,
            "favorite" to query.favorite?.toString(),
            "year" to query.year?.toString(),
            "cursor" to query.cursor,
            "limit" to query.limit?.toString(),
        )

    suspend fun item(id: String): ItemDetail = get("/api/items/${seg(id)}", ItemDetail.serializer())

    suspend fun updateItem(id: String, patch: ItemPatch): ItemDetail =
        send(
            request("/api/items/${seg(id)}").patch(json.encodeToString(ItemPatch.serializer(), patch).toRequestBody(JSON)).build(),
            ItemDetail.serializer(),
        )

    suspend fun organize(id: String, target: OrganizeRequest): ItemDetail =
        post("/api/items/${seg(id)}/organize", target, OrganizeRequest.serializer(), ItemDetail.serializer())

    /** Where an item's thumbnail is (`sm` or `lg`); load it with the session client. */
    fun thumbUrl(id: String, size: String = "sm"): HttpUrl =
        url("/api/items/${seg(id)}/thumb").newBuilder().addQueryParameter("size", size).build()

    /** A server-relative URL from an answer (`Item.thumb`, `PlaybackDecision.url`) made absolute. */
    fun absolute(path: String): HttpUrl = server.resolve(path) ?: throw ApiException.unexpected(200)

    fun fileUrl(id: String, download: Boolean = false): HttpUrl =
        url("/api/items/${seg(id)}/file").let { if (download) it.newBuilder().addQueryParameter("download", "1").build() else it }

    fun viewUrl(id: String): HttpUrl = url("/api/items/${seg(id)}/view")

    suspend fun playback(id: String, request: PlaybackRequest): PlaybackDecision =
        post("/api/items/${seg(id)}/playback", request, PlaybackRequest.serializer(), PlaybackDecision.serializer())

    /** Stops an HLS session when the player leaves. */
    suspend fun stopStream(sessionId: String) = sendEmpty(request("/api/stream/${seg(sessionId)}").delete().build())

    suspend fun saveProgress(id: String, positionS: Double, durationS: Double) =
        sendEmpty(
            request("/api/items/${seg(id)}/progress")
                .put(json.encodeToString(ProgressUpdate.serializer(), ProgressUpdate(positionS, durationS)).toRequestBody(JSON))
                .build(),
        )

    // -------------------------------------------------------------- inbox ----

    suspend fun inbox(): Inbox = get("/api/inbox", Inbox.serializer())

    suspend fun inboxAction(id: String, action: InboxAction): ItemDetail =
        post("/api/inbox/${seg(id)}", action, InboxAction.serializer(), ItemDetail.serializer())

    // ---------------------------------------------------------- downloads ----

    /** `state`: `active`, `finished` or `all`. */
    suspend fun downloads(state: String = "all", limit: Int? = null): List<Download> =
        get("/api/downloads", DownloadList.serializer(), "state" to state, "limit" to limit?.toString()).items

    suspend fun addDownload(url: String, filename: String? = null, libraryId: String? = null, fileTypes: List<TorrentFileKind>? = null, fileRules: TorrentFileRules? = null): Download =
        post("/api/downloads", AddDownload(url, filename, libraryId, fileTypes, fileRules), AddDownload.serializer(), Download.serializer())

    suspend fun torrentFiles(id: String): TorrentFiles = get("/api/downloads/${seg(id)}/files", TorrentFiles.serializer())

    suspend fun selectTorrentFiles(id: String, indices: List<Int>) = sendEmpty(
        request("/api/downloads/${seg(id)}/files")
            .put(json.encodeToString(TorrentFileSelection.serializer(), TorrentFileSelection(indices)).toRequestBody(JSON)).build(),
    )

    suspend fun startTorrent(id: String, indices: List<Int>): Download =
        post("/api/downloads/${seg(id)}/start", TorrentFileSelection(indices), TorrentFileSelection.serializer(), Download.serializer())

    suspend fun pauseDownload(id: String): Download =
        send(request("/api/downloads/${seg(id)}/pause").post(EMPTY_JSON).build(), Download.serializer())

    suspend fun resumeDownload(id: String): Download =
        send(request("/api/downloads/${seg(id)}/resume").post(EMPTY_JSON).build(), Download.serializer())

    suspend fun removeDownload(id: String, deleteFiles: Boolean = true) =
        sendEmpty(
            Request.Builder()
                .url(url("/api/downloads/${seg(id)}").newBuilder().addQueryParameter("delete_files", deleteFiles.toString()).build())
                .delete()
                .build(),
        )

    // ------------------------------------------------------------ uploads ----

    suspend fun startUpload(create: UploadCreate): UploadStarted =
        post("/api/uploads", create, UploadCreate.serializer(), UploadStarted.serializer())

    /**
     * Sends one chunk at [offset]. When the server holds a different amount (an earlier try
     * did arrive), it answers 409 with what it has, returned here so the upload goes on there.
     */
    suspend fun uploadChunk(id: String, offset: Long, bytes: RequestBody): UploadReceived {
        val request = Request.Builder()
            .url(url("/api/uploads/${seg(id)}").newBuilder().addQueryParameter("offset", offset.toString()).build())
            .put(bytes)
            .build()
        execute(request).use { response ->
            val text = response.text()
            if (response.isSuccessful) return decode(UploadReceived.serializer(), text, response.code)
            if (response.code == 409) {
                val received = runCatching { json.parseToJsonElement(text).jsonObject["received"]?.jsonPrimitive?.content?.toLong() }
                    .getOrNull()
                if (received != null) return UploadReceived(received)
            }
            throw errorFrom(response.code, text, json)
        }
    }

    suspend fun uploadStatus(id: String): UploadStatus = get("/api/uploads/${seg(id)}", UploadStatus.serializer())

    suspend fun completeUpload(id: String): Download =
        send(request("/api/uploads/${seg(id)}/complete").post(EMPTY_JSON).build(), Download.serializer())

    suspend fun cancelUpload(id: String) = sendEmpty(request("/api/uploads/${seg(id)}").delete().build())

    // ---------------------------------------------------------------- VPN ----

    suspend fun vpnStatus(): VpnStatus = get("/api/vpn/status", VpnStatus.serializer())

    // ------------------------------------------------------------ helpers ----

    /** The live events stream (`GET /api/events`); see [EventStream]. */
    fun eventsRequest(): Request = request("/api/events").header("Accept", "text/event-stream").get().build()

    fun url(path: String): HttpUrl = server.resolve(path) ?: throw IllegalArgumentException("Not a path: $path")

    private fun request(path: String): Request.Builder = Request.Builder().url(url(path))

    private suspend fun <T> get(path: String, decoder: DeserializationStrategy<T>, vararg query: Pair<String, String?>): T {
        val builder = url(path).newBuilder()
        for ((key, value) in query) if (value != null) builder.addQueryParameter(key, value)
        return send(Request.Builder().url(builder.build()).get().build(), decoder)
    }

    private suspend fun <B, T> post(path: String, body: B, encoder: KSerializer<B>, decoder: DeserializationStrategy<T>): T =
        send(request(path).post(json.encodeToString(encoder, body).toRequestBody(JSON)).build(), decoder)

    private suspend fun <T> send(request: Request, decoder: DeserializationStrategy<T>): T {
        val text = send(request, allowEmpty = false)
        return decode(decoder, text, 200)
    }

    private suspend fun sendEmpty(request: Request) {
        send(request, allowEmpty = true)
    }

    /** Sends [request] and returns the body of a 2xx answer; anything else is an [ApiException]. */
    private suspend fun send(request: Request, allowEmpty: Boolean): String {
        execute(request).use { response ->
            val text = response.text()
            if (!response.isSuccessful) throw errorFrom(response.code, text, json)
            if (text.isEmpty() && !allowEmpty) throw ApiException.unexpected(response.code)
            return text
        }
    }

    private suspend fun execute(request: Request) = try {
        http.newCall(request).await()
    } catch (_: IOException) {
        throw ApiException.network()
    }

    /**
     * The whole body of an answer. A connection that breaks while the body arrives is a network
     * failure like one that never answered, never an exception a caller does not expect.
     */
    private fun Response.text(): String = try {
        body.string()
    } catch (_: IOException) {
        throw ApiException.network()
    }

    private fun <T> decode(decoder: DeserializationStrategy<T>, text: String, status: Int): T = try {
        json.decodeFromString(decoder, text)
    } catch (_: IllegalArgumentException) {
        throw ApiException.unexpected(status)
    }

    private fun <T> wire(serializer: KSerializer<T>, value: T): String =
        json.encodeToJsonElement(serializer, value).jsonPrimitive.content

    companion object {
        val JSON = "application/json".toMediaType()
        private val EMPTY_JSON = "{}".toRequestBody(JSON)

        /** One path segment, escaped (a show name can hold spaces, slashes or question marks). */
        private fun seg(value: String): String =
            okhttp3.HttpUrl.Builder().scheme("https").host("x").addPathSegment(value).build().encodedPathSegments.single()
    }
}

/**
 * Validate the app-link statement for [packageName] and the installed signing certificates.
 * Null means the local statement matches; a sentence explains why pairing is needed otherwise.
 */
fun assetLinksProblem(body: String, packageName: String, signingFingerprints: Set<String>): String? {
    val statements = runCatching { StashJson.parseToJsonElement(body).jsonArray }.getOrNull()
        ?: return "The server's app verification file could not be read. Pair with a code instead."
    val targets = statements.mapNotNull { statement ->
        val obj = statement as? JsonObject ?: return@mapNotNull null
        val target = obj["target"] as? JsonObject ?: return@mapNotNull null
        if ((target["namespace"] as? JsonPrimitive)?.content != "android_app" ||
            (target["package_name"] as? JsonPrimitive)?.content != packageName
        ) return@mapNotNull null
        obj
    }
    if (targets.isEmpty()) return "The server has not enabled passkeys for this app. Pair with a code instead."
    val grants = targets.filter { obj ->
        val relations = (obj["relation"] as? JsonArray)?.mapNotNull { (it as? JsonPrimitive)?.content }.orEmpty()
        "delegate_permission/common.get_login_creds" in relations && "delegate_permission/common.handle_all_urls" in relations
    }
    if (grants.isEmpty()) return "The server's app verification file is missing a permission. Pair with a code instead."
    val own = signingFingerprints.mapNotNull(::certificateHex).toSet()
    if (own.isEmpty()) return "This app's signing certificate could not be checked. Pair with a code instead."
    val matches = grants.any { obj ->
        val target = obj["target"] as JsonObject
        (target["sha256_cert_fingerprints"] as? JsonArray).orEmpty().any {
            (it as? JsonPrimitive)?.content?.let(::certificateHex) in own
        }
    }
    return if (matches) null else "The server lists a different build of this app. Ask its owner to add this build's signing certificate, or pair with a code."
}

private fun certificateHex(value: String): String? = value.replace(":", "").lowercase()
    .takeIf { it.length == 64 && it.all { char -> char in '0'..'9' || char in 'a'..'f' } }
