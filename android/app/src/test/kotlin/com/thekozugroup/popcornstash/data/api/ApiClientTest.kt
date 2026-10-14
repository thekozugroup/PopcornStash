package com.thekozugroup.popcornstash.data.api

import com.thekozugroup.popcornstash.feature.library.capturedAt
import com.thekozugroup.popcornstash.testing.Fixtures
import com.thekozugroup.popcornstash.testing.bodyText
import com.thekozugroup.popcornstash.testing.next
import com.thekozugroup.popcornstash.testing.path
import com.thekozugroup.popcornstash.testing.respond
import com.thekozugroup.popcornstash.testing.respondEmpty
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import mockwebserver3.SocketEffect
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.RequestBody.Companion.toRequestBody
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Before
import org.junit.Test

/** Every call the app makes, against MockWebServer: method, path, query, body, token, and how the answer reads. */
class ApiClientTest {
    private val server = MockWebServer()
    private lateinit var api: ApiClient
    private lateinit var publicApi: ApiClient
    private var unauthorized = 0

    @Before
    fun setUp() {
        server.start()
        val base = server.url("/")
        val http = baseHttpClient("PopcornStash-Android/test")
        api = ApiClient(base, http.newBuilder().addInterceptor(AuthInterceptor(base, { Fixtures.TOKEN }, { unauthorized++ })).build())
        publicApi = ApiClient(base, http)
    }

    @After
    fun tearDown() = server.close()

    private fun json(text: String): JsonObject = Json.parseToJsonElement(text).jsonObject

    private fun assertAuthorized(request: mockwebserver3.RecordedRequest) =
        assertEquals("Bearer ${Fixtures.TOKEN}", request.headers["Authorization"])

    // ------------------------------------------------------------ public ----

    @Test
    fun health() = runTest {
        server.respond("""{"status":"ok","version":"0.2.0","source":"https://github.com/thekozugroup/PopcornStash"}""")
        val health = publicApi.health()
        assertEquals("ok", health.status)
        assertEquals("0.2.0", health.version)
        val request = server.next()
        assertEquals("GET /api/health", "${request.method} ${request.path}")
        assertNull("sign-in calls carry no token", request.headers["Authorization"])
        assertEquals("PopcornStash-Android/test", request.headers["User-Agent"])
    }

    @Test
    fun authState() = runTest {
        server.respond(
            """{"setup_required":false,"authenticated":false,"user":null,"public_url":"https://stash.example.com",
            "rp_id":"stash.example.com","passkeys_ready":true,"reason":null}""",
        )
        val state = publicApi.authState()
        assertTrue(state.passkeysReady)
        assertFalse(state.setupRequired)
        assertEquals("stash.example.com", state.rpId)
        assertEquals("/api/auth/state", server.next().path)
    }

    @Test
    fun assetLinksListingTheAppOffersPasskeys() = runTest {
        server.respond(
            """[{"relation":["delegate_permission/common.handle_all_urls","delegate_permission/common.get_login_creds"],
            "target":{"namespace":"android_app","package_name":"com.thekozugroup.popcornstash",
            "sha256_cert_fingerprints":["AB:CD"]}}]""",
        )
        assertTrue(publicApi.listsAppForPasskeys("com.thekozugroup.popcornstash"))
        assertEquals("/.well-known/assetlinks.json", server.next().path)
    }

    @Test
    fun assetLinksForAnotherAppOrMissingDoNotOfferPasskeys() = runTest {
        server.respond(
            """[{"relation":["delegate_permission/common.get_login_creds"],
            "target":{"namespace":"android_app","package_name":"com.example.other"}}]""",
        )
        assertFalse(publicApi.listsAppForPasskeys("com.thekozugroup.popcornstash"))
        server.respond("""{"error":{"code":"not_found","message":"Not found."}}""", code = 404)
        assertFalse(publicApi.listsAppForPasskeys("com.thekozugroup.popcornstash"))
        server.respond("<html>nope</html>")
        assertFalse(publicApi.listsAppForPasskeys("com.thekozugroup.popcornstash"))
        assertFalse(assetLinksAllow("""[{"relation":["delegate_permission/common.handle_all_urls"],
            "target":{"namespace":"android_app","package_name":"com.thekozugroup.popcornstash"}}]""", "com.thekozugroup.popcornstash"))
    }

