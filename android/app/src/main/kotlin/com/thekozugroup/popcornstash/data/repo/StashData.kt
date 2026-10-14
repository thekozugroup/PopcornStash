package com.thekozugroup.popcornstash.data.repo

import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.ApiException
import com.thekozugroup.popcornstash.data.api.Download
import com.thekozugroup.popcornstash.data.api.Home
import com.thekozugroup.popcornstash.data.api.Inbox
import com.thekozugroup.popcornstash.data.api.Library
import com.thekozugroup.popcornstash.data.api.StashEvent
import com.thekozugroup.popcornstash.data.api.VpnStatus
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** Something loaded from the server, with what the page showed before while it reloads. */
sealed interface Loadable<out T> {
    data object Idle : Loadable<Nothing>

    data object Loading : Loadable<Nothing>

    data class Ready<T>(val value: T, val refreshing: Boolean = false) : Loadable<T>

    /** [message] can be shown as it is; [previous] is what was there before, if anything. */
    data class Failed<T>(val message: String, val previous: T? = null) : Loadable<T>

    val valueOrNull: T?
        get() = when (this) {
            is Ready -> value
            is Failed -> previous
            else -> null
        }
}

/**
 * The data the tabs show for one session, from the API and kept current by the live events
 * ([events]): Home, the library list, downloads, the Inbox, the VPN line and the count of
 * files ready to import. Features read the flows and call `refresh...` when a page opens or
 * the person asks; events update what is loaded without a page doing anything.
 */
class StashData(
    private val api: ApiClient,
    events: Flow<StashEvent>,
    private val scope: CoroutineScope,
    /** How long a burst of item events is gathered before Home or the Inbox reloads once. */
    private val settleMillis: Long = 400,
) {
    private val _home = MutableStateFlow<Loadable<Home>>(Loadable.Idle)
    val home: StateFlow<Loadable<Home>> = _home.asStateFlow()

    private val _libraries = MutableStateFlow<Loadable<List<Library>>>(Loadable.Idle)
    val libraries: StateFlow<Loadable<List<Library>>> = _libraries.asStateFlow()

    private val _downloads = MutableStateFlow<Loadable<List<Download>>>(Loadable.Idle)
    val downloads: StateFlow<Loadable<List<Download>>> = _downloads.asStateFlow()

    private val _inbox = MutableStateFlow<Loadable<Inbox>>(Loadable.Idle)
    val inbox: StateFlow<Loadable<Inbox>> = _inbox.asStateFlow()

    private val _vpn = MutableStateFlow<VpnStatus?>(null)
    val vpn: StateFlow<VpnStatus?> = _vpn.asStateFlow()

    private val _readyToImport = MutableStateFlow(0L)

    /** Waiting files this person can see: the badge on the Downloads square. */
    val readyToImport: StateFlow<Long> = _readyToImport.asStateFlow()

    private var homeReload: Job? = null
    private var inboxReload: Job? = null

    init {
        scope.launch { events.collect { apply(it) } }
    }

    fun refreshHome(): Job = scope.launch {
        load(_home) { api.home() }?.let { _readyToImport.value = it.inboxCount }
    }

    fun refreshLibraries(): Job = scope.launch { load(_libraries) { api.libraries() } }

    fun refreshDownloads(): Job = scope.launch { load(_downloads) { api.downloads("all") } }

    fun refreshInbox(): Job = scope.launch {
        load(_inbox) { api.inbox() }?.let { _readyToImport.value = it.items.size.toLong() }
    }

    fun refreshVpn(): Job = scope.launch {
        try {
            _vpn.value = api.vpnStatus()
        } catch (_: ApiException) {
            // The VPN line keeps what it showed; the next `vpn` event brings it up to date.
        }
    }

    /** Applies one live event to what is loaded. */
    internal fun apply(event: StashEvent) {
        when (event) {
            is StashEvent.DownloadChanged -> _downloads.updateValue { list -> upsert(list, event.download) }
            is StashEvent.DownloadRemoved -> _downloads.updateValue { list -> list.filterNot { it.id == event.id } }
            is StashEvent.LibraryChanged -> {
                reloadHomeSoon()
                reloadInboxSoon()
            }
            StashEvent.LibrariesChanged -> {
                if (_libraries.value !is Loadable.Idle) refreshLibraries()
                reloadHomeSoon()
            }
            is StashEvent.VpnChanged -> _vpn.value = event.status
            is StashEvent.InboxCount -> {
                _readyToImport.value = event.count
                reloadInboxSoon()
            }
            is StashEvent.JobChanged -> Unit
        }
    }

    private fun reloadHomeSoon() {
        if (_home.value is Loadable.Idle) return
        homeReload?.cancel()
        homeReload = scope.launch {
            delay(settleMillis)
            refreshHome()
        }
    }

    private fun reloadInboxSoon() {
        if (_inbox.value is Loadable.Idle) return
        inboxReload?.cancel()
        inboxReload = scope.launch {
            delay(settleMillis)
            refreshInbox()
        }
    }

    private suspend fun <T> load(state: MutableStateFlow<Loadable<T>>, fetch: suspend () -> T): T? {
        val before = state.value.valueOrNull
        state.value = if (before != null) Loadable.Ready(before, refreshing = true) else Loadable.Loading
        return try {
            fetch().also { state.value = Loadable.Ready(it) }
        } catch (e: ApiException) {
            state.value = Loadable.Failed(e.message, before)
            null
        }
    }

    private fun <T> MutableStateFlow<Loadable<T>>.updateValue(change: (T) -> T) = update { current ->
        when (current) {
            is Loadable.Ready -> current.copy(value = change(current.value))
            is Loadable.Failed -> current.copy(previous = current.previous?.let(change))
            else -> current
        }
    }

    private companion object {
        /** Replaces the download with the same id, or adds a new one at the top. */
        fun upsert(list: List<Download>, download: Download): List<Download> {
            val index = list.indexOfFirst { it.id == download.id }
            return if (index >= 0) list.toMutableList().also { it[index] = download } else listOf(download) + list
        }
    }
}
