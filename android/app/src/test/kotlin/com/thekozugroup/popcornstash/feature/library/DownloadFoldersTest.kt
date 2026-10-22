package com.thekozugroup.popcornstash.feature.library

import androidx.activity.ComponentActivity
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.github.takahirom.roborazzi.captureRoboImage
import com.thekozugroup.popcornstash.data.api.*
import com.thekozugroup.popcornstash.data.repo.StashData
import com.thekozugroup.popcornstash.navigation.ViewModelStores
import com.thekozugroup.popcornstash.testing.json
import com.thekozugroup.popcornstash.ui.theme.Popcorn
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.emptyFlow
import kotlinx.serialization.builtins.ListSerializer
import mockwebserver3.*
import org.junit.*
import org.junit.Assert.*
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import java.util.concurrent.CopyOnWriteArrayList

@RunWith(AndroidJUnit4::class)
@Config(qualifiers = "w411dp-h891dp-xxhdpi")
class DownloadFoldersTest {
    @get:Rule val compose = createAndroidComposeRule<ComponentActivity>()
    @get:Rule val temporary = TemporaryFolder()
    private val server = MockWebServer()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val paths = CopyOnWriteArrayList<String>()
    private val writes = CopyOnWriteArrayList<String>()
    @Volatile private var groups = listOf(DownloadFolder("download:one", "Harbor collection", 2, 900_000_000, 1), DownloadFolder("custom:archive", "Archive", 0, 0, 0, true))
    private val videos = listOf(Item(id = "one", title = "Harbor One", relPath = "Movies/Original/deep/a.mkv", kind = MediaKind.Video), Item(id = "two", title = "Harbor Two", relPath = "Movies/Other/b.mkv", kind = MediaKind.Video))
    @Volatile private var failMove = false
    @Volatile private var blockFlatten = false
    private lateinit var services: LibraryServices
    @Before fun setup() {
        server.dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse {
                paths += "${request.method} ${request.target}"
                val target = request.target.substringBefore('?')
                if (request.method == "POST") writes += request.body!!.utf8()
                val body = when {
                    target.endsWith("/flatten-preview") -> """{"key":"download:one","folder":"Movies/Harbor collection","files":[{"item_id":"one","from":"Movies/Original/deep/a.mkv","to":"Movies/Harbor collection/a.mkv","kind":"video","fingerprint":"f"}],"blocked":${if (blockFlatten) "[\"A video is converting. Try again after it finishes.\"]" else "[]"},"token":"confirmed-preview"}"""
                    target.endsWith("/flatten") -> """{"completed":["one"],"failed":[]}"""
                    target.endsWith("/download-folders/move") -> return MockResponse.Builder().code(if (failMove) 409 else 204).body(if (failMove) """{"error":{"code":"conflict","message":"A video moved. Refresh and select it again."}}""" else "").build()
                    target.endsWith("/download-folders") && request.method == "POST" -> { groups = groups + DownloadFolder("custom:new", "My collection", 0, 0, 0, true); """{"key":"custom:new"}""" }
                    target.endsWith("/download-folders") -> json(ListSerializer(DownloadFolder.serializer()), groups)
                    target == "/api/items" -> json(ItemPage.serializer(), ItemPage(if (request.target.contains("custom%3Anew")) emptyList() else videos))
                    else -> "{}"
                }
                return MockResponse.Builder().code(200).addHeader("Content-Type", "application/json").body(body).build()
            }
        }
        server.start()
        val api = ApiClient(server.url("/"), baseHttpClient("test"))
        services = LibraryServices(api, StashData(api, emptyFlow(), scope), emptyFlow(), emptyFlow(), FileFetcher(baseHttpClient("test")), temporary.newFolder("opened"), ViewModelStores())
    }
    @After fun cleanup() { scope.cancel(); server.close() }
    private fun waitFor(text: String) = compose.waitUntil(5_000) { compose.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty() }
    private fun show(dark: Boolean = false, owner: Boolean = true) {
        compose.setContent { PopcornTheme(darkTheme = dark, reduceMotion = true) { Box(Modifier.fillMaxSize().background(Popcorn.colors.page)) {
            DownloadFoldersScreen("movies", "Movies", services, owner, 96.dp, {}, {}, {}, {})
        } } }
        waitFor("Harbor collection")
    }
    private fun open() { compose.onNodeWithText("Harbor collection").performClick(); waitFor("Harbor One") }
    @Test fun flatSourceContentsUseTheScopedPagedQuery() {
        show(); open()
        compose.onNodeWithText("Harbor Two").assertExists()
        assertTrue(paths.any { it.startsWith("GET /api/items?") && it.contains("library_id=movies") && it.contains("download_group=download%3Aone") })
        assertTrue(paths.none { it.contains("/folders?") })
    }
    @Test fun bulkMoveSendsExactSelectedIdsAndFailureKeepsSelection() {
        failMove = true
        show(); open()
        compose.onNodeWithText("Select videos").performClick()
        compose.onNodeWithText("Select shown").performClick()
        compose.onNodeWithText("Move to folder").performClick()
        compose.onNodeWithText("Archive").performClick()
        compose.onNodeWithText("Move selected").performClick()
        waitFor("A video moved. Refresh and select it again.")
        compose.onNodeWithText("2 selected").assertExists()
        assertTrue(writes.single().contains("\"item_ids\":[\"one\",\"two\"]"))
        assertTrue(writes.single().contains("custom:archive"))
    }
    @Test fun createFolderOpensTheNewEmptyCollection() {
        show()
        compose.onNodeWithText("New folder").performClick()
        compose.onNodeWithText("Folder name").performTextInput("My collection")
        compose.onNodeWithText("Save folder").performClick()
        waitFor("My collection")
        assertTrue(writes.single().contains("My collection"))
        assertTrue(writes.single().contains("name"))
    }
    @Test fun membersCanBrowseButCannotEdit() {
        show(owner = false); open()
        compose.onNodeWithText("New folder").assertDoesNotExist()
        compose.onNodeWithText("Rename folder").assertDoesNotExist()
        compose.onNodeWithText("Select videos").assertDoesNotExist()
        compose.onNodeWithText("Flatten files").assertDoesNotExist()
    }
    @Test fun flattenRequiresPreviewAndExplicitConfirmation() {
        show(); open()
        compose.onNodeWithText("Flatten files").performClick()
        waitFor("Move files")
        assertTrue(paths.none { it.endsWith("/flatten") })
        compose.onNodeWithText("Destination: Movies/Harbor collection").assertExists()
        compose.onNodeWithText("Move files").performClick()
        waitFor("1 video organized.")
        assertTrue(writes.last().contains("confirmed-preview"))
        assertTrue(paths.any { it.endsWith("/flatten") })
    }
    @Test fun blockedFlattenDisablesConfirmation() {
        blockFlatten = true; show(); open()
        compose.onNodeWithText("Flatten files").performClick(); waitFor("Move files")
        compose.onNodeWithText("Move files").assertIsNotEnabled()
        assertTrue(paths.none { it.endsWith("/flatten") })
    }
    @Test fun flattenPreviewDark() { show(dark = true); open(); compose.onNodeWithText("Flatten files").performClick(); waitFor("Move files"); compose.onRoot().captureRoboImage("src/test/screenshots/download-folder-flatten-dark.png") }
    @Test fun flattenPreviewLight() { show(); open(); compose.onNodeWithText("Flatten files").performClick(); waitFor("Move files"); compose.onRoot().captureRoboImage("src/test/screenshots/download-folder-flatten-light.png") }
    @Test fun foldersLight() { show(); compose.onRoot().captureRoboImage("src/test/screenshots/download-folders-light.png") }
    @Test fun foldersDark() { show(dark = true); compose.onRoot().captureRoboImage("src/test/screenshots/download-folders-dark.png") }
    @Test fun contentsLight() { show(); open(); compose.onRoot().captureRoboImage("src/test/screenshots/download-folder-videos-light.png") }
    @Test fun contentsDark() { show(dark = true); open(); compose.onRoot().captureRoboImage("src/test/screenshots/download-folder-videos-dark.png") }
}
