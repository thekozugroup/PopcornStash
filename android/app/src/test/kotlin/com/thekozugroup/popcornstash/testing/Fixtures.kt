package com.thekozugroup.popcornstash.testing

import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import mockwebserver3.RecordedRequest
import java.util.concurrent.TimeUnit

/** Answers shaped like a real PopcornStash server's (docs/architecture.md section 8). */
object Fixtures {
    const val TOKEN = "pst_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"

    const val OWNER = """{"id":"u-owner","display_name":"Ana Ribeiro","role":"owner","created_at":1760400000}"""
    const val MEMBER = """{"id":"u-kid","display_name":"Tomas","role":"member","created_at":1760400100}"""

    const val MOVIES = """{"id":"0192a000-0000-7000-8000-000000000001","name":"Movies","icon":"film",
        "content_type":"video","structure":"one_offs","claims":["movie"],"layout":"posters","position":0,
        "item_count":3,"show_count":null,"created_at":1760400000,"updated_at":1760400000,
        "folder":"Movies","visibility":"everyone","member_ids":[],"folder_state":"ok","is_empty":false}"""

    const val SHOWS = """{"id":"0192a000-0000-7000-8000-000000000002","name":"Shows","icon":"tv",
        "content_type":"video","structure":"episodes","claims":["episode"],"layout":"series","position":1,
        "item_count":3,"show_count":1,"created_at":1760400000,"updated_at":1760400000,
        "folder":null,"visibility":null,"member_ids":null,"folder_state":null,"is_empty":null}"""

    const val PHOTOS = """{"id":"0192a000-0000-7000-8000-000000000004","name":"Photos","icon":"image",
        "content_type":"photos","structure":null,"claims":["photo","a_kind_from_the_future"],"layout":"timeline","position":2,
        "item_count":4,"show_count":null,"created_at":1760400000,"updated_at":1760400000}"""

    const val ITEM = """{"id":"i-paper","kind":"video","category":"movie","status":"ready","title":"Paper Boats",
        "year":2011,"show_name":null,"season":null,"episode":null,"episode_title":null,
        "rel_path":"Movies/Paper Boats (2011)/Paper Boats (2011).mp4","size":182044,"duration_s":6.0,
        "width":320,"height":180,"taken_at":null,"camera":null,"thumb":"/api/items/i-paper/thumb?v=3",
        "favorite":false,"added_at":1760400200,"progress":{"position_s":2.5,"duration_s":6.0,"finished":false},
        "library_id":"0192a000-0000-7000-8000-000000000001","place":"library"}"""

    const val EPISODE = """{"id":"i-lantern-1","kind":"video","category":"episode","status":"ready",
        "title":"The Lantern Keepers S01E01","year":null,"show_name":"The Lantern Keepers","season":1,"episode":1,
        "episode_title":"Pilot","rel_path":"Shows/The Lantern Keepers/Season 01/S01E01.mkv","size":200000,
        "duration_s":8.0,"width":320,"height":180,"taken_at":null,"camera":null,"thumb":null,"favorite":true,
        "added_at":1760400300,"progress":null,"library_id":"0192a000-0000-7000-8000-000000000002","place":"library"}"""

    const val WAITING = """{"id":"i-clip","kind":"video","category":"other","status":"needs_review","title":"clip_final_v2",
        "rel_path":"Inbox/clip_final_v2.mkv","size":50000,"thumb":null,"favorite":false,"added_at":1760400400,
        "progress":null,"library_id":null,"place":"incoming","media":{"container":"matroska","duration_s":3.0,
        "video":{"index":0,"codec":"h264","width":320,"height":180,"rotation":0},"audio":[{"index":1,"codec":"aac","channels":2}],
        "subtitles":[],"tags":{"encoder":"x"}},"mime":"video/x-matroska",
        "suggestion":{"category":"movie","title":"clip final v2","year":null,"show_name":null,"season":null,
        "episode":null,"episode_title":null,"taken_at":null,"confidence":"low","reason":"Not sure this is a movie.",
        "place":"library","library_id":"0192a000-0000-7000-8000-000000000001",
        "destination":"Movies/clip final v2/clip final v2.mkv","waiting":"low_confidence"},
        "source_download_id":"d-1","next_episode_id":null,"previous_episode_id":null}"""

    const val DOWNLOAD = """{"id":"d-1","kind":"http","name":"holiday-clip.mp4","source":"http://203.0.113.10/e2e/holiday-clip.mp4",
        "state":"downloading","progress":0.42,"total_bytes":1000,"done_bytes":420,"down_bps":2100000,"up_bps":0,
        "peers":null,"eta_s":12,"error":null,"created_at":1760400500,"completed_at":null,"item_ids":[],"destinations":[]}"""

    const val SORTED = """{"id":"d-2","kind":"torrent","name":"Harbor.Lights.2019","source":"magnet:?xt=urn:btih:abc",
        "state":"sorted","progress":1.0,"total_bytes":5000,"done_bytes":5000,"down_bps":0,"up_bps":0,"peers":0,
        "eta_s":null,"error":null,"created_at":1760400000,"completed_at":1760400100,"item_ids":["i-harbor"],
        "destinations":[{"place":"library","library_id":"0192a000-0000-7000-8000-000000000001","count":1}]}"""

    const val VPN = """{"configured":true,"reachable":true,"state":"connected","provider":"nordvpn","protocol":"wireguard",
        "label":"NordVPN","server":"nl123.nordvpn.com","country":"Netherlands","public_ip":"203.0.113.5",
        "protected":true,"since":1760400000,"error":null,"kill_switch":true,"allow_no_vpn":false,"torrents_off":false}"""

    fun home() = """{"continue_watching":[$ITEM],"libraries":[
        {"library":$MOVIES,"recent":[$ITEM],"show_posters":{}},
        {"library":$SHOWS,"recent":[$EPISODE],"show_posters":{"The Lantern Keepers":"i-lantern-1"}}],
        "other_files_count":2,"inbox_count":1,"total_bytes":382044}"""
}

fun MockWebServer.respond(body: String, code: Int = 200, vararg headers: Pair<String, String>) {
    val builder = MockResponse.Builder().code(code).body(body)
    if (body.isNotEmpty()) builder.addHeader("Content-Type", "application/json")
    headers.forEach { (name, value) -> builder.addHeader(name, value) }
    enqueue(builder.build())
}

fun MockWebServer.respondEmpty(code: Int = 204) = enqueue(MockResponse.Builder().code(code).build())

fun MockWebServer.next(): RecordedRequest = takeRequest(5, TimeUnit.SECONDS) ?: error("No request reached the server")

val RecordedRequest.path: String get() = url.encodedPath

val RecordedRequest.bodyText: String get() = body?.utf8() ?: ""

/**
 * Waits up to five real seconds. Inside runTest, a plain withTimeout runs on virtual time and
 * would give up at once while a real HTTP answer is still on its way.
 */
suspend fun <T> withinRealSeconds(block: suspend () -> T): T =
    kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.Default) { kotlinx.coroutines.withTimeout(5_000) { block() } }
