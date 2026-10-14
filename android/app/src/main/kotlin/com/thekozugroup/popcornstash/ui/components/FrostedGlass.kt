package com.thekozugroup.popcornstash.ui.components

import android.app.UiModeManager
import android.graphics.ColorMatrixColorFilter
import android.graphics.RenderEffect
import android.graphics.RenderNode
import android.graphics.Shader
import android.os.Build
import androidx.annotation.RequiresApi
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.BlurEffect
import androidx.compose.ui.graphics.ClipOp
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.graphics.ColorMatrix
import androidx.compose.ui.graphics.Outline
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.TileMode
import androidx.compose.ui.graphics.addOutline
import androidx.compose.ui.graphics.asAndroidPath
import androidx.compose.ui.graphics.drawscope.ContentDrawScope
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.clipPath
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.layer.GraphicsLayer
import androidx.compose.ui.graphics.layer.drawLayer
import androidx.compose.ui.graphics.layer.setOutline
import androidx.compose.ui.graphics.nativeCanvas
import androidx.compose.ui.graphics.rememberGraphicsLayer
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.layout.LayoutCoordinates
import androidx.compose.ui.layout.onPlaced
import androidx.compose.ui.node.DrawModifierNode
import androidx.compose.ui.node.GlobalPositionAwareModifierNode
import androidx.compose.ui.node.ModifierNodeElement
import androidx.compose.ui.node.invalidateDraw
import androidx.compose.ui.node.requireGraphicsContext
import androidx.compose.ui.node.requireLayoutCoordinates
import androidx.compose.ui.platform.InspectorInfo
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.toIntSize
import kotlin.math.ceil
import kotlin.math.floor

/**
 * How the floating toolbar draws its glass on this phone (android/DESIGN.md, Floating toolbar).
 * The two glass tiers look the same and differ only in how the blur is made; every tier casts
 * the same shadow, outside the bar only.
 */
enum class GlassTier {
    /**
     * Android 17 QPR2 (SDK 37.2) and later: the platform blurs whatever the window drew behind
     * the bar (RenderNode.setBackdropRenderEffect). Nothing is recorded for it.
     */
    Backdrop,

    /**
     * Android 12 to 17.1: the tab pages are recorded into one [GraphicsLayer] each frame they
     * draw ([glassSource]), and the bar draws that recording, moved to its own place and blurred.
     */
    Captured,

    /**
     * Android 9 to 11, which have no RenderEffect, and any phone set to more contrast: the bar
     * keeps its opaque surface.
     */
    Opaque,
}

/** `Build.VERSION_CODES_FULL.CINNAMON_BUN_2`: Android 17 QPR2, the first with the backdrop effect. */
internal const val BACKDROP_SDK_INT_FULL = 3_700_002

/** Android 12, the first with RenderEffect. */
private const val RENDER_EFFECT_SDK_INT = 31

/**
 * The glass tier for a phone that reports [sdkInt] (`Build.VERSION.SDK_INT`) and [sdkIntFull]
 * (`Build.VERSION.SDK_INT_FULL`, the major and minor release; it exists from Android 16, so
 * pass 0 below it). [moreContrast] is the system's contrast setting above standard: the person
 * asked for stronger edges, so the bar is solid instead of see-through.
 */
fun glassTierFor(sdkInt: Int, sdkIntFull: Int, moreContrast: Boolean = false): GlassTier = when {
    moreContrast -> GlassTier.Opaque
    sdkInt >= Build.VERSION_CODES.CINNAMON_BUN && sdkIntFull >= BACKDROP_SDK_INT_FULL -> GlassTier.Backdrop
    sdkInt >= RENDER_EFFECT_SDK_INT -> GlassTier.Captured
    else -> GlassTier.Opaque
}

