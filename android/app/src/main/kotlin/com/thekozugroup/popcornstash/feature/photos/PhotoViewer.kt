package com.thekozugroup.popcornstash.feature.photos

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.displayCutout
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.systemBarsIgnoringVisibility
import androidx.compose.foundation.layout.union
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.IconButton
import androidx.compose.material3.PlainTooltip
import androidx.compose.material3.Text
import androidx.compose.material3.TooltipAnchorPosition
import androidx.compose.material3.TooltipBox
import androidx.compose.material3.TooltipDefaults
import androidx.compose.material3.rememberTooltipState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.core.net.toUri
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import coil3.size.Size
import com.thekozugroup.popcornstash.data.api.Item
import com.thekozugroup.popcornstash.data.api.MediaKind
import com.thekozugroup.popcornstash.feature.library.LibraryServices
import com.thekozugroup.popcornstash.feature.library.MediaFormat
import com.thekozugroup.popcornstash.feature.library.capturedAt
import com.thekozugroup.popcornstash.feature.library.OtherFilesViewModel
import com.thekozugroup.popcornstash.feature.library.displayTitle
import com.thekozugroup.popcornstash.navigation.HideToolbarWhileShown
import com.thekozugroup.popcornstash.ui.components.MediaDisc
import com.thekozugroup.popcornstash.ui.icons.LucideGlyph
import com.thekozugroup.popcornstash.ui.icons.LucideIcon
import com.thekozugroup.popcornstash.ui.icons.PopcornIcons
import com.thekozugroup.popcornstash.ui.media.LocalPictures
import com.thekozugroup.popcornstash.ui.media.StashImage
import com.thekozugroup.popcornstash.ui.theme.LocalPlayerTokens
import com.thekozugroup.popcornstash.ui.theme.Popcorn
import com.thekozugroup.popcornstash.ui.theme.PopcornMotion

/**
 * The photo viewer over a library page or Home, with everything it needs: it saves
 * originals through the system's "Save to" picker and says how that went with [onMessage].
 */
@Composable
fun PhotoViewerOverlay(
    photos: List<Item>,
    startId: String,
    services: LibraryServices,
    onClose: () -> Unit,
    onDetails: (Item) -> Unit,
    onPlay: (Item) -> Unit,
    onMessage: (String) -> Unit,
    onNearEnd: () -> Unit = {},
) {
    val context = LocalContext.current
    val resolver = context.applicationContext.contentResolver
    val saver: SaveOriginalViewModel = viewModel(
        key = "save original",
        factory = viewModelFactory {
            initializer { SaveOriginalViewModel(services.api, services.files) { uri -> resolver.openOutputStream(uri.toUri(), "w") } }
        },
    )
    val saving by saver.saving.collectAsStateWithLifecycle()
    val message by rememberUpdatedState(onMessage)
    LaunchedEffect(saver) { saver.messages.collect { message(it) } }
    var pending by rememberSaveable { mutableStateOf<String?>(null) }
    val pendingItem = photos.firstOrNull { it.id == pending }
    val mime = remember(pendingItem) { pendingItem?.let { OtherFilesViewModel.mimeFor(MediaFormat.fileName(it.relPath)) } ?: "image/*" }
    val contract = remember(mime) { ActivityResultContracts.CreateDocument(mime) }
    val launcher = rememberLauncherForActivityResult(contract) { uri ->
        val id = pending
        pending = null
        if (uri != null && id != null) saver.save(id, uri.toString())
    }
    var launchFor by remember { mutableStateOf<Item?>(null) }
    // Launch once the contract for this file's type is the registered one.
    LaunchedEffect(launchFor, contract) {
        val item = launchFor ?: return@LaunchedEffect
        launchFor = null
        launcher.launch(MediaFormat.fileName(item.relPath).ifBlank { item.title })
    }
    PhotoViewer(
        photos = photos,
        startId = startId,
        savingId = saving,
        onClose = onClose,
        onDetails = onDetails,
        onPlay = onPlay,
        onSaveOriginal = { item ->
            pending = item.id
            launchFor = item
        },
        onNearEnd = onNearEnd,
        format = services.format,
    )
}