    @Test
    fun requestPairing() = runTest {
        server.respond(
            """{"pairing_id":"p-1","user_code":"K7QX-M2PD","poll_secret":"secret","expires_at":1760400600,
            "verify_url":"https://stash.example.com/pair?code=K7QX-M2PD","interval":6}""",
        )
        val started = publicApi.requestPairing("Ana's Pixel")
        assertEquals("K7QX-M2PD", started.userCode)
        assertEquals(6, started.interval)
        assertFalse("the poll secret is never printed", started.toString().contains("secret"))
        val request = server.next()
        assertEquals("POST /api/pairing/request", "${request.method} ${request.path}")
        assertEquals(json("""{"client_name":"Ana's Pixel","client_kind":"android"}"""), json(request.bodyText))
        assertEquals("application/json; charset=utf-8", request.headers["Content-Type"])
    }

    @Test
    fun pollPairingReadsPendingApprovedDeniedAndExpired() = runTest {
        server.respond("""{"status":"pending"}""", code = 202)
        server.respond("""{"status":"approved","token":"${Fixtures.TOKEN}","scopes":["full"],"server_name":"stash.example.com"}""")
        server.respond("""{"status":"denied"}""", code = 410)
        server.respond("""{"status":"expired"}""", code = 410)
        assertEquals("pending", publicApi.pollPairing("p-1", "secret").status)
        val approved = publicApi.pollPairing("p-1", "secret")
        assertEquals(Fixtures.TOKEN, approved.token)
        assertEquals(listOf("full"), approved.scopes)
        assertFalse(approved.toString().contains(Fixtures.TOKEN))
        assertEquals("denied", publicApi.pollPairing("p-1", "secret").status)
        assertEquals("expired", publicApi.pollPairing("p-1", "secret").status)
        val request = server.next()
        assertEquals("POST /api/pairing/poll", "${request.method} ${request.path}")
        assertEquals(json("""{"pairing_id":"p-1","poll_secret":"secret"}"""), json(request.bodyText))
    }

    @Test
    fun pollPairingOfAnUnknownRequestIsNotFound() = runTest {
        server.respond(
            """{"error":{"code":"not_found","message":"That pairing request does not exist. Start pairing again."}}""",
            code = 404,
        )
        val error = expectApiError { publicApi.pollPairing("gone", "secret") }
        assertTrue(error.isNotFound)
        assertEquals("That pairing request does not exist. Start pairing again.", error.message)
    }

    @Test
    fun passkeyLoginAsksForAToken() = runTest {
        server.respond("""{"flow_id":"f-1","options":{"publicKey":{"challenge":"abc","rpId":"stash.example.com","userVerification":"required"},"mediation":null}}""")
        server.respond("""{"user":${Fixtures.OWNER},"token":"${Fixtures.TOKEN}"}""")
        val started = publicApi.startLogin()
        assertEquals("f-1", started.flowId)
        assertEquals("abc", started.options["publicKey"]!!.jsonObject["challenge"]!!.jsonPrimitive.content)
        val credential = json("""{"id":"cred","rawId":"cred","type":"public-key","response":{"clientDataJSON":"x"}}""")
        val finished = publicApi.finishLogin(LoginFinishRequest("f-1", credential, wantToken = true, deviceName = "Pixel 9"))
        assertEquals(Fixtures.TOKEN, finished.token)
        assertEquals(Role.Owner, finished.user.role)
        assertFalse(finished.toString().contains(Fixtures.TOKEN))
        assertEquals("/api/auth/login/start", server.next().path)
        val finish = server.next()
        assertEquals("/api/auth/login/finish", finish.path)
        assertEquals(
            json("""{"flow_id":"f-1","credential":{"id":"cred","rawId":"cred","type":"public-key","response":{"clientDataJSON":"x"}},
            "want_token":true,"device_name":"Pixel 9"}"""),
            json(finish.bodyText),
        )
    }

    // ----------------------------------------------------------- session ----

