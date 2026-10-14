package com.thekozugroup.popcornstash.feature.settings

import com.thekozugroup.popcornstash.data.api.Library
import com.thekozugroup.popcornstash.data.api.StashJson
import com.thekozugroup.popcornstash.data.api.UnassignedFolder
import com.thekozugroup.popcornstash.data.api.User
import mockwebserver3.Dispatcher
import mockwebserver3.MockResponse
import mockwebserver3.RecordedRequest
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit

/**
 * A server that answers by method and path, because Settings asks for several things at once
 * (the libraries, the people, the folders, the icons) and their order on the wire is not fixed.
 * Each route answers its queued responses in turn and keeps repeating the last one.
 */
class Routes : Dispatcher() {
    private val answers = HashMap<String, ArrayDeque<MockResponse>>()
    private val seen = LinkedBlockingQueue<RecordedRequest>()

    fun on(method: String, path: String, body: String = "", code: Int = 200): Routes {
        val response = MockResponse.Builder().code(code).apply {
            if (body.isNotEmpty()) {
                body(body)
                addHeader("Content-Type", "application/json")
            }
        }.build()
        synchronized(answers) { answers.getOrPut("$method $path") { ArrayDeque() }.addLast(response) }
        return this
    }

    /** Replaces what a route answers from now on. */
    fun only(method: String, path: String, body: String = "", code: Int = 200): Routes {
        synchronized(answers) { answers.remove("$method $path") }
        return on(method, path, body, code)
    }

    override fun dispatch(request: RecordedRequest): MockResponse {
        seen.add(request)
        val key = "${request.method} ${request.url.encodedPath}"
        synchronized(answers) {
            val queue = answers[key] ?: return MockResponse.Builder().code(404).body("""{"error":{"code":"not_found","message":"No route for $key."}}""").build()
            return if (queue.size > 1) queue.removeFirst() else queue.first()
        }
    }

    /** The next request that is not a plain read, waiting up to five seconds. */
    fun nextWrite(): RecordedRequest {
        while (true) {
            val request = seen.poll(5, TimeUnit.SECONDS) ?: error("No write reached the server")
            if (request.method != "GET") return request
        }
    }

    /** Every request seen so far, as "METHOD path". */
    fun drain(): List<String> = buildList {
        while (true) add(seen.poll()?.let { "${it.method} ${it.url.encodedPath}" } ?: break)
    }
}

/** Owner-level answers: full library fields, the people, folders that are not a library. */
object SettingsFixtures {
    const val MOVIES_ID = "0192a000-0000-7000-8000-000000000001"
    const val SHOWS_ID = "0192a000-0000-7000-8000-000000000002"
    const val HOME_VIDEOS_ID = "0192a000-0000-7000-8000-000000000003"
    const val PHOTOS_ID = "0192a000-0000-7000-8000-000000000004"
    const val KIDS_ID = "0192a000-0000-7000-8000-000000000009"

    fun library(
        id: String,
        name: String,
        icon: String,
        contentType: String,
        structure: String?,
        claims: List<String>,
        layout: String,
        position: Int,
        itemCount: Int = 0,
        folder: String = name,
        visibility: String = "everyone",
        memberIds: List<String> = emptyList(),
        folderState: String = "ok",
        isEmpty: Boolean = itemCount == 0,
    ): String = """{"id":"$id","name":"$name","icon":"$icon","content_type":"$contentType",
        "structure":${structure?.let { "\"$it\"" } ?: "null"},"claims":[${claims.joinToString(",") { "\"$it\"" }}],
        "layout":"$layout","position":$position,"item_count":$itemCount,"show_count":null,
        "created_at":1760400000,"updated_at":1760400000,"folder":"$folder","visibility":"$visibility",
        "member_ids":[${memberIds.joinToString(",") { "\"$it\"" }}],"folder_state":"$folderState","is_empty":$isEmpty}"""

    val MOVIES = library(MOVIES_ID, "Movies", "film", "video", "one_offs", listOf("movie"), "posters", 0, itemCount = 214)
    val SHOWS = library(SHOWS_ID, "Shows", "tv", "video", "episodes", listOf("episode"), "series", 1, itemCount = 140)
    val HOME_VIDEOS = library(HOME_VIDEOS_ID, "Home Videos", "clapperboard", "video", "one_offs", listOf("home_video"), "frames", 2, itemCount = 58)
    val PHOTOS = library(PHOTOS_ID, "Photos", "image", "photos", null, listOf("photo"), "timeline", 3, itemCount = 3214)
    val KIDS = library(
        KIDS_ID, "Kids", "baby", "video", "episodes", emptyList(), "series", 4,
        visibility = "selected", memberIds = listOf("u-tomas"),
    )

    fun list(vararg libraries: String) = """{"libraries":[${libraries.joinToString(",")}]}"""

    val FOUR = list(MOVIES, SHOWS, HOME_VIDEOS, PHOTOS)
    val FIVE = list(MOVIES, SHOWS, HOME_VIDEOS, PHOTOS, KIDS)

    const val PEOPLE = """[{"id":"u-owner","display_name":"Ana Ribeiro","role":"owner","created_at":1760400000},
        {"id":"u-tomas","display_name":"Tomas","role":"member","created_at":1760400100},
        {"id":"u-ines","display_name":"Ines","role":"member","created_at":1760400200}]"""

    const val FOLDERS = """{"folders":[
        {"name":"Family 2019","item_count":36,"video_count":12,"photo_count":20,"other_count":4,
         "suggested":{"content_type":"photos","structure":null}},
        {"name":"Grandma's scans","item_count":312,"video_count":0,"photo_count":312,"other_count":0,
         "suggested":{"content_type":"photos","structure":null}},
        {"name":"Cartoons","item_count":40,"video_count":40,"photo_count":0,"other_count":0,
         "suggested":{"content_type":"video","structure":"episodes"}}]}"""

    const val NO_FOLDERS = """{"folders":[]}"""

    const val ICONS = """{"icons":["film","clapperboard","tv","popcorn","projector","video","drama","image","images","camera",
        "aperture","users","baby","heart","cake","party-popper","graduation-cap","gift","plane","tree-palm","mountain-snow",
        "tent","sun","snowflake","gamepad-2","music","book-open","trophy","paw-print","bike","star","folder"]}"""

    /** A server where an owner sees the four starting libraries, three people and three folders. */
    fun ownerServer(libraries: String = FOUR) = Routes()
        .on("GET", "/api/libraries", libraries)
        .on("GET", "/api/users", PEOPLE)
        .on("GET", "/api/libraries/folders", FOLDERS)
        .on("GET", "/api/libraries/icons", ICONS)

    fun libraries(json: String = FIVE): List<Library> =
        StashJson.decodeFromString(com.thekozugroup.popcornstash.data.api.LibraryList.serializer(), json).libraries

    fun people(): List<User> = StashJson.decodeFromString(kotlinx.serialization.builtins.ListSerializer(User.serializer()), PEOPLE)

    fun folders(): List<UnassignedFolder> =
        StashJson.decodeFromString(com.thekozugroup.popcornstash.data.api.FolderList.serializer(), FOLDERS).folders
}