/**
 * The full-screen photo viewer (android/DESIGN.md, Player and photo viewer): a black stage
 * under the system bars, the floating toolbar gone, photos paged sideways. A photo zooms
 * with a pinch or a double tap and pans while zoomed; a tap shows or hides the close button
 * and the plate with the date, the camera, "Save the original" and "Details" (and the system
 * bars with them). Back closes it.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun PhotoViewer(
    photos: List<Item>,
    startId: String,
    onClose: () -> Unit,
    onDetails: (Item) -> Unit,
    onPlay: (Item) -> Unit,
    onSaveOriginal: (Item) -> Unit,
    modifier: Modifier = Modifier,
    savingId: String? = null,
    onNearEnd: () -> Unit = {},
    format: MediaFormat = MediaFormat(),
    initialControls: Boolean = true,
) {
    HideToolbarWhileShown()
    BackHandler(onBack = onClose)
    var controls by rememberSaveable { mutableStateOf(initialControls) }
    ImmersiveBars(controlsVisible = controls)

    val start = remember { photos.indexOfFirst { it.id == startId }.coerceAtLeast(0) }
    val pager = rememberPagerState(initialPage = start) { photos.size }
    val zooms = remember { HashMap<String, ZoomState>() }
    val reduceMotion = Popcorn.reduceMotion
    val nearEnd by rememberUpdatedState(onNearEnd)
    LaunchedEffect(pager.currentPage, photos.size) {
        if (photos.isEmpty()) onClose() else if (pager.currentPage >= photos.size - 5) nearEnd()
    }
    LaunchedEffect(pager.settledPage) {
        val shown = photos.getOrNull(pager.settledPage)?.id
        zooms.filterKeys { it != shown }.values.forEach { it.reset() }
    }
    val current = photos.getOrNull(pager.currentPage)
    val decode = decodeSize()
    val pictures = LocalPictures.current
    val tokens = LocalPlayerTokens.current

    Box(
        modifier
            .fillMaxSize()
            .background(Popcorn.colors.stage)
            .testTag("photo viewer"),
    ) {
        HorizontalPager(
            state = pager,
            key = { photos[it].id },
            beyondViewportPageCount = 1,
            pageSpacing = 16.dp,
            userScrollEnabled = current?.let { zooms[it.id]?.zoomed } != true,
            modifier = Modifier.fillMaxSize(),
        ) { index ->
            val photo = photos[index]
            val label = format.displayTitle(photo)
            if (photo.kind == MediaKind.Video) {
                Box(
                    Modifier
                        .fillMaxSize()
                        .clickable(onClickLabel = "Show or hide the controls", onClick = { controls = !controls }),
                    contentAlignment = Alignment.Center,
                ) {
                    StashImage(pictures.thumb(photo, large = true), contentDescription = label, contentScale = ContentScale.Fit, modifier = Modifier.fillMaxSize())
                    MediaDisc(PopcornIcons.Play, "Play", onClick = { onPlay(photo) }, size = tokens.playDisc, iconSize = tokens.playIcon)
                }
            } else {
                val zoom = zooms.getOrPut(photo.id) { ZoomState() }
                Zoomable(zoom, reduceMotion, onTap = { controls = !controls }) {
                    StashImage(
                        url = pictures.view(photo.id),
                        contentDescription = label,
                        contentScale = ContentScale.Fit,
                        size = decode,
                        modifier = Modifier.fillMaxSize(),
                        placeholder = {
                            StashImage(pictures.thumb(photo, large = true), contentDescription = null, contentScale = ContentScale.Fit, modifier = Modifier.fillMaxSize())
                        },
                    )
                }
            }
        }

        val fade = tween<Float>(if (reduceMotion) 0 else PopcornMotion.FAST_MS)
        // The player's chrome rules: every control the same edge inside insets that do not
        // move when the system bars come and go with the controls.
        val safe = WindowInsets.systemBarsIgnoringVisibility.union(WindowInsets.displayCutout)
        Box(
            Modifier
                .fillMaxSize()
                .windowInsetsPadding(safe)
                .testTag("viewer chrome")
                .padding(tokens.edge),
        ) {
            AnimatedVisibility(
                visible = controls,
                enter = fadeIn(fade),
                exit = fadeOut(fade),
                modifier = Modifier.align(Alignment.TopStart),
            ) {
                MediaDisc(PopcornIcons.Close, "Close", onClick = onClose)
            }
            if (current != null) {
                AnimatedVisibility(
                    visible = controls,
                    enter = fadeIn(fade),
                    exit = fadeOut(fade),
                    modifier = Modifier.align(Alignment.BottomCenter),
                ) {
                    Plate(current, saving = savingId == current.id, format, onSaveOriginal, onDetails)
                }
            }
        }
    }
}

/** The date and camera, with "Save the original" and "Details", on a `mediaControl` plate. */
@Composable
private fun Plate(item: Item, saving: Boolean, format: MediaFormat, onSave: (Item) -> Unit, onDetails: (Item) -> Unit) {
    val onMedia = Popcorn.colors.onMedia
    // The camera's clock as it read where the photo was taken, not moved to this phone's zone.
    val taken = item.capturedAt
    Row(
        Modifier
            .fillMaxWidth()
            .testTag("viewer plate")
            .background(Popcorn.colors.mediaControl, Popcorn.shapes.panel)
            .padding(start = Popcorn.spacing.m, end = Popcorn.spacing.xxs, top = Popcorn.spacing.xxs, bottom = Popcorn.spacing.xxs),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f).padding(vertical = Popcorn.spacing.xs).semantics(mergeDescendants = true) { }) {
            Text(
                if (taken != null) format.dateTime(taken) else "Added ${format.date(item.addedAt)}",
                style = Popcorn.type.item,
                color = onMedia,
            )
            val camera = item.camera?.takeIf { it.isNotBlank() }
            if (camera != null) Text(camera, style = Popcorn.type.metadata, color = onMedia.copy(alpha = 0.72f))
        }
        if (saving) {
            Box(Modifier.size(48.dp).semantics { contentDescription = "Saving the original" }, contentAlignment = Alignment.Center) {
                CircularProgressIndicator(color = onMedia, strokeWidth = 2.dp, modifier = Modifier.size(20.dp))
            }
        } else {
            PlateAction(PopcornIcons.SaveOriginal, "Save the original") { onSave(item) }
        }
        PlateAction(PopcornIcons.Details, "Details") { onDetails(item) }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun PlateAction(glyph: LucideGlyph, label: String, onClick: () -> Unit) {
    TooltipBox(
        positionProvider = TooltipDefaults.rememberTooltipPositionProvider(TooltipAnchorPosition.Above),
        tooltip = { PlainTooltip { Text(label) } },
        state = rememberTooltipState(),
    ) {
        IconButton(onClick = onClick) {
            LucideIcon(glyph, contentDescription = label, tint = Popcorn.colors.onMedia)
        }
    }
}