    @Test
    fun logoutMeAndUsers() = runTest {
        server.respondEmpty()
        server.respond(Fixtures.OWNER)
        server.respond("[${Fixtures.OWNER},${Fixtures.MEMBER}]")
        api.logout()
        val me = api.me()
        val users = api.users()
        assertEquals("Ana Ribeiro", me.displayName)
        assertEquals(listOf(Role.Owner, Role.Member), users.map { it.role })
        val logout = server.next()
        assertEquals("POST /api/auth/logout", "${logout.method} ${logout.path}")
        assertAuthorized(logout)
        assertEquals("/api/me", server.next().path)
        assertEquals("/api/users", server.next().path)
    }

    // -------------------------------------------------- home and libraries ----

    @Test
    fun home() = runTest {
        server.respond(Fixtures.home())
        val home = api.home()
        assertEquals(listOf("Movies", "Shows", "Photos"), home.libraries.map { it.library.name })
        assertEquals("i-paper", home.continueWatching.single().id)
        assertEquals(2.5, home.continueWatching.single().progress!!.positionS, 0.0)
        assertEquals("i-lantern-1", home.libraries[1].showPosters["The Lantern Keepers"])
        assertEquals(1L, home.inboxCount)
        assertEquals(Layout.Series, home.libraries[1].library.layout)
        // A series shelf comes as shows; the other libraries send an empty list.
        val show = home.libraries[1].shows!!.single()
        assertEquals("The Lantern Keepers", show.showName)
        assertEquals("i-lantern-1", show.posterItemId)
        assertEquals(3L, show.itemCount)
        assertEquals(1L, show.newCount)
        assertEquals("i-lantern-1", show.latest.id)
        assertEquals(1_760_400_300L, show.lastAddedAt)
        assertEquals(emptyList<HomeShow>(), home.libraries[0].shows)
        // A capture time: the moment, the camera's clock and the offset the file wrote.
        val photo = home.libraries[2].recent.single()
        assertEquals(1_719_802_800L, photo.takenAt)
        assertEquals("2024-06-30T20:00:00", photo.takenLocal)
        assertEquals(-420, photo.takenOffset)
        assertEquals(java.time.LocalDateTime.of(2024, 6, 30, 20, 0), photo.capturedAt)
        val request = server.next()
        assertEquals("GET /api/home", "${request.method} ${request.path}")
        assertAuthorized(request)
    }

    @Test
    fun librariesReadOwnerAndMemberShapesAndUnknownKinds() = runTest {
        server.respond("""{"libraries":[${Fixtures.MOVIES},${Fixtures.SHOWS},${Fixtures.PHOTOS}]}""")
        val libraries = api.libraries()
        assertEquals(listOf("film", "tv", "image"), libraries.map { it.icon })
        assertEquals(Visibility.Everyone, libraries[0].visibility)
        assertEquals(FolderState.Ok, libraries[0].folderState)
        assertNull("members get no folder", libraries[1].folder)
        assertEquals(Structure.Episodes, libraries[1].structure)
        assertEquals(ContentType.Photos, libraries[2].contentType)
        assertEquals(listOf(Category.Photo, Category.Unknown), libraries[2].claims)
        assertEquals("/api/libraries", server.next().path)
    }

    @Test
    fun oneLibraryIconsAndFolders() = runTest {
        server.respond(Fixtures.MOVIES)
        server.respond("""{"icons":["film","clapperboard","tv"]}""")
        server.respond(
            """{"folders":[{"name":"Family 2019","item_count":1,"video_count":1,"photo_count":0,"other_count":0,
            "suggested":{"content_type":"video","structure":"one_offs"}}]}""",
        )
        assertEquals("Movies", api.library("0192a000-0000-7000-8000-000000000001").name)
        assertEquals(listOf("film", "clapperboard", "tv"), api.libraryIcons())
        val folder = api.unassignedFolders().single()
        assertEquals("Family 2019", folder.name)
        assertEquals(Structure.OneOffs, folder.suggested.structure)
        assertEquals("/api/libraries/0192a000-0000-7000-8000-000000000001", server.next().path)
        assertEquals("/api/libraries/icons", server.next().path)
        assertEquals("/api/libraries/folders", server.next().path)
    }

