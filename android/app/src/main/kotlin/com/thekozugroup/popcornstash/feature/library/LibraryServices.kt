package com.thekozugroup.popcornstash.feature.library

import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.ApiException
import com.thekozugroup.popcornstash.data.api.EventStream
import com.thekozugroup.popcornstash.data.api.StashEvent
import com.thekozugroup.popcornstash.data.api.StashJson
import com.thekozugroup.popcornstash.data.api.await
import com.thekozugroup.popcornstash.data.api.errorFrom
import com.thekozugroup.popcornstash.data.repo.StashData
import com.thekozugroup.popcornstash.navigation.ViewModelStores
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.emptyFlow
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.transformLatest
import kotlinx.coroutines.withContext
import okhttp3.HttpUrl
import okhttp3.OkHttpClient
import okhttp3.Request
import java.io.File
import java.io.IOException
import java.io.OutputStream

/**
 * What Home and the library pages need from the signed-in session: the API, the data the
 * tabs share, the live events, a way to fetch files with the session's token, and the view
 * model stores of the pages.
 */
class LibraryServices(
    val api: ApiClient,
    val data: StashData,
    /** The live events, already filtered for this person by the server. */
    val events: Flow<StashEvent>,
    /** One signal each time the events stream is back after a gap (a lost connection, the app in the background). */
    val reconnects: Flow<Unit>,
    val files: FileFetcher,
    /** Where private copies of opened Other files go (`cacheDir/opened`, docs/architecture.md section 10). */
    val openedDir: File,
    val pages: ViewModelStores,
    /** True while the events stream has been away for a while; the page says it is trying again. */
    val connectionLost: Flow<Boolean> = emptyFlow(),
    val format: MediaFormat = MediaFormat(),
)

/**
 * One signal each time the events stream connects again after it was away (it failed and is
 * retrying, or the app was in the background): events sent meanwhile were missed, so the
 * pages load again.
 */
fun Flow<EventStream.Status>.reconnects(): Flow<Unit> = flow {
    var away = false
    collect { status ->
        when (status) {
            EventStream.Status.Waiting, EventStream.Status.Stopped -> away = true
            EventStream.Status.Connected -> if (away) {
                away = false
                emit(Unit)
            }
            EventStream.Status.Connecting -> Unit
        }
    }
}

/**
 * True once the events stream has been away for [graceMillis] without a break (failing and
 * retrying, not merely reconnecting for a moment); false as soon as it is back. The app in
 * the background does not count.
 */
@OptIn(ExperimentalCoroutinesApi::class)
fun Flow<EventStream.Status>.connectionLost(graceMillis: Long = 3_000): Flow<Boolean> =
    map { it == EventStream.Status.Connected || it == EventStream.Status.Stopped }
        .distinctUntilChanged()
        .transformLatest { connected ->
            if (connected) {
                emit(false)
            } else {
                delay(graceMillis)
                emit(true)
            }
        }

/**
 * Fetches a file with the session's own client, so the token never leaves the app
 * (docs/architecture.md section 10): never DownloadManager, never another app.
 */
class FileFetcher(private val http: OkHttpClient) {
    /** Saves [url] to [target] through a temporary file next to it; a failure leaves nothing behind. */
    suspend fun download(url: HttpUrl, target: File) = withContext(Dispatchers.IO) {
        target.parentFile?.mkdirs()
        val partial = File(target.parentFile, target.name + ".part")
        try {
            partial.outputStream().use { out -> copy(url, out) }
            if (!partial.renameTo(target)) throw IOException("Could not keep the file.")
        } catch (e: IOException) {
            partial.delete()
            throw ApiException.network()
        } finally {
            partial.delete()
        }
    }

    /** Copies [url] into [out] (a place the person chose); the caller closes [out]. */
    suspend fun copy(url: HttpUrl, out: OutputStream) {
        val response = try {
            http.newCall(Request.Builder().url(url).get().build()).await()
        } catch (_: IOException) {
            throw ApiException.network()
        }
        response.use {
            if (!it.isSuccessful) {
                val text = withContext(Dispatchers.IO) { runCatching { it.body.string() }.getOrDefault("") }
                throw errorFrom(it.code, text, StashJson)
            }
            withContext(Dispatchers.IO) {
                try {
                    it.body.byteStream().use { input -> input.copyTo(out) }
                } catch (_: IOException) {
                    throw ApiException.network()
                }
            }
        }
    }
}
