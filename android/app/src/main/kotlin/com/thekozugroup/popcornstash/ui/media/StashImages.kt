package com.thekozugroup.popcornstash.ui.media

import android.content.Context
import androidx.compose.foundation.layout.Box
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import coil3.ImageLoader
import coil3.compose.AsyncImage
import coil3.compose.AsyncImagePainter
import coil3.disk.DiskCache
import coil3.memory.MemoryCache
import coil3.network.okhttp.OkHttpNetworkFetcherFactory
import coil3.request.ImageRequest
import coil3.request.crossfade
import coil3.size.Precision
import coil3.size.Size
import com.thekozugroup.popcornstash.data.api.Item
import com.thekozugroup.popcornstash.ui.theme.Popcorn
import com.thekozugroup.popcornstash.ui.theme.PopcornMotion
import okhttp3.HttpUrl
import okhttp3.HttpUrl.Companion.toHttpUrl
import okio.Path.Companion.toOkioPath
import java.io.File

/**
 * Where the session's pictures come from (docs/architecture.md section 8): thumbnails
 * (`sm`, 400 px on the long edge, or `lg`, 1280 px) and the browser-friendly photo (`view`).
 * Every address is on the signed-in server, so the session client adds the bearer token.
 */
class Pictures(private val server: HttpUrl) {
    /** The item's thumbnail, or null while it has none; [large] for frames, posters and the detail page. */
    fun thumb(item: Item, large: Boolean = false): String? {
        val path = item.thumb ?: return null
        val url = server.resolve(path) ?: return null
        return (if (large) url.newBuilder().setQueryParameter("size", "lg").build() else url).toString()
    }

    /** The thumbnail of the item with [id] when only its id is known (a show's poster). */
    fun thumbOf(id: String, large: Boolean = false): String =
        server.newBuilder().addPathSegments("api/items").addPathSegment(id).addPathSegment("thumb")
            .addQueryParameter("size", if (large) "lg" else "sm").build().toString()

    /** The photo as a phone can show it (the original, or a rendition of HEIC, RAW and TIFF). */
    fun view(id: String): String =
        server.newBuilder().addPathSegments("api/items").addPathSegment(id).addPathSegment("view").build().toString()

    companion object {
        /** For previews and tests: addresses that go nowhere. */
        val Placeholder = Pictures("https://stash.invalid/".toHttpUrl())
    }
}

/** The session's [Pictures]. */
val LocalPictures = staticCompositionLocalOf { Pictures.Placeholder }

/** The session's image loader ([sessionImageLoader]); null where nothing may be fetched (previews). */
val LocalStashImages = staticCompositionLocalOf<ImageLoader?> { null }

/** The folder that holds the session's pictures on disk; deleted when the session ends. */
fun imageCacheDir(context: Context): File = File(context.cacheDir, "images")

/**
 * Coil for one session: it fetches with the session's own client ([http], which adds the
 * bearer token for the server and nowhere else), keeps pictures in memory and in
 * [imageCacheDir], and is dropped with its caches when the session ends
 * (docs/architecture.md section 10, Leaving a person's data behind).
 */
fun sessionImageLoader(context: Context, http: okhttp3.OkHttpClient): ImageLoader =
    ImageLoader.Builder(context)
        .components { add(OkHttpNetworkFetcherFactory(callFactory = { http })) }
        .memoryCache { MemoryCache.Builder().maxSizePercent(context, 0.2).build() }
        .diskCache {
            DiskCache.Builder()
                .directory(imageCacheDir(context).toOkioPath())
                .maxSizeBytes(256L * 1024 * 1024)
                .build()
        }
        .build()

/**
 * A picture from the server. [placeholder] shows while there is no address, while it
 * loads and when it fails; the picture fades in over it (no fade under reduce motion).
 * [size] asks for a bigger decode than the layout (the zoomable photo).
 */
@Composable
fun StashImage(
    url: String?,
    contentDescription: String?,
    modifier: Modifier = Modifier,
    contentScale: ContentScale = ContentScale.Crop,
    size: Size? = null,
    onLoaded: () -> Unit = {},
    placeholder: @Composable () -> Unit = {},
) {
    val loader = LocalStashImages.current
    val context = LocalContext.current
    val reduceMotion = Popcorn.reduceMotion
    var shown by remember(url) { mutableStateOf(false) }
    Box(modifier) {
        if (!shown) placeholder()
        if (url != null && loader != null) {
            val request = remember(url, reduceMotion, size) {
                ImageRequest.Builder(context)
                    .data(url)
                    .crossfade(if (reduceMotion) 0 else PopcornMotion.FAST_MS)
                    .apply { if (size != null) size(size).precision(Precision.INEXACT) }
                    .build()
            }
            AsyncImage(
                model = request,
                imageLoader = loader,
                contentDescription = contentDescription,
                contentScale = contentScale,
                onState = { state ->
                    val success = state is AsyncImagePainter.State.Success
                    if (success && !shown) onLoaded()
                    shown = success
                },
                modifier = Modifier.matchParentSize(),
            )
        }
    }
}