    @Test
    fun createLibrary() = runTest {
        server.respond(Fixtures.MOVIES, code = 201)
        api.createLibrary(
            LibraryCreate(
                name = "Kids",
                icon = "baby",
                contentType = ContentType.Video,
                structure = Structure.Episodes,
                claims = listOf(Category.Episode),
                folder = FolderChoice("new", "Kids"),
                visibility = Visibility.Selected,
                memberIds = listOf("u-kid"),
            ),
        )
        val request = server.next()
        assertEquals("POST /api/libraries", "${request.method} ${request.path}")
        assertEquals(
            json(
                """{"name":"Kids","icon":"baby","content_type":"video","structure":"episodes","claims":["episode"],
                "folder":{"mode":"new","name":"Kids"},"visibility":"selected","member_ids":["u-kid"]}""",
            ),
            json(request.bodyText),
        )
    }

    @Test
    fun editLibrarySendsOnlyWhatChangedAndAnExplicitNullStructure() = runTest {
        server.respond(Fixtures.PHOTOS)
        server.respond(Fixtures.MOVIES)
        api.editLibrary("lib-1", LibraryChanges(contentType = ContentType.Photos, structureSet = true, claims = listOf(Category.Photo)))
        api.editLibrary("lib-2", LibraryChanges(name = "Films", icon = "clapperboard", visibility = Visibility.Everyone, memberIds = emptyList()))
        val first = server.next()
        assertEquals("PATCH /api/libraries/lib-1", "${first.method} ${first.path}")
        assertEquals(json("""{"content_type":"photos","structure":null,"claims":["photo"]}"""), json(first.bodyText))
        val second = server.next()
        assertEquals(json("""{"name":"Films","icon":"clapperboard","visibility":"everyone","member_ids":[]}"""), json(second.bodyText))
    }

    @Test
    fun reorderAndDeleteLibraries() = runTest {
        server.respond("""{"libraries":[${Fixtures.SHOWS},${Fixtures.MOVIES}]}""")
        server.respondEmpty()
        assertEquals(listOf("Shows", "Movies"), api.reorderLibraries(listOf("b", "a")).map { it.name })
        api.deleteLibrary("a")
        val order = server.next()
        assertEquals("PUT /api/libraries/order", "${order.method} ${order.path}")
        assertEquals(json("""{"ids":["b","a"]}"""), json(order.bodyText))
        val delete = server.next()
        assertEquals("DELETE /api/libraries/a", "${delete.method} ${delete.path}")
    }

    @Test
    fun showsShowAndTimeline() = runTest {
        server.respond(
            """[{"show_name":"The Lantern Keepers","item_count":3,"season_count":2,"poster_item_id":"i-lantern-1","last_added_at":1760400300}]""",
        )
        server.respond("""{"show_name":"Who / What?","poster_item_id":null,"seasons":[{"season":1,"episodes":[${Fixtures.EPISODE}]}]}""")
        server.respond(
            """{"months":[{"month":"2022-05","count":3,"items":[]}],"next_cursor":"c2"}""",
        )
        assertEquals(2L, api.shows("lib-2").single().seasonCount)
        assertEquals(1, api.show("lib-2", "Who / What?").seasons.single().season)
        val timeline = api.timeline("lib-4", cursor = "c1", limit = 200)
        assertEquals("2022-05", timeline.months.single().month)
        assertEquals("c2", timeline.nextCursor)
        assertEquals("/api/libraries/lib-2/shows", server.next().path)
        assertEquals("/api/libraries/lib-2/shows/Who%20%2F%20What%3F", server.next().path)
        val request = server.next()
        assertEquals("/api/libraries/lib-4/timeline", request.path)
        assertEquals("c1", request.url.queryParameter("cursor"))
        assertEquals("200", request.url.queryParameter("limit"))
    }

    // -------------------------------------------------------------- items ----

    @Test
    fun itemsSendsOnlyTheFiltersGiven() = runTest {
        server.respond("""{"items":[${Fixtures.ITEM}],"next_cursor":"n1"}""")
        server.respond("""{"items":[],"next_cursor":null}""")
        val page = api.items(ItemQuery(libraryId = "lib-1", sort = "added", order = "desc", limit = 50))
        api.items(ItemQuery(place = Place.OtherFiles, category = Category.HomeVideo, favorite = true, q = "beach day", year = 2019, cursor = "n1"))
        assertEquals("n1", page.nextCursor)
        assertEquals(Place.Library, page.items.single().place)
        val first = server.next()
        assertEquals("/api/items", first.path)
        assertEquals(setOf("library_id", "sort", "order", "limit"), first.url.queryParameterNames)
        assertEquals("lib-1", first.url.queryParameter("library_id"))
        val second = server.next()
        assertEquals("other_files", second.url.queryParameter("place"))
        assertEquals("home_video", second.url.queryParameter("category"))
        assertEquals("true", second.url.queryParameter("favorite"))
        assertEquals("beach day", second.url.queryParameter("q"))
        assertEquals("2019", second.url.queryParameter("year"))
        assertEquals("n1", second.url.queryParameter("cursor"))
    }