/** The glass tier for the phone the app runs on, from its release alone. */
fun deviceGlassTier(moreContrast: Boolean = false): GlassTier {
    val full = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.BAKLAVA) Build.VERSION.SDK_INT_FULL else 0
    return glassTierFor(Build.VERSION.SDK_INT, full, moreContrast)
}

/**
 * The glass tier for this phone as it is set now: its release, and whether the system's colour
 * contrast setting (Android 14 and later) is above standard. It follows a change of that setting
 * while the app is open.
 */
@Composable
fun rememberGlassTier(): GlassTier = deviceGlassTier(moreContrast = rememberMoreContrast())

/** True while the system contrast setting is medium or high (Android 14 and later). */
@Composable
private fun rememberMoreContrast(): Boolean {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.UPSIDE_DOWN_CAKE) return false
    val context = LocalContext.current
    val uiMode = remember(context) { context.getSystemService(UiModeManager::class.java) } ?: return false
    var contrast by remember(uiMode) { mutableFloatStateOf(uiMode.contrast) }
    DisposableEffect(uiMode) {
        val listener = UiModeManager.ContrastChangeListener { contrast = it }
        uiMode.addContrastChangeListener(context.mainExecutor, listener)
        contrast = uiMode.contrast
        onDispose { uiMode.removeContrastChangeListener(listener) }
    }
    return contrast > 0f
}

/**
 * The tab pages as the captured glass ([GlassTier.Captured]) sees them: one [GraphicsLayer]
 * that [glassSource] records each frame the pages draw, and the toolbar draws again blurred.
 */
@Stable
class GlassBackdrop internal constructor(internal val layer: GraphicsLayer) {
    /** Where the pages sit, so the bar can find the part of them under itself. */
    internal var coordinates: LayoutCoordinates? = null
}

/**
 * The recording the captured glass needs, or null when [tier] does not use one: the platform
 * blurs the window by itself on Android 17 QPR2 and later, and there is no blur before Android 12.
 */
@Composable
fun rememberGlassBackdrop(tier: GlassTier): GlassBackdrop? {
    if (tier != GlassTier.Captured) return null
    val layer = rememberGraphicsLayer()
    return remember(layer) { GlassBackdrop(layer) }
}

/**
 * Records what this draws into [backdrop] (once per frame it draws) and draws it as usual.
 * Without a [backdrop] it changes nothing.
 */
fun Modifier.glassSource(backdrop: GlassBackdrop?): Modifier =
    if (backdrop == null) {
        this
    } else {
        this
            .onPlaced { backdrop.coordinates = it }
            .drawWithContent {
                backdrop.layer.record { this@drawWithContent.drawContent() }
                drawLayer(backdrop.layer)
            }
    }

/** What the glass looks like: the tokens it is drawn from (android/DESIGN.md, Floating toolbar). */
@Immutable
data class GlassStyle(
    /** Laid over the blur: `surface` at the alpha that keeps the icons legible over any picture. */
    val tint: Color,
    /** The bar's fill where there is no blur (Android 9 to 11, or nothing recorded yet). */
    val opaque: Color,
    val blur: Dp,
    /** 1 keeps the colours behind as they are; above 1 makes them read through the tint. */
    val saturation: Float,
    val elevation: Dp,
    val shadow: Color,
)

/**
 * Draws a frosted glass bar in [shape] behind this element's content, as [tier] allows, and
 * its shadow outside [shape] only, so the shadow never darkens the glass. [backdrop] is the
 * recording [GlassTier.Captured] draws from; without it that tier draws the opaque bar.
 */
fun Modifier.frostedGlass(tier: GlassTier, backdrop: GlassBackdrop?, shape: Shape, style: GlassStyle): Modifier =
    this then FrostedGlassElement(tier, backdrop, shape, style)

