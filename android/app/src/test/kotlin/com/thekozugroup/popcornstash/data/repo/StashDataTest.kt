package com.thekozugroup.popcornstash.data.repo

import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.Download
import com.thekozugroup.popcornstash.data.api.DownloadState
import com.thekozugroup.popcornstash.data.api.StashEvent
import com.thekozugroup.popcornstash.data.api.StashJson
import com.thekozugroup.popcornstash.data.api.VpnState
import com.thekozugroup.popcornstash.data.api.VpnStatus
import com.thekozugroup.popcornstash.data.api.baseHttpClient
import com.thekozugroup.popcornstash.testing.Fixtures
import com.thekozugroup.popcornstash.testing.next
import com.thekozugroup.popcornstash.testing.path
import com.thekozugroup.popcornstash.testing.respond
import com.thekozugroup.popcornstash.testing.withinRealSeconds
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runTest
import mockwebserver3.MockWebServer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import java.util.concurrent.TimeUnit

/** What the tabs read, loaded from the API and kept current by live events. */
@OptIn(ExperimentalCoroutinesApi::class)
class StashDataTest {
    private val server = MockWebServer()
    private val events = MutableSharedFlow<StashEvent>(extraBufferCapacity = 16)
    private lateinit var api: ApiClient

    @Before
    fun setUp() {
        server.start()
        api = ApiClient(server.url("/"), baseHttpClient("t"))
    }

    @After
    fun tearDown() = server.close()

    private fun TestScope.data() = StashData(api, events, CoroutineScope(UnconfinedTestDispatcher(testScheduler)), settleMillis = 400)

    private fun download(json: String): Download = StashJson.decodeFromString(Download.serializer(), json)

    @Test
    fun homeLoadsAndSetsTheReadyToImportCount() = runTest {
        val data = data()
        server.respond(Fixtures.home())
        data.refreshHome().join()
        val home = (data.home.value as Loadable.Ready).value
        assertEquals(3, home.libraries.size)
        assertEquals(1L, data.readyToImport.value)
    }

    @Test
    fun aFailedLoadKeepsWhatWasShownAndSaysWhy() = runTest {
        val data = data()
        server.respond(Fixtures.home())
        data.refreshHome().join()
        server.respond("""{"error":{"code":"internal","message":"PopcornStash could not read its database."}}""", code = 500)
        data.refreshHome().join()
        val failed = data.home.value as Loadable.Failed
        assertEquals("PopcornStash could not read its database.", failed.message)
        assertEquals(3, failed.previous!!.libraries.size)
    }

    @Test
    fun downloadEventsUpdateAddAndDropRows() = runTest {
        val data = data()
        server.respond("""{"items":[${Fixtures.DOWNLOAD},${Fixtures.SORTED}]}""")
        data.refreshDownloads().join()
        assertEquals("all", server.next().url.queryParameter("state"))

        events.emit(StashEvent.DownloadChanged(download(Fixtures.DOWNLOAD.replace("0.42", "0.9"))))
        events.emit(StashEvent.DownloadChanged(download(Fixtures.DOWNLOAD.replace("\"d-1\"", "\"d-3\""))))
        events.emit(StashEvent.DownloadRemoved("d-2"))
        val list = (data.downloads.value as Loadable.Ready).value
        assertEquals(listOf("d-3", "d-1"), list.map { it.id })
        assertEquals(0.9, list[1].progress, 0.0)
        assertEquals(DownloadState.Downloading, list[1].state)
    }

    @Test
    fun theVpnLineFollowsVpnEvents() = runTest {
        val data = data()
        server.respond(Fixtures.VPN)
        data.refreshVpn().join()
        assertEquals(VpnState.Connected, data.vpn.value!!.state)
        events.emit(StashEvent.VpnChanged(VpnStatus(configured = true, state = VpnState.Connecting)))
        assertEquals(VpnState.Connecting, data.vpn.value!!.state)
    }

    @Test
    fun aBurstOfItemEventsReloadsHomeOnceAfterItSettles() = runTest {
        val data = data()
        server.respond(Fixtures.home())
        data.refreshHome().join()
        server.next()
        server.respond(Fixtures.home())
        repeat(5) { events.emit(StashEvent.LibraryChanged("added", "i-$it", "lib")) }
        advanceTimeBy(300)
        assertEquals(1, server.requestCount)
        advanceTimeBy(200)
        assertEquals("/api/home", server.takeRequest(5, TimeUnit.SECONDS)!!.path)
        withinRealSeconds { data.home.first { it is Loadable.Ready && !it.refreshing } }
        assertEquals(2, server.requestCount)
    }

    @Test
    fun nothingReloadsForAPageNobodyOpened() = runTest {
        data()
        events.emit(StashEvent.LibraryChanged("added", "i-1", "lib"))
        events.emit(StashEvent.LibrariesChanged)
        advanceTimeBy(1_000)
        assertEquals(0, server.requestCount)
    }

    @Test
    fun inboxEventsSetTheBadgeAndReloadTheOpenInbox() = runTest {
        val data = data()
        server.respond("""{"items":[${Fixtures.WAITING}],"ignored":[]}""")
        data.refreshInbox().join()
        assertEquals(1L, data.readyToImport.value)
        server.next()
        server.respond("""{"items":[],"ignored":[]}""")
        events.emit(StashEvent.InboxCount(0))
        assertEquals(0L, data.readyToImport.value)
        advanceTimeBy(500)
        assertEquals("/api/inbox", server.takeRequest(5, TimeUnit.SECONDS)!!.path)
    }

    @Test
    fun aLibrariesEventReloadsTheLibraryList() = runTest {
        val data = data()
        server.respond("""{"libraries":[${Fixtures.MOVIES}]}""")
        data.refreshLibraries().join()
        server.next()
        server.respond("""{"libraries":[${Fixtures.MOVIES},${Fixtures.SHOWS}]}""")
        events.emit(StashEvent.LibrariesChanged)
        assertEquals("/api/libraries", server.next().path)
        withinRealSeconds { data.libraries.first { (it as? Loadable.Ready)?.value?.size == 2 } }
        assertTrue(true)
    }
}