    @Test
    fun itemDetailUpdateAndOrganize() = runTest {
        server.respond(Fixtures.WAITING)
        server.respond(Fixtures.WAITING)
        server.respond(Fixtures.WAITING)
        val detail = api.item("i-clip")
        assertEquals(Waiting.LowConfidence, detail.suggestion!!.waiting)
        assertEquals("Not sure this is a movie.", detail.suggestion.reason)
        assertEquals("aac", detail.media!!.audio.single().codec)
        assertEquals("i-clip", detail.item.id)
        api.updateItem("i-clip", ItemPatch(favorite = true))
        api.organize("i-clip", OrganizeRequest(place = Place.Library, libraryId = "lib-1", title = "Clip"))
        assertEquals("GET /api/items/i-clip", server.next().let { "${it.method} ${it.path}" })
        val patch = server.next()
        assertEquals("PATCH /api/items/i-clip", "${patch.method} ${patch.path}")
        assertEquals(json("""{"favorite":true}"""), json(patch.bodyText))
        val organize = server.next()
        assertEquals("POST /api/items/i-clip/organize", "${organize.method} ${organize.path}")
        assertEquals(json("""{"place":"library","library_id":"lib-1","title":"Clip"}"""), json(organize.bodyText))
    }

    @Test
    fun pictureAndFileAddresses() {
        assertEquals("/api/items/i-1/thumb?size=lg", api.thumbUrl("i-1", "lg").let { "${it.encodedPath}?${it.encodedQuery}" })
        assertEquals("/api/items/i-1/file?download=1", api.fileUrl("i-1", download = true).let { "${it.encodedPath}?${it.encodedQuery}" })
        assertEquals("/api/items/i-1/file", api.fileUrl("i-1").encodedPath)
        assertEquals("/api/items/i-1/view", api.viewUrl("i-1").encodedPath)
        assertEquals(server.url("/api/items/i-paper/thumb?v=3"), api.absolute("/api/items/i-paper/thumb?v=3"))
    }

    @Test
    fun playbackStreamAndProgress() = runTest {
        server.respond("""{"mode":"hls","url":"/api/stream/0123456789abcdef0123456789abcdef/index.m3u8","session_id":"0123456789abcdef0123456789abcdef"}""")
        server.respondEmpty()
        server.respondEmpty()
        val decision = api.playback("i-1", PlaybackRequest(listOf("h264", "hevc"), listOf("aac"), hdr = false, maxHeight = 1080, startS = 12.5))
        assertEquals("hls", decision.mode)
        assertEquals("0123456789abcdef0123456789abcdef", decision.sessionId)
        api.stopStream(decision.sessionId!!)
        api.saveProgress("i-1", 30.0, 120.0)
        val playback = server.next()
        assertEquals("POST /api/items/i-1/playback", "${playback.method} ${playback.path}")
        assertEquals(
            json("""{"video_codecs":["h264","hevc"],"audio_codecs":["aac"],"hdr":false,"max_height":1080,"start_s":12.5}"""),
            json(playback.bodyText),
        )
        val stop = server.next()
        assertEquals("DELETE /api/stream/0123456789abcdef0123456789abcdef", "${stop.method} ${stop.path}")
        val progress = server.next()
        assertEquals("PUT /api/items/i-1/progress", "${progress.method} ${progress.path}")
        assertEquals(json("""{"position_s":30.0,"duration_s":120.0}"""), json(progress.bodyText))
    }

    // -------------------------------------------------------------- inbox ----