/** How big a photo is decoded: twice the window's long edge, so a zoom stays sharp, at most 4096. */
@Composable
private fun decodeSize(): Size {
    val window = LocalWindowInfo.current.containerSize
    val edge = (maxOf(window.width, window.height) * 2).coerceIn(1024, 4096)
    return Size(edge, edge)
}

/**
 * Edge to edge on the stage: the system bars hide with the controls (a swipe from the edge
 * shows them for a moment), and their icons stay light on black while the viewer shows.
 */
@Composable
private fun ImmersiveBars(controlsVisible: Boolean) {
    val view = LocalView.current
    val window = view.context.findActivity()?.window ?: return
    DisposableEffect(window) {
        val controller = WindowCompat.getInsetsController(window, view)
        val lightStatus = controller.isAppearanceLightStatusBars
        val lightNavigation = controller.isAppearanceLightNavigationBars
        controller.isAppearanceLightStatusBars = false
        controller.isAppearanceLightNavigationBars = false
        onDispose {
            controller.isAppearanceLightStatusBars = lightStatus
            controller.isAppearanceLightNavigationBars = lightNavigation
            controller.show(WindowInsetsCompat.Type.systemBars())
        }
    }
    DisposableEffect(window, controlsVisible) {
        val controller = WindowCompat.getInsetsController(window, view)
        if (controlsVisible) {
            controller.show(WindowInsetsCompat.Type.systemBars())
        } else {
            controller.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            controller.hide(WindowInsetsCompat.Type.systemBars())
        }
        onDispose { }
    }
}

private tailrec fun Context.findActivity(): Activity? = when (this) {
    is Activity -> this
    is ContextWrapper -> baseContext.findActivity()
    else -> null
}
