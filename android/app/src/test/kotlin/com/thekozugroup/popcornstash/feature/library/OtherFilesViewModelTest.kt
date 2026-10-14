package com.thekozugroup.popcornstash.feature.library

import androidx.test.ext.junit.runners.AndroidJUnit4
import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.ItemPage
import com.thekozugroup.popcornstash.data.api.StashEvent
import com.thekozugroup.popcornstash.data.api.baseHttpClient
import com.thekozugroup.popcornstash.data.repo.Loadable
import com.thekozugroup.popcornstash.testing.MainDispatcherRule
import com.thekozugroup.popcornstash.testing.keep
import com.thekozugroup.popcornstash.testing.SampleMedia
import com.thekozugroup.popcornstash.testing.awaitUntil
import com.thekozugroup.popcornstash.testing.json
import com.thekozugroup.popcornstash.testing.next
import com.thekozugroup.popcornstash.testing.respond
import com.thekozugroup.popcornstash.testing.withinRealSeconds
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.emptyFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.test.runTest
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import okio.Buffer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith
import java.io.File

/** Other files: the list, its order, and opening a file through a private copy. */
@RunWith(AndroidJUnit4::class)
class OtherFilesViewModelTest {
    @get:Rule
    val main = MainDispatcherRule()

    @get:Rule
    val folder = TemporaryFolder()

    private val server = MockWebServer()
    private val events = MutableSharedFlow<StashEvent>(extraBufferCapacity = 16)
    private lateinit var api: ApiClient
    private lateinit var http: okhttp3.OkHttpClient
    private lateinit var opened: File

    @Before
    fun setUp() {
        server.start()
        http = baseHttpClient("t")
        api = ApiClient(server.url("/"), http)
        opened = File(folder.root, "opened")
    }

    @After
    fun tearDown() = server.close()

    private fun model() = main.keep { OtherFilesViewModel(api, events, emptyFlow(), FileFetcher(http), opened, settleMillis = 0) }

    @Test
    fun listsOtherFilesNewestFirstAndSortsByName() = runTest {
        server.respond(json(ItemPage.serializer(), ItemPage(SampleMedia.otherFiles)))
        val model = model()
        model.ui.awaitUntil { it.files is Loadable.Ready }
        assertEquals("/api/items?place=other_files&sort=added&order=desc&limit=60", server.next().target)
        server.respond(json(ItemPage.serializer(), ItemPage(SampleMedia.otherFiles.sortedBy { it.title })))
        model.setSort(ItemSort.Name)
        model.ui.awaitUntil { it.sort == ItemSort.Name && it.files is Loadable.Ready }
        assertEquals("/api/items?place=other_files&sort=title&order=asc&limit=60", server.next().target)
    }

    @Test
    fun anEventWithoutALibraryLoadsTheListAgain() = runTest {
        server.respond(json(ItemPage.serializer(), ItemPage(SampleMedia.otherFiles)))
        val model = model()
        model.ui.awaitUntil { it.files is Loadable.Ready }
        server.respond(json(ItemPage.serializer(), ItemPage(SampleMedia.otherFiles.drop(1))))
        events.emit(StashEvent.LibraryChanged("removed", "o-1", null))
        model.ui.awaitUntil { it.files.valueOrNull?.items?.size == 3 }
    }

    @Test
    fun anEventPastTheServersLargestPageLoadsEveryFileShownAgain() = runTest {
        val all = List(230) { SampleMedia.otherFiles[0].copy(id = "o-$it") }
        server.respond(json(ItemPage.serializer(), ItemPage(all.take(60), "c2")))
        val model = model()
        model.ui.awaitUntil { it.files is Loadable.Ready }
        server.respond(json(ItemPage.serializer(), ItemPage(all.subList(60, 230), "c3")))
        model.loadMore()
        model.ui.awaitUntil { it.files.valueOrNull?.items?.size == 230 }
        server.next()
        server.next()

        server.respond(json(ItemPage.serializer(), ItemPage(all.take(200), "r2")))
        server.respond(json(ItemPage.serializer(), ItemPage(all.subList(200, 230), "r3")))
        events.emit(StashEvent.LibraryChanged("updated", "o-4", null))
        val ui = model.ui.awaitUntil { it.files.valueOrNull?.nextCursor == "r3" }
        assertEquals("/api/items?place=other_files&sort=added&order=desc&limit=200", server.next().target)
        assertEquals("/api/items?place=other_files&sort=added&order=desc&cursor=r2&limit=30", server.next().target)
        assertEquals(all.map { it.id }, ui.files.valueOrNull?.items?.map { it.id })
    }

    @Test
    fun openingFetchesAPrivateCopyWithTheSessionClient() = runTest {
        server.respond(json(ItemPage.serializer(), ItemPage(SampleMedia.otherFiles)))
        val model = model()
        model.ui.awaitUntil { it.files is Loadable.Ready }
        server.next()
        server.enqueue(MockResponse.Builder().code(200).body(Buffer().writeUtf8("%PDF-1.7 permission slip")).build())
        val announced = backgroundScope.async(Dispatchers.Unconfined) { withinRealSeconds { model.opened.first() } }
        model.open(SampleMedia.otherFiles[0])
        assertEquals("o-1", model.ui.value.opening)
        val file = announced.await()
        assertEquals("/api/items/o-1/file", server.next().target)
        assertEquals("School trip permission.pdf", file.name)
        assertEquals("application/pdf", file.mime)
        assertEquals("%PDF-1.7 permission slip", file.file.readText())
        assertEquals(File(File(opened, "o-1"), "School trip permission.pdf"), file.file)
        assertFalse(File(file.file.parentFile, "School trip permission.pdf.part").exists())
        assertNull(model.ui.awaitUntil { it.opening == null }.opening)
    }

    @Test
    fun aFileThatIsGoneSaysSoAndLeavesNothingBehind() = runTest {
        server.respond(json(ItemPage.serializer(), ItemPage(SampleMedia.otherFiles)))
        val model = model()
        model.ui.awaitUntil { it.files is Loadable.Ready }
        server.respond("""{"error":{"code":"not_found","message":"That item no longer exists."}}""", 404)
        server.respond(json(ItemPage.serializer(), ItemPage(SampleMedia.otherFiles.drop(1))))
        val message = backgroundScope.async(Dispatchers.Unconfined) { withinRealSeconds { model.messages.first() } }
        model.open(SampleMedia.otherFiles[0])
        assertEquals("That file is not available any more.", message.await())
        assertTrue(opened.walk().none { it.isFile })
    }
}