    @Test
    fun inboxAcceptAndIgnore() = runTest {
        server.respond("""{"items":[${Fixtures.WAITING}],"ignored":[]}""")
        server.respond(Fixtures.WAITING)
        server.respond(Fixtures.WAITING)
        assertEquals("i-clip", api.inbox().items.single().id)
        api.inboxAction("i-clip", InboxAction("accept", place = Place.Library, libraryId = "lib-1", title = "Clip"))
        api.inboxAction("i-clip", InboxAction("ignore"))
        assertEquals("/api/inbox", server.next().path)
        val accept = server.next()
        assertEquals("POST /api/inbox/i-clip", "${accept.method} ${accept.path}")
        assertEquals(json("""{"action":"accept","place":"library","library_id":"lib-1","title":"Clip"}"""), json(accept.bodyText))
        assertEquals(json("""{"action":"ignore"}"""), json(server.next().bodyText))
    }

    // ---------------------------------------------------------- downloads ----

    @Test
    fun downloads() = runTest {
        server.respond("""{"items":[${Fixtures.DOWNLOAD},${Fixtures.SORTED}]}""")
        server.respond(Fixtures.DOWNLOAD)
        server.respond(Fixtures.DOWNLOAD.replace("downloading", "paused"))
        server.respond(Fixtures.DOWNLOAD)
        server.respondEmpty()
        val list = api.downloads("all")
        assertEquals(DownloadState.Sorted, list[1].state)
        assertEquals(Place.Library, list[1].destinations.single().place)
        api.addDownload("magnet:?xt=urn:btih:abc")
        assertEquals(DownloadState.Paused, api.pauseDownload("d-1").state)
        api.resumeDownload("d-1")
        api.removeDownload("d-1")
        val listRequest = server.next()
        assertEquals("/api/downloads", listRequest.path)
        assertEquals("all", listRequest.url.queryParameter("state"))
        val add = server.next()
        assertEquals("POST /api/downloads", "${add.method} ${add.path}")
        assertEquals(json("""{"url":"magnet:?xt=urn:btih:abc"}"""), json(add.bodyText))
        assertEquals("POST /api/downloads/d-1/pause", server.next().let { "${it.method} ${it.path}" })
        assertEquals("POST /api/downloads/d-1/resume", server.next().let { "${it.method} ${it.path}" })
        val remove = server.next()
        assertEquals("DELETE /api/downloads/d-1", "${remove.method} ${remove.path}")
        assertEquals("true", remove.url.queryParameter("delete_files"))
    }

    // ------------------------------------------------------------ uploads ----

    @Test
    fun resumableUpload() = runTest {
        server.respond("""{"id":"up-1","chunk_size":16777216,"received":0}""")
        server.respond("""{"received":5}""")
        server.respond("""{"error":{"code":"conflict","message":"The upload is at 10 bytes.","received":10},"received":10}""", code = 409)
        server.respond("""{"id":"up-1","filename":"beach.mp4","size":20,"received":10,"state":"receiving"}""")
        server.respond(Fixtures.DOWNLOAD.replace("\"http\"", "\"upload\""))
        server.respondEmpty()
        val started = api.startUpload(UploadCreate("beach.mp4", 20, "video/mp4", 1_700_000_000_000))
        assertEquals(16_777_216L, started.chunkSize)
        val octets = "application/octet-stream".toMediaType()
        assertEquals(5L, api.uploadChunk("up-1", 0, "hello".toByteArray().toRequestBody(octets)).received)
        // The server already holds more than this offset: carry on from what it has.
        assertEquals(10L, api.uploadChunk("up-1", 5, "world".toByteArray().toRequestBody(octets)).received)
        assertEquals(10L, api.uploadStatus("up-1").received)
        assertEquals(DownloadKind.Upload, api.completeUpload("up-1").kind)
        api.cancelUpload("up-1")
        val create = server.next()
        assertEquals("POST /api/uploads", "${create.method} ${create.path}")
        assertEquals(json("""{"filename":"beach.mp4","size":20,"mime":"video/mp4","last_modified":1700000000000}"""), json(create.bodyText))
        val chunk = server.next()
        assertEquals("PUT /api/uploads/up-1", "${chunk.method} ${chunk.path}")
        assertEquals("0", chunk.url.queryParameter("offset"))
        assertEquals("hello", chunk.bodyText)
        assertEquals("5", server.next().url.queryParameter("offset"))
        assertEquals("GET /api/uploads/up-1", server.next().let { "${it.method} ${it.path}" })
        assertEquals("POST /api/uploads/up-1/complete", server.next().let { "${it.method} ${it.path}" })
        assertEquals("DELETE /api/uploads/up-1", server.next().let { "${it.method} ${it.path}" })
    }

