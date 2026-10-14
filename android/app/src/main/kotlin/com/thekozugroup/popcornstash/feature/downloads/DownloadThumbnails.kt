package com.thekozugroup.popcornstash.feature.downloads

import android.graphics.BitmapFactory
import android.util.LruCache
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.MediaKind
import com.thekozugroup.popcornstash.data.api.await
import com.thekozugroup.popcornstash.ui.icons.LucideGlyph
import com.thekozugroup.popcornstash.ui.icons.LucideGlyphs
import com.thekozugroup.popcornstash.ui.icons.LucideIcon
import com.thekozugroup.popcornstash.ui.theme.Popcorn
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.OkHttpClient
import okhttp3.Request
import java.io.IOException

/**
 * Small pictures for the Ready to import rows: the server's `sm` thumbnail, fetched with the
 * session's client (so the token goes only to the server) and kept in memory for as long as
 * the session lasts. Nothing is written to disk.
 */
class SessionThumbnails(private val http: OkHttpClient, private val api: ApiClient) : ThumbnailLoader {
    private val cache = LruCache<String, ImageBitmap>(48)

    override suspend fun load(path: String): ImageBitmap? {
        cache.get(path)?.let { return it }
        return withContext(Dispatchers.IO) {
            val url = runCatching { api.absolute(path) }.getOrNull() ?: return@withContext null
            try {
                http.newCall(Request.Builder().url(url).get().build()).await().use { response ->
                    if (!response.isSuccessful) return@use null
                    val body = response.body
                    if (body.contentLength() > MAX_BYTES) return@use null
                    val bytes = body.source().readByteArray().takeIf { it.size <= MAX_BYTES } ?: return@use null
                    val options = BitmapFactory.Options().apply { inSampleSize = 2 }
                    BitmapFactory.decodeByteArray(bytes, 0, bytes.size, options)?.asImageBitmap()
                }
            } catch (_: IOException) {
                null
            }
        }?.also { cache.put(path, it) }
    }

    private companion object {
        const val MAX_BYTES = 2L * 1024 * 1024
    }
}

/** Loads an item's thumbnail from its server-relative URL; null when there is none. */
fun interface ThumbnailLoader {
    suspend fun load(path: String): ImageBitmap?
}

/** The session's thumbnails; null in previews and tests without pictures, where tiles show their glyph. */
val LocalDownloadThumbnails = staticCompositionLocalOf<ThumbnailLoader?> { null }

/** The glyph that stands for a kind of file when it has no picture. */
fun glyphForKind(kind: MediaKind): LucideGlyph = when (kind) {
    MediaKind.Video -> LucideGlyphs.Film
    MediaKind.Photo -> LucideGlyphs.Image
    MediaKind.Audio -> LucideGlyphs.Music
    else -> LucideGlyphs.FileText
}

/**
 * A square media tile (android/DESIGN.md, Tiles): the picture cropped to fill, or the
 * file's glyph on `subtle`, with media corners and the hairline inside the edge.
 */
@Composable
fun MediaThumb(path: String?, glyph: LucideGlyph, size: Dp, modifier: Modifier = Modifier) {
    val loader = LocalDownloadThumbnails.current
    val image by produceState<ImageBitmap?>(null, path, loader) {
        value = if (path != null && loader != null) loader.load(path) else null
    }
    val shape = Popcorn.shapes.media
    Box(
        modifier
            .size(size)
            .clip(shape)
            .background(Popcorn.colors.subtle)
            .clearAndSetSemantics { },
        contentAlignment = Alignment.Center,
    ) {
        val picture = image
        if (picture != null) {
            Image(picture, contentDescription = null, contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize())
        } else {
            LucideIcon(glyph, contentDescription = null, tint = Popcorn.colors.textMuted, size = Popcorn.media.placeholderIcon)
        }
        // The hairline sits over the picture, so a dark frame keeps its edge on a dark page.
        Box(Modifier.matchParentSize().border(1.dp, Popcorn.colors.mediaHairline, shape))
    }
}
