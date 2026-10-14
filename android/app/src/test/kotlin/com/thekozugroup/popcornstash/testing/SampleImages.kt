package com.thekozugroup.popcornstash.testing

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.LinearGradient
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RadialGradient
import android.graphics.Shader
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.remember
import androidx.compose.ui.platform.LocalContext
import coil3.ImageLoader
import coil3.asImage
import coil3.test.FakeImageLoaderEngine
import com.thekozugroup.popcornstash.ui.media.LocalPictures
import com.thekozugroup.popcornstash.ui.media.LocalStashImages
import com.thekozugroup.popcornstash.ui.media.Pictures
import kotlinx.coroutines.Dispatchers
import okhttp3.HttpUrl.Companion.toHttpUrl
import kotlin.math.absoluteValue
import kotlin.math.sin

/**
 * Pictures for screenshots, painted here instead of fetched: each item id gets its own
 * landscape (sky, sun, hills) in one of a few palettes, so posters, frames and photos read
 * as different pictures without shipping any image files.
 */
object SampleImages {
    private data class Palette(val skyTop: Int, val skyBottom: Int, val sun: Int, val far: Int, val near: Int)

    private val palettes = listOf(
        Palette(0xFF1E3A5F.toInt(), 0xFFF29E6D.toInt(), 0xFFFFE3B3.toInt(), 0xFF6B4E71.toInt(), 0xFF2E2440.toInt()),
        Palette(0xFF8EC5E8.toInt(), 0xFFE3F1F8.toInt(), 0xFFFFF6D6.toInt(), 0xFF5E8C6A.toInt(), 0xFF2F5D3A.toInt()),
        Palette(0xFF0F2027.toInt(), 0xFF2C5364.toInt(), 0xFFE6F0F3.toInt(), 0xFF1B3B47.toInt(), 0xFF0B1D24.toInt()),
        Palette(0xFFF6D365.toInt(), 0xFFFDA085.toInt(), 0xFFFFFBEA.toInt(), 0xFFC0703F.toInt(), 0xFF7A3E25.toInt()),
        Palette(0xFF3A6073.toInt(), 0xFF8FB8C9.toInt(), 0xFFF4F1E8.toInt(), 0xFF4A6B57.toInt(), 0xFF223B2C.toInt()),
        Palette(0xFF2B1B3D.toInt(), 0xFFB65C7C.toInt(), 0xFFFFD0A8.toInt(), 0xFF5C3A5E.toInt(), 0xFF1F1426.toInt()),
        Palette(0xFFB7D3DF.toInt(), 0xFFF2E8DA.toInt(), 0xFFFFFFFF.toInt(), 0xFF9AA88F.toInt(), 0xFF5B6B50.toInt()),
    )

    /** A picture [width] by [height] for [seed]. */
    fun paint(seed: String, width: Int, height: Int): Bitmap {
        val hash = seed.hashCode().absoluteValue
        val p = palettes[hash % palettes.size]
        val bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(bitmap)
        val w = width.toFloat()
        val h = height.toFloat()
        val paint = Paint(Paint.ANTI_ALIAS_FLAG)
        paint.shader = LinearGradient(0f, 0f, 0f, h * 0.75f, p.skyTop, p.skyBottom, Shader.TileMode.CLAMP)
        canvas.drawRect(0f, 0f, w, h, paint)

        val sunX = w * (0.2f + (hash % 60) / 100f)
        val sunY = h * (0.3f + (hash % 17) / 100f)
        val radius = minOf(w, h) * 0.11f
        paint.shader = RadialGradient(sunX, sunY, radius * 3.2f, (p.sun and 0x00FFFFFF) or 0x66000000, 0x00FFFFFF and p.sun, Shader.TileMode.CLAMP)
        canvas.drawCircle(sunX, sunY, radius * 3.2f, paint)
        paint.shader = null
        paint.color = p.sun
        canvas.drawCircle(sunX, sunY, radius, paint)

        fun hills(base: Float, amplitude: Float, waves: Float, phase: Float, color: Int) {
            val path = Path()
            path.moveTo(0f, h)
            var x = 0f
            while (x <= w) {
                val y = base + amplitude * sin((x / w) * waves * Math.PI.toFloat() + phase)
                path.lineTo(x, y)
                x += w / 60f
            }
            path.lineTo(w, h)
            path.close()
            paint.color = color
            canvas.drawPath(path, paint)
        }
        hills(h * 0.62f, h * 0.05f, 2.2f + hash % 3, (hash % 7).toFloat(), p.far)
        hills(h * 0.76f, h * 0.06f, 1.4f + hash % 2, (hash % 5).toFloat() + 1.3f, p.near)
        return bitmap
    }

    /** An image loader that answers every item picture with its painted picture; any other address gets nothing. */
    fun loader(context: Context): ImageLoader {
        val engine = FakeImageLoaderEngine.Builder()
            .intercept(
                { it is String && it.contains("/api/items/") },
                { chain ->
                    val url = chain.request.data as String
                    val id = url.substringAfter("/api/items/").substringBefore('/')
                    val large = url.contains("size=lg") || url.contains("/view")
                    val (w, h) = if (large) 640 to 360 else 400 to 300
                    coil3.request.SuccessResult(paint(id, w, h).asImage(), chain.request)
                },
            )
            .build()
        return ImageLoader.Builder(context)
            .components { add(engine) }
            .coroutineContext(Dispatchers.Unconfined)
            .build()
    }
}

/** Gives [content] the painted pictures, as a signed-in session gives it the server's. */
@Composable
fun WithSampleImages(content: @Composable () -> Unit) {
    val context = LocalContext.current
    val loader = remember { SampleImages.loader(context) }
    CompositionLocalProvider(
        LocalStashImages provides loader,
        LocalPictures provides Pictures("https://stash.example.com/".toHttpUrl()),
        content = content,
    )
}