private data class FrostedGlassElement(
    val tier: GlassTier,
    val backdrop: GlassBackdrop?,
    val shape: Shape,
    val style: GlassStyle,
) : ModifierNodeElement<FrostedGlassNode>() {
    override fun create() = FrostedGlassNode(tier, backdrop, shape, style)

    override fun update(node: FrostedGlassNode) {
        node.tier = tier
        node.backdrop = backdrop
        node.shape = shape
        node.style = style
    }

    override fun InspectorInfo.inspectableProperties() {
        name = "frostedGlass"
        properties["tier"] = tier
        properties["shape"] = shape
        properties["style"] = style
    }
}

private class FrostedGlassNode(
    var tier: GlassTier,
    var backdrop: GlassBackdrop?,
    var shape: Shape,
    var style: GlassStyle,
) : Modifier.Node(), DrawModifierNode, GlobalPositionAwareModifierNode {
    private var shadowLayer: GraphicsLayer? = null
    private var blurLayer: GraphicsLayer? = null
    private var platform: PlatformGlass? = null

    /** Where the bar was drawn over the pages, last time ([GlassTier.Captured]). */
    private var drawnAt = Offset.Unspecified

    override fun onDetach() {
        val graphics = requireGraphicsContext()
        shadowLayer?.let(graphics::releaseGraphicsLayer)
        blurLayer?.let(graphics::releaseGraphicsLayer)
        shadowLayer = null
        blurLayer = null
        if (Build.VERSION.SDK_INT >= RENDER_EFFECT_SDK_INT) platform?.release()
        platform = null
    }

    /**
     * The captured glass shows the pages under the bar's own place, so it is drawn again when
     * the bar moves over them (sliding in or out), not only when the bar itself changes.
     */
    override fun onGloballyPositioned(coordinates: LayoutCoordinates) {
        if (tier != GlassTier.Captured) return
        val source = backdrop?.coordinates ?: return
        if (!source.isAttached || !coordinates.isAttached) return
        if (source.localPositionOf(coordinates, Offset.Zero) != drawnAt) invalidateDraw()
    }

    override fun ContentDrawScope.draw() {
        val outline = shape.createOutline(size, layoutDirection, this)
        val path = Path().apply { addOutline(outline) }
        drawShadowOutside(outline, path)
        val glass = when (tier) {
            GlassTier.Captured -> drawCaptured(path)
            GlassTier.Backdrop -> Build.VERSION.SDK_INT >= Build.VERSION_CODES.CINNAMON_BUN && drawPlatformBackdrop(path)
            GlassTier.Opaque -> false
        }
        if (!glass) drawPath(path, style.opaque)
        drawContent()
    }

    /**
     * The platform's elevation shadow, cast by a layer of the bar's shape and drawn with the
     * bar's own area cut out.
     */
    private fun DrawScope.drawShadowOutside(outline: Outline, path: Path) {
        if (style.elevation <= 0.dp) return
        val layer = shadowLayer ?: requireGraphicsContext().createGraphicsLayer().also { shadowLayer = it }
        layer.shadowElevation = style.elevation.toPx()
        layer.ambientShadowColor = style.shadow
        layer.spotShadowColor = style.shadow
        layer.setOutline(outline)
        layer.record(size.toIntSize()) { drawPath(path, style.opaque) }
        clipPath(path, ClipOp.Difference) { drawLayer(layer) }
    }

    /**
     * Android 12 to 17.1: draws the recorded pages under the bar, blurred, then the tint. Only
     * the bar's part of the pages (and the blur's reach around it) is blurred, never the whole
     * screen. False when there is nothing recorded to draw from yet.
     */
    private fun DrawScope.drawCaptured(path: Path): Boolean {
        val backdrop = backdrop ?: return false
        val source = backdrop.coordinates ?: return false
        val bar = requireLayoutCoordinates()
        if (!source.isAttached || !bar.isAttached) return false
        val at = source.localPositionOf(bar, Offset.Zero)
        drawnAt = at

        val blur = style.blur.toPx()
        // The blur reads this far around each pixel; outside the pages the edge is repeated.
        val reach = blur * 2f
        val left = floor(maxOf(0f, at.x - reach))
        val top = floor(maxOf(0f, at.y - reach))
        val right = ceil(minOf(source.size.width.toFloat(), at.x + size.width + reach))
        val bottom = ceil(minOf(source.size.height.toFloat(), at.y + size.height + reach))
        if (right <= left || bottom <= top) return false

        val layer = blurLayer ?: requireGraphicsContext().createGraphicsLayer().also { blurLayer = it }
        layer.renderEffect = BlurEffect(blur, blur, TileMode.Clamp)
        layer.colorFilter = saturation(style.saturation)
        layer.record(IntSize((right - left).toInt(), (bottom - top).toInt())) {
            translate(-left, -top) { drawLayer(backdrop.layer) }
        }
        clipPath(path) {
            drawRect(style.opaque)
            translate(left - at.x, top - at.y) { drawLayer(layer) }
            drawRect(style.tint)
        }
        return true
    }

    /**
     * Android 17 QPR2 and later: a platform render node the size of the bar whose backdrop
     * effect blurs what the window drew behind it, with the tint drawn inside it. False when the
     * canvas cannot draw render nodes (a software canvas).
     */
    @RequiresApi(Build.VERSION_CODES.CINNAMON_BUN)
    private fun ContentDrawScope.drawPlatformBackdrop(path: Path): Boolean {
        if (!drawContext.canvas.nativeCanvas.isHardwareAccelerated) return false
        val glass = platform ?: PlatformGlass().also { platform = it }
        val blur = style.blur.toPx()
        clipPath(path) {
            glass.draw(drawContext.canvas.nativeCanvas, size.toIntSize(), path, blur, style.saturation, style.tint)
        }
        return true
    }
}