    // ---------------------------------------------------------------- VPN ----

    @Test
    fun vpnStatus() = runTest {
        server.respond(Fixtures.VPN)
        val vpn = api.vpnStatus()
        assertEquals(VpnState.Connected, vpn.state)
        assertEquals(true, vpn.protected)
        assertEquals("Netherlands", vpn.country)
        assertEquals("/api/vpn/status", server.next().path)
    }

    @Test
    fun eventsRequestAsksForAStream() {
        val request = api.eventsRequest()
        assertEquals("/api/events", request.url.encodedPath)
        assertEquals("text/event-stream", request.header("Accept"))
    }

    // ------------------------------------------------------------- errors ----

    @Test
    fun aConnectionThatBreaksWhileTheAnswerArrivesIsANetworkError() = runTest {
        server.enqueue(
            MockResponse.Builder()
                .addHeader("Content-Type", "application/json")
                .body("""{"libraries":[""" + "x".repeat(200_000))
                .onResponseBody(SocketEffect.ShutdownConnection)
                .build(),
        )
        val error = expectApiError { api.libraries() }
        assertTrue(error.isNetwork)
        assertEquals("PopcornStash did not answer. Check the connection and try again.", error.message)
    }

    @Test
    fun theServersSentenceAndFieldComeThrough() = runTest {
        server.respond(
            """{"error":{"code":"conflict","message":"A library called Kids already exists.","field":"name"}}""",
            code = 409,
        )
        val error = expectApiError { api.createLibrary(LibraryCreate("Kids", "baby", ContentType.Video, Structure.OneOffs, folder = FolderChoice("new", "Kids"))) }
        assertEquals(409, error.status)
        assertEquals("conflict", error.code)
        assertEquals("A library called Kids already exists.", error.message)
        assertEquals("name", error.field)
    }

    @Test
    fun answersWithoutTheErrorBodyGetAPlainSentence() = runTest {
        server.respond("<html>Bad gateway</html>", code = 502)
        val error = expectApiError { api.home() }
        assertEquals(502, error.status)
        assertEquals("PopcornStash ran into a problem. Try again in a moment.", error.message)
        server.respond("not json at all")
        assertEquals(ApiException.CODE_UNEXPECTED, expectApiError { api.home() }.code)
    }

    @Test
    fun aServerThatDoesNotAnswerIsANetworkError() = runTest {
        val gone = server.url("/")
        server.close()
        val error = expectApiError { ApiClient(gone, baseHttpClient("t")).health() }
        assertTrue(error.isNetwork)
        assertEquals("PopcornStash did not answer. Check the connection and try again.", error.message)
    }

    @Test
    fun a401SignsThePersonOut() = runTest {
        server.respond("""{"error":{"code":"unauthorized","message":"Sign in first."}}""", code = 401)
        val error = expectApiError { api.home() }
        assertTrue(error.isUnauthorized)
        assertEquals(1, unauthorized)
        // Without a token (sign-in), a 401 is just an answer.
        server.respond("""{"error":{"code":"unauthorized","message":"That passkey is not known here."}}""", code = 401)
        expectApiError { publicApi.finishLogin(LoginFinishRequest("f", Json.parseToJsonElement("{}"))) }
        assertEquals(1, unauthorized)
    }

    @Test
    fun theTokenNeverGoesToAnotherAddress() = runTest {
        MockWebServer().use { other ->
            other.start()
            other.respond(Fixtures.OWNER)
            ApiClient(other.url("/"), api.let { _ -> clientFor(server.url("/")) }).me()
            assertNull(other.next().headers["Authorization"])
        }
    }

    private fun clientFor(origin: okhttp3.HttpUrl) =
        baseHttpClient("t").newBuilder().addInterceptor(AuthInterceptor(origin, { Fixtures.TOKEN }, {})).build()

    private suspend fun expectApiError(block: suspend () -> Unit): ApiException {
        try {
            block()
        } catch (e: ApiException) {
            return e
        }
        fail("expected an ApiException")
        throw AssertionError()
    }
}
