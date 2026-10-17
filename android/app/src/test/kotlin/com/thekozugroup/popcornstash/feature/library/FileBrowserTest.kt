package com.thekozugroup.popcornstash.feature.library

import androidx.activity.ComponentActivity
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.ui.Modifier
import com.thekozugroup.popcornstash.ui.theme.Popcorn
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.github.takahirom.roborazzi.captureRoboImage
import com.thekozugroup.popcornstash.data.api.*
import com.thekozugroup.popcornstash.data.repo.StashData
import com.thekozugroup.popcornstash.navigation.ViewModelStores
import com.thekozugroup.popcornstash.testing.json
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.emptyFlow
import mockwebserver3.*
import org.junit.*
import org.junit.Assert.assertTrue
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import java.util.concurrent.CopyOnWriteArrayList

/** Native selection, partial failures and scopes, over the real HTTP client. */
@RunWith(AndroidJUnit4::class)
@Config(qualifiers = "w411dp-h891dp-xxhdpi")
class FileBrowserTest {
    @get:Rule val compose = createAndroidComposeRule<ComponentActivity>()
    @get:Rule val temporary = TemporaryFolder()
    private val server = MockWebServer()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val paths = CopyOnWriteArrayList<String>()
    private val initial = listOf(
        Item(id = "one", title = "Harbor One", relPath = "Movies/Harbor One.mkv", kind = MediaKind.Video),
        Item(id = "two", title = "Harbor Two", relPath = "Movies/Harbor Two.mkv", kind = MediaKind.Video),
    )
    @Volatile private var files = initial
    private lateinit var services: LibraryServices

    @Before fun setup() {
        server.dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse {
                paths += "${request.method} ${request.target}"
                val target = request.target.substringBefore('?')
                val body = when {
                    target.endsWith("/move") -> """{"completed":["one"],"failed":[{"id":"two","message":"A file already uses this name."}]}"""
                    target == "/api/inbox/trash" -> {
                        files = files.filterNot { it.id == "one" }
                        """{"completed":["one"],"failed":[{"id":"two","message":"This file left Inbox."}]}"""
                    }
                    target.endsWith("/folders") && request.method == "POST" -> return MockResponse.Builder().code(201).build()
                    target.endsWith("/folders") -> """{"folder":"","folders":["Archive"]}"""
                    target == "/api/items" -> json(ItemPage.serializer(), ItemPage(files))
                    target == "/api/inbox" -> """{"items":[],"ignored":[]}"""
                    target == "/api/libraries" -> """{"libraries":[]}"""
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

    private fun show(organize: Boolean = false, inbox: Boolean = false, dark: Boolean = false, owner: Boolean = true) {
        compose.setContent {
            PopcornTheme(darkTheme = dark, reduceMotion = true) {
                Box(Modifier.fillMaxSize().background(Popcorn.colors.page)) {
                    FileBrowserScreen(services, 96.dp, owner, {}, if (organize) "movies" else null, "Movies", organize, inbox)
                }
            }
        }
        compose.waitUntil(5_000) { compose.onAllNodesWithText("Harbor One").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test fun bulkMoveKeepsFailedFilesSelected() {
        show(organize = true)
        compose.onNodeWithText("Select shown (2)").performClick()
        compose.onNodeWithText("Move selected").performClick()
        compose.onNodeWithText("Use this folder").performClick()
        compose.onNodeWithText("Move files").performClick()
        compose.waitUntil(5_000) { compose.onAllNodesWithText("1 files did not move. A file already uses this name.").fetchSemanticsNodes().isNotEmpty() }
        compose.waitUntil(5_000) { compose.onAllNodesWithText("Harbor Two").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithText("1 selected").assertExists()
        compose.onNodeWithText("Harbor Two").assertIsOn()
        assertTrue(paths.any { it == "POST /api/libraries/movies/move" })
    }

    @Test fun inboxDeleteRemovesOnlySuccessAndRetainsFailedSelection() {
        show(inbox = true)
        compose.onNodeWithText("Select shown (2)").performClick()
        compose.onNodeWithText("Delete selected").performClick()
        compose.onNodeWithText("Move to Trash").performClick()
        compose.waitUntil(5_000) { compose.onAllNodesWithText("Harbor One").fetchSemanticsNodes().isEmpty() }
        compose.onNodeWithText("Harbor Two").assertIsOn()
        compose.onNodeWithText("1 selected").assertExists()
    }

    @Test fun membersBrowseWithoutMovingOrDeleting() {
        show(organize = true, owner = false)
        compose.onNodeWithText("Archive").assertExists()
        compose.onNodeWithText("Create subfolder").assertDoesNotExist()
        compose.onNodeWithText("Select shown (2)").assertDoesNotExist()
    }

    @Test fun organizationLight() { show(organize = true); compose.onRoot().captureRoboImage("src/test/screenshots/files-organize-light.png") }
    @Test fun inboxDark() { show(inbox = true, dark = true); compose.onRoot().captureRoboImage("src/test/screenshots/files-inbox-dark.png") }
    @Test fun searchLight() { show(); compose.onRoot().captureRoboImage("src/test/screenshots/files-search-light.png") }
}