private fun saturation(amount: Float): ColorFilter? =
    if (amount == 1f) null else ColorFilter.colorMatrix(ColorMatrix().apply { setToSaturation(amount) })

/** The render node the platform backdrop blur is drawn through (Android 17 QPR2 and later). */
@RequiresApi(Build.VERSION_CODES.S)
private class PlatformGlass {
    private val node = RenderNode("Toolbar glass")
    private var effectFor: Pair<Float, Float>? = null

    fun draw(canvas: android.graphics.Canvas, size: IntSize, path: Path, blur: Float, saturation: Float, tint: Color) {
        node.setPosition(0, 0, size.width, size.height)
        node.setOutline(android.graphics.Outline().apply { setPath(path.asAndroidPath()) })
        node.clipToOutline = true
        if (effectFor != blur to saturation) {
            effectFor = blur to saturation
            val blurred = RenderEffect.createBlurEffect(blur, blur, Shader.TileMode.CLAMP)
            val matrix = android.graphics.ColorMatrix().apply { setSaturation(saturation) }
            node.setBackdropBlur(RenderEffect.createColorFilterEffect(ColorMatrixColorFilter(matrix), blurred))
        }
        // The node needs something to draw for the platform to draw it at all; the tint is it.
        val recording = node.beginRecording(size.width, size.height)
        recording.drawColor(tint.toArgb())
        node.endRecording()
        canvas.drawRenderNode(node)
    }

    fun release() = node.discardDisplayList()
}

/**
 * RenderNode.setBackdropRenderEffect is new in Android 17 QPR2 (SDK 37.2). Its only caller runs
 * when [glassTierFor] chose [GlassTier.Backdrop]; the version is checked again here, at the
 * call, because lint follows an SDK_INT_FULL check only where it is written.
 */
@RequiresApi(Build.VERSION_CODES.S)
private fun RenderNode.setBackdropBlur(effect: RenderEffect) {
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.CINNAMON_BUN &&
        Build.VERSION.SDK_INT_FULL >= Build.VERSION_CODES_FULL.CINNAMON_BUN_2
    ) {
        setBackdropRenderEffect(effect)
    }
}
