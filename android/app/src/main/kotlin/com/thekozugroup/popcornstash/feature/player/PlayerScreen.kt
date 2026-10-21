package com.thekozugroup.popcornstash.feature.player

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectVerticalDragGestures
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.TextButton
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.displayCutout
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.systemBarsIgnoringVisibility
import androidx.compose.foundation.layout.union
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import com.thekozugroup.popcornstash.ui.components.PopcornMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Slider
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.platform.LocalAccessibilityManager
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInWindow
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.thekozugroup.popcornstash.ui.components.MediaDisc
import com.thekozugroup.popcornstash.ui.components.MediaPlate
import com.thekozugroup.popcornstash.ui.components.PrimaryButton
import com.thekozugroup.popcornstash.ui.components.SecondaryButton
import com.thekozugroup.popcornstash.ui.icons.LucideIcon
import com.thekozugroup.popcornstash.ui.icons.PopcornIcons
import com.thekozugroup.popcornstash.ui.theme.LocalPlayerTokens
import com.thekozugroup.popcornstash.ui.theme.Popcorn
import com.thekozugroup.popcornstash.ui.theme.PopcornMotion
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import kotlinx.coroutines.delay

/**
 * The player's theme: always the dark Popcorn theme, whatever the system's, because the stage
 * is black and the picture decides the light in the room (android/DESIGN.md, Player).
 */
@Composable
fun PlayerTheme(content: @Composable () -> Unit) = PopcornTheme(darkTheme = true, content = content)

/** What the screen reads from the engine several times a second while the seek bar shows. */
data class PlayerClock(val positionMs: Long, val bufferedMs: Long)

/** What a person can do in the player. */
data class PlayerActions(
    val onClose: () -> Unit = {},
    val onTogglePlay: () -> Unit = {},
    val onSeek: (Long) -> Unit = {},
    val onChooseAudio: (Int) -> Unit = {},
    val onChooseQuality: (Int?) -> Unit = {},
    val onPlayNext: () -> Unit = {},
    val onPlayAgain: () -> Unit = {},
    val onRetry: () -> Unit = {},
    val onPlayVideo: (String) -> Unit = {},
    val onReloadVideos: () -> Unit = {},
    val onBrowse: () -> Unit = {},
)

/**
 * The player (android/DESIGN.md, Player and photo viewer): the video full-bleed on the black
 * stage, and over it the chrome on `mediaControl` discs and plates: close and the title at
 * the top start, the audio and quality menu at the top end, play and pause in the centre,
 * the seek bar with the times at the bottom, and "Next episode" beside it when there is one.
 *
 * The chrome fades away 3 seconds after the last touch while playing (never while paused,
 * loading, at the end, after a failure, or for TalkBack, which gets the recommended timeout)
 * and a tap on the picture brings it back. [chromeVisible] is hoisted so the host can hide
 * the system bars with it. In picture-in-picture only the picture shows.
 *
 * [surface] draws the video ([androidx.media3.ui.compose.ContentFrame] on a phone).
 */
@Composable
@OptIn(ExperimentalLayoutApi::class, ExperimentalMaterial3Api::class)
fun PlayerScreen(
    state: PlayerUiState,
    clock: () -> PlayerClock,
    actions: PlayerActions,
    chromeVisible: Boolean,
    onChromeVisibleChange: (Boolean) -> Unit,
    modifier: Modifier = Modifier,
    inPictureInPicture: Boolean = false,
    menuInitiallyOpen: Boolean = false,
    flexHinge: FlexHinge? = null,
    surface: @Composable (Modifier) -> Unit,
) {
    val colors = Popcorn.colors
    val tokens = LocalPlayerTokens.current
    val phase = state.phase
    val ready = phase == PlayerPhase.Ready
    val playing = ready && state.playWhenReady
    var drawerOpen by remember { mutableStateOf(false) }
    val playerActions = actions.copy(onBrowse = { onChromeVisibleChange(true); drawerOpen = true })
    var menuOpen by remember { mutableStateOf(menuInitiallyOpen) }
    var seeking by remember { mutableStateOf(false) }
    var seekNotice by remember { mutableStateOf<String?>(null) }
    var seekSide by remember { mutableStateOf(false) }
    var seekSerial by remember { mutableIntStateOf(0) }
    LaunchedEffect(seekSerial) { if (seekNotice != null) { delay(900); seekNotice = null } }
    var touches by remember { mutableIntStateOf(0) }
    val touched: () -> Unit = { touches++ }

    val hideAfter = LocalAccessibilityManager.current?.calculateRecommendedTimeoutMillis(
        tokens.hideAfterMs,
        containsIcons = true,
        containsText = true,
        containsControls = true,
    ) ?: tokens.hideAfterMs
    // Pausing by any route (headphones unplugged, the end, a failure) brings the chrome back.
    LaunchedEffect(playing) { if (!playing) onChromeVisibleChange(true) }

    var windowTop by remember { mutableFloatStateOf(0f) }
    val density = LocalDensity.current
    BoxWithConstraints(
        modifier
            .fillMaxSize()
            .background(colors.stage)
            .onGloballyPositioned { windowTop = it.positionInWindow().y }
            .testTag("player"),
    ) {
        val top = with(density) { ((flexHinge?.top ?: 0) - windowTop).toDp() }
        val bottom = with(density) { ((flexHinge?.bottom ?: 0) - windowTop).toDp() }
        val flex = !inPictureInPicture && flexHinge != null && top > 0.dp && bottom >= top && bottom < maxHeight
        val split = flex
        val chromeShown = chromeVisible || !ready || split
        val mayHide = playing && !menuOpen && !drawerOpen && !seeking && !inPictureInPicture && !split
        LaunchedEffect(chromeVisible, mayHide, touches) {
            if (chromeVisible && mayHide) {
                delay(hideAfter)
                onChromeVisibleChange(false)
            }
        }
        LaunchedEffect(split) { if (split) onChromeVisibleChange(true) }
        val safe = WindowInsets.systemBarsIgnoringVisibility.union(WindowInsets.displayCutout)
        val pictureHeight = top
        val controlsTop = bottom
        Box(if (split) Modifier.fillMaxWidth().height(pictureHeight).testTag("flex video") else Modifier.fillMaxSize()) {
            surface(if (split) Modifier.fillMaxSize().windowInsetsPadding(safe.only(WindowInsetsSides.Top + WindowInsetsSides.Horizontal)) else Modifier.fillMaxSize())
        }
        if (inPictureInPicture) {
            PictureInPictureOverlay(state)
            return@BoxWithConstraints
        }
        val seekBy: (Long) -> Unit = { delta ->
            if (ready && state.durationMs > 0) {
                touched()
                actions.onSeek((clock().positionMs + delta).coerceIn(0L, state.durationMs))
                seekSide = delta > 0
                seekNotice = if (delta > 0) "+15 seconds" else "−15 seconds"
                seekSerial++
            }
        }
        val currentSeek by androidx.compose.runtime.rememberUpdatedState(seekBy)
        val currentToggle by androidx.compose.runtime.rememberUpdatedState {
            touched()
            onChromeVisibleChange(if (split) true else !chromeVisible)
        }
        Box(
            (if (split) Modifier.fillMaxWidth().height(pictureHeight) else Modifier.fillMaxSize())
                .testTag("player tap area")
                .semantics {
                    onClick(label = if (chromeShown && !split) "Hide controls" else "Show controls") { currentToggle(); true }
                    customActions = listOf(
                        CustomAccessibilityAction("Back 15 seconds") { currentSeek(-15_000L); true },
                        CustomAccessibilityAction("Forward 15 seconds") { currentSeek(15_000L); true },
                    )
                }
                .pointerInput(Unit) {
                    detectTapGestures(onTap = { currentToggle() }, onDoubleTap = { point -> currentSeek(if (point.x < size.width / 2f) -15_000L else 15_000L) })
                },
        ) {
            seekNotice?.let { notice ->
                Text(notice, style = Popcorn.type.action, color = colors.onMedia,
                    modifier = Modifier.align(if (seekSide) Alignment.CenterEnd else Alignment.CenterStart)
                        .padding(24.dp).clip(Popcorn.shapes.control).background(colors.mediaControl).padding(16.dp)
                        .semantics { liveRegion = LiveRegionMode.Polite })
            }
        }
        if (split) {
            Box(Modifier.offset(y = controlsTop).fillMaxWidth().height(maxHeight - controlsTop).background(colors.page).testTag("flex controls")) {
                ControlPanel(
                    state = state,
                    clock = clock,
                    actions = playerActions,
                    waiting = phase == PlayerPhase.Loading || (ready && (state.buffering || state.switching)),
                    menuOpen = menuOpen,
                    onMenuOpen = { menuOpen = it },
                    onSeeking = { seeking = it },
                    touched = touched,
                )
            }
            return@BoxWithConstraints
        }
        if (!chromeShown) {
            var upward by remember { mutableFloatStateOf(0f) }
            Box(Modifier.align(Alignment.BottomCenter).fillMaxWidth().height(48.dp).testTag("browse swipe area").pointerInput(Unit) {
                detectVerticalDragGestures(onDragStart = { upward = 0f }, onVerticalDrag = { change, delta -> change.consume(); upward += delta }, onDragEnd = { if (upward < -24.dp.toPx()) drawerOpen = true })
            })
        }
        if (phase is PlayerPhase.Ended || phase is PlayerPhase.Failed) {
            Box(Modifier.fillMaxSize().background(colors.mediaControl))
        }
        // Waiting for the picture shows even with the chrome away, so a stalled frame never looks frozen.
        val waiting = phase == PlayerPhase.Loading || (ready && (state.buffering || state.switching))
        if (waiting && !chromeShown) {
            CentreDisc(state, loading = true, onClick = {}, modifier = Modifier.align(Alignment.Center))
        }
        AnimatedVisibility(
            visible = chromeShown,
            enter = fadeIn(PopcornMotion.fast()),
            exit = fadeOut(PopcornMotion.fast()),
            modifier = Modifier.fillMaxSize(),
        ) {
            Chrome(
                state = state,
                clock = clock,
                actions = playerActions,
                waiting = waiting,
                menuOpen = menuOpen,
                onMenuOpen = { open ->
                    touched()
                    menuOpen = open
                },
                onSeeking = { now ->
                    touched()
                    seeking = now
                },
                touched = touched,
            )
        }
    }
    if (drawerOpen && !inPictureInPicture) {
        ModalBottomSheet(onDismissRequest = { drawerOpen = false }, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true), containerColor = colors.page) {
            Column(Modifier.fillMaxWidth().heightIn(max = with(density) { androidx.compose.ui.platform.LocalWindowInfo.current.containerSize.height.toDp() * .8f }).padding(horizontal = 24.dp)) {
                Text("More videos", style = Popcorn.type.section, modifier = Modifier.semantics { heading() })
                when {
                    state.recommendationsLoading -> CircularProgressIndicator(Modifier.padding(24.dp))
                    state.recommendationsError -> {
                        Text("Videos could not be loaded.", modifier = Modifier.padding(vertical = 16.dp))
                        SecondaryButton("Try again", onClick = actions.onReloadVideos)
                    }
                    state.recommendations.isEmpty() -> Text("No other videos in this library.", modifier = Modifier.padding(vertical = 24.dp))
                    else -> LazyColumn(Modifier.fillMaxWidth().weight(1f, fill = false)) {
                        items(state.recommendations, key = { it.id }) { video ->
                            Column(Modifier.fillMaxWidth().clickable(role = Role.Button) { drawerOpen = false; actions.onPlayVideo(video.id) }.padding(vertical = 16.dp)) {
                                Text(video.title, style = Popcorn.type.body, maxLines = 2, overflow = TextOverflow.Ellipsis)
                                video.durationS?.let { Text(PlayerFormat.clock(it), style = Popcorn.type.metadata, color = colors.textMuted) }
                            }
                            HorizontalDivider()
                        }
                    }
                }
                Spacer(Modifier.height(24.dp))
            }
        }
    }
}

/** A separate, scrollable lower pane. Its top is the hinge/picture edge, not a status bar. */
@Composable
@OptIn(ExperimentalLayoutApi::class)
private fun ControlPanel(
    state: PlayerUiState,
    clock: () -> PlayerClock,
    actions: PlayerActions,
    waiting: Boolean,
    menuOpen: Boolean,
    onMenuOpen: (Boolean) -> Unit,
    onSeeking: (Boolean) -> Unit,
    touched: () -> Unit,
) {
    val tokens = LocalPlayerTokens.current
    val safe = WindowInsets.systemBarsIgnoringVisibility.union(WindowInsets.displayCutout)
    BoxWithConstraints(Modifier.fillMaxSize().windowInsetsPadding(safe.only(WindowInsetsSides.Horizontal + WindowInsetsSides.Bottom))) {
        Column(Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).heightIn(min = maxHeight).padding(tokens.edge),
            horizontalAlignment = Alignment.CenterHorizontally) {
            TopRow(state, actions, menuOpen, { touched(); onMenuOpen(it) })
            Spacer(Modifier.weight(1f).height(24.dp))
            when (val phase = state.phase) {
                PlayerPhase.Loading, PlayerPhase.Ready -> CentreDisc(state, waiting, { touched(); actions.onTogglePlay() })
                PlayerPhase.Ended -> EndMessage(state, actions)
                is PlayerPhase.Failed -> FailedMessage(phase, actions)
            }
            Spacer(Modifier.weight(1f).height(24.dp))
            if (state.phase == PlayerPhase.Ready) BottomRow(state, clock, actions, { touched(); onSeeking(it) }, stacked = true)
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun Chrome(
    state: PlayerUiState,
    clock: () -> PlayerClock,
    actions: PlayerActions,
    waiting: Boolean,
    menuOpen: Boolean,
    onMenuOpen: (Boolean) -> Unit,
    onSeeking: (Boolean) -> Unit,
    touched: () -> Unit,
) {
    val tokens = LocalPlayerTokens.current
    // Insets that do not move when the system bars come and go with the chrome.
    val safe = WindowInsets.systemBarsIgnoringVisibility.union(WindowInsets.displayCutout)
    Box(
        Modifier
            .fillMaxSize()
            .windowInsetsPadding(safe)
            .padding(tokens.edge),
    ) {
        TopRow(state, actions, menuOpen, onMenuOpen, Modifier.align(Alignment.TopStart))
        when (val phase = state.phase) {
            PlayerPhase.Loading, PlayerPhase.Ready -> CentreDisc(
                state = state,
                loading = waiting,
                onClick = {
                    touched()
                    actions.onTogglePlay()
                },
                modifier = Modifier.align(Alignment.Center),
            )
            PlayerPhase.Ended -> EndMessage(state, actions, Modifier.align(Alignment.Center))
            is PlayerPhase.Failed -> FailedMessage(phase, actions, Modifier.align(Alignment.Center))
        }
        if (state.phase == PlayerPhase.Ready) {
            BottomRow(state, clock, actions, onSeeking, Modifier.align(Alignment.BottomCenter), stacked = androidx.compose.ui.platform.LocalConfiguration.current.orientation == android.content.res.Configuration.ORIENTATION_PORTRAIT)
        }
    }
}

@Composable
private fun TopRow(
    state: PlayerUiState,
    actions: PlayerActions,
    menuOpen: Boolean,
    onMenuOpen: (Boolean) -> Unit,
    modifier: Modifier = Modifier,
) {
    val tokens = LocalPlayerTokens.current
    val colors = Popcorn.colors
    Row(
        modifier.fillMaxWidth(),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        Row(Modifier.weight(1f, fill = false), verticalAlignment = Alignment.CenterVertically) {
            MediaDisc(PopcornIcons.Close, "Close player", actions.onClose)
            if (state.title.isNotEmpty()) {
                Spacer(Modifier.width(tokens.gap))
                MediaPlate(Modifier.semantics(mergeDescendants = true) { heading() }) {
                    Column {
                        Text(
                            state.title,
                            style = Popcorn.type.section,
                            color = colors.onMedia,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                        state.subtitle?.let {
                            Text(it, style = Popcorn.type.metadata, color = colors.onMedia, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        }
                    }
                }
            }
        }
        val showMenu = state.hasMenu && (state.phase == PlayerPhase.Ready || state.phase == PlayerPhase.Loading)
        if (showMenu) {
            Spacer(Modifier.width(tokens.gap))
            StreamMenu(state, actions, menuOpen, onMenuOpen)
        }
    }
}

@Composable
private fun CentreDisc(state: PlayerUiState, loading: Boolean, onClick: () -> Unit, modifier: Modifier = Modifier) {
    val tokens = LocalPlayerTokens.current
    val colors = Popcorn.colors
    val canToggle = state.phase == PlayerPhase.Ready
    val label = when {
        !canToggle -> "Loading video"
        state.playWhenReady -> "Pause"
        else -> "Play"
    }
    Box(
        modifier
            .size(tokens.playDisc)
            .clip(CircleShape)
            .background(colors.mediaControl)
            .clickable(enabled = canToggle, role = Role.Button, onClick = onClick)
            .semantics {
                contentDescription = label
                if (loading) stateDescription = "Loading"
            }
            .testTag("play pause"),
        contentAlignment = Alignment.Center,
    ) {
        if (loading) {
            CircularProgressIndicator(color = colors.onMedia, strokeWidth = 2.5.dp, modifier = Modifier.size(tokens.loadingRing))
        } else {
            LucideIcon(
                if (state.playWhenReady) PopcornIcons.Pause else PopcornIcons.Play,
                contentDescription = null,
                tint = colors.onMedia,
                size = tokens.playIcon,
            )
        }
    }
}

@Composable
private fun BottomRow(
    state: PlayerUiState,
    clock: () -> PlayerClock,
    actions: PlayerActions,
    onSeeking: (Boolean) -> Unit,
    modifier: Modifier = Modifier,
    stacked: Boolean = false,
) {
    val tokens = LocalPlayerTokens.current
    val colors = Popcorn.colors
    // The times and the bar move only while they show.
    var now by remember { mutableStateOf(clock()) }
    LaunchedEffect(clock) {
        while (true) {
            now = clock()
            delay(tokens.tickMs)
        }
    }
    Column(modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally) {
    if (stacked) {
        Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(tokens.gap), horizontalAlignment = Alignment.End) {
            MediaPlate(Modifier.fillMaxWidth()) {
                SeekBar(now.positionMs, now.bufferedMs, state.durationMs, actions.onSeek, onSeeking)
            }
            NextEpisodeButton(state, actions)
        }
    } else {
    Row(
        Modifier.fillMaxWidth(),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(tokens.gap),
    ) {
        MediaPlate(Modifier.weight(1f)) {
            SeekBar(
                positionMs = now.positionMs,
                bufferedMs = now.bufferedMs,
                durationMs = state.durationMs,
                onSeek = actions.onSeek,
                onSeeking = onSeeking,
            )
        }
        NextEpisodeButton(state, actions)
    }
    }
    var upward by remember { mutableFloatStateOf(0f) }
    TextButton(onClick = actions.onBrowse, modifier = Modifier.heightIn(min = 48.dp).testTag("more videos")
        .pointerInput(actions.onBrowse) {
            detectVerticalDragGestures(onDragStart = { upward = 0f }, onVerticalDrag = { change, delta -> change.consume(); upward += delta }, onDragEnd = { if (upward < -24.dp.toPx()) actions.onBrowse() })
        }) { Text("More videos", color = colors.onMedia) }
    }

}

@Composable
private fun NextEpisodeButton(state: PlayerUiState, actions: PlayerActions) {
    state.next?.let { next ->
        val description = next.label?.let { "Next episode: $it" } ?: "Next episode"
        MediaPlate(
            Modifier
                .clickable(role = Role.Button, onClick = actions.onPlayNext)
                .semantics(mergeDescendants = true) { contentDescription = description },
        ) {
            Text("Next episode", style = Popcorn.type.action, color = Popcorn.colors.onMedia, maxLines = 1)
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun RowScope.SeekBar(
    positionMs: Long,
    bufferedMs: Long,
    durationMs: Long,
    onSeek: (Long) -> Unit,
    onSeeking: (Boolean) -> Unit,
) {
    val tokens = LocalPlayerTokens.current
    val colors = Popcorn.colors
    var dragFraction by remember { mutableStateOf<Float?>(null) }
    val duration = durationMs.coerceAtLeast(0)
    val fraction = if (duration > 0) (positionMs.toFloat() / duration).coerceIn(0f, 1f) else 0f
    val buffered = if (duration > 0) (bufferedMs.toFloat() / duration).coerceIn(0f, 1f) else 0f
    val shownMs = dragFraction?.let { (it * duration).toLong() } ?: positionMs
    val shownS = shownMs / 1000.0
    val totalS = duration / 1000.0

    Text(
        PlayerFormat.clock(shownS),
        style = Popcorn.type.metadata,
        color = colors.onMedia,
        // The seek bar says both times to TalkBack.
        modifier = Modifier.clearAndSetSemantics {},
    )
    Spacer(Modifier.width(Popcorn.spacing.s))
    Slider(
        value = dragFraction ?: fraction,
        onValueChange = {
            if (dragFraction == null) onSeeking(true)
            dragFraction = it
        },
        onValueChangeFinished = {
            // A tap on the current thumb can finish without onValueChange. Keep its
            // current position instead of using an uninitialized/stale drag value.
            val target = dragFraction ?: fraction
            dragFraction = null
            onSeeking(false)
            onSeek((target * duration).toLong())
        },
        enabled = duration > 0,
        thumb = {
            Box(
                Modifier
                    .size(tokens.seekThumb)
                    .testTag("seek thumb")
                    .background(colors.accent, CircleShape)
                    .border(tokens.seekThumbRing, colors.onMedia, CircleShape),
            )
        },
        track = {
            SeekTrack(played = dragFraction ?: fraction, buffered = buffered)
        },
        modifier = Modifier
            .weight(1f)
            .height(tokens.disc)
            .semantics {
                contentDescription = "Position"
                stateDescription = "${PlayerFormat.spokenDuration(shownS)} of ${PlayerFormat.spokenDuration(totalS)}"
            }
            .testTag("seek bar"),
    )
    Spacer(Modifier.width(Popcorn.spacing.s))
    Text(
        PlayerFormat.clock(totalS),
        style = Popcorn.type.metadata,
        color = colors.onMedia,
        // The seek bar says both times to TalkBack.
        modifier = Modifier.clearAndSetSemantics {},
    )
}

/** The bar itself: Action Blue for what played, `onMedia` at low strength for what loaded and what is left. */
@Composable
private fun SeekTrack(played: Float, buffered: Float) {
    val tokens = LocalPlayerTokens.current
    val colors = Popcorn.colors
    val rtl = androidx.compose.ui.platform.LocalLayoutDirection.current == androidx.compose.ui.unit.LayoutDirection.Rtl
    Canvas(
        Modifier
            .fillMaxWidth()
            .height(tokens.seekTrack),
    ) {
        val radius = CornerRadius(size.height / 2, size.height / 2)
        drawRoundRect(colors.onMedia.copy(alpha = 0.28f), cornerRadius = radius)
        if (buffered > played) {
            drawRoundRect(colors.onMedia.copy(alpha = 0.5f), topLeft = Offset(if (rtl) size.width * (1 - buffered) else 0f, 0f), size = Size(size.width * buffered, size.height), cornerRadius = radius)
        }
        if (played > 0f) {
            drawRoundRect(colors.accent, topLeft = Offset(if (rtl) size.width * (1 - played) else 0f, 0f), size = Size(size.width * played, size.height), cornerRadius = radius)
        }
    }
}

@Composable
private fun StreamMenu(state: PlayerUiState, actions: PlayerActions, open: Boolean, onOpen: (Boolean) -> Unit) {
    val colors = Popcorn.colors
    val label = when {
        state.audio.isNotEmpty() && state.qualities.isNotEmpty() -> "Audio and quality"
        state.audio.isNotEmpty() -> "Audio"
        else -> "Quality"
    }
    Box {
        MediaDisc(PopcornIcons.More, label, onClick = { onOpen(true) })
        DropdownMenu(
            expanded = open,
            onDismissRequest = { onOpen(false) },
            shape = Popcorn.shapes.panel,
            containerColor = colors.surface,
            modifier = Modifier.widthIn(min = 240.dp).testTag("stream menu"),
        ) {
            if (state.audio.isNotEmpty()) {
                MenuHeading("Audio")
                state.audio.forEach { track ->
                    MenuChoice(track.label, track.detail, selected = track.index == state.selectedAudio) {
                        onOpen(false)
                        actions.onChooseAudio(track.index)
                    }
                }
            }
            if (state.qualities.isNotEmpty()) {
                if (state.audio.isNotEmpty()) HorizontalDivider(color = colors.divider, modifier = Modifier.padding(vertical = 4.dp))
                MenuHeading("Quality")
                state.qualities.forEach { quality ->
                    MenuChoice(quality.label, quality.detail, selected = quality.height == state.selectedQuality) {
                        onOpen(false)
                        actions.onChooseQuality(quality.height)
                    }
                }
            }
            if (state.converted) {
                HorizontalDivider(color = colors.divider, modifier = Modifier.padding(vertical = 4.dp))
                Text(
                    "Converted for this phone while you watch",
                    style = Popcorn.type.metadata,
                    color = colors.textMuted,
                    modifier = Modifier.padding(horizontal = Popcorn.spacing.m, vertical = Popcorn.spacing.xs),
                )
            }
        }
    }
}

@Composable
private fun MenuHeading(text: String) {
    Text(
        text,
        style = Popcorn.type.metadata,
        color = Popcorn.colors.textMuted,
        modifier = Modifier
            .padding(start = Popcorn.spacing.m, end = Popcorn.spacing.m, top = Popcorn.spacing.s, bottom = Popcorn.spacing.xxs)
            .semantics { heading() },
    )
}

@Composable
private fun MenuChoice(label: String, detail: String, selected: Boolean, onClick: () -> Unit) {
    val colors = Popcorn.colors
    PopcornMenuItem(
        text = {
            Column {
                Text(label, style = Popcorn.type.item, color = colors.text)
                Text(detail, style = Popcorn.type.metadata, color = colors.textMuted)
            }
        },
        leadingIcon = {
            Box(Modifier.size(24.dp)) {
                if (selected) LucideIcon(PopcornIcons.Chosen, contentDescription = null, tint = colors.text)
            }
        },
        onClick = onClick,
        modifier = Modifier.semantics {
            this.selected = selected
            role = Role.RadioButton
        },
    )
}

@Composable
private fun EndMessage(state: PlayerUiState, actions: PlayerActions, modifier: Modifier = Modifier) {
    val tokens = LocalPlayerTokens.current
    val colors = Popcorn.colors
    Column(
        modifier.widthIn(max = tokens.messageWidth),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        val next = state.next
        if (next != null) {
            next.label?.let {
                Text(
                    "Next: $it",
                    style = Popcorn.type.section,
                    color = colors.onMedia,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite },
                )
                Spacer(Modifier.height(Popcorn.spacing.gutter))
            }
            Row(horizontalArrangement = Arrangement.spacedBy(Popcorn.spacing.s)) {
                PrimaryButton("Play next episode", actions.onPlayNext, glyph = PopcornIcons.Play, modifier = Modifier.testTag("play next"))
                SecondaryButton("Back", actions.onClose)
            }
        } else {
            Row(horizontalArrangement = Arrangement.spacedBy(Popcorn.spacing.s)) {
                PrimaryButton("Play again", actions.onPlayAgain, glyph = PopcornIcons.StartAgain)
                SecondaryButton("Back", actions.onClose)
            }
        }
    }
}

@Composable
private fun FailedMessage(phase: PlayerPhase.Failed, actions: PlayerActions, modifier: Modifier = Modifier) {
    val tokens = LocalPlayerTokens.current
    val colors = Popcorn.colors
    Column(
        modifier.widthIn(max = tokens.messageWidth),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        if (phase.gone) {
            Text(
                phase.message,
                style = Popcorn.type.section,
                color = colors.onMedia,
                textAlign = TextAlign.Center,
                modifier = Modifier.semantics { liveRegion = LiveRegionMode.Assertive },
            )
            Spacer(Modifier.height(Popcorn.spacing.gutter))
            PrimaryButton("Back", actions.onClose)
        } else {
            Column(
                Modifier.semantics(mergeDescendants = true) { liveRegion = LiveRegionMode.Assertive },
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Text("Playback stopped.", style = Popcorn.type.section, color = colors.onMedia, textAlign = TextAlign.Center)
                Spacer(Modifier.height(Popcorn.spacing.xs))
                Text(phase.message, style = Popcorn.type.body, color = colors.onMedia, textAlign = TextAlign.Center)
            }
            Spacer(Modifier.height(Popcorn.spacing.gutter))
            Row(horizontalArrangement = Arrangement.spacedBy(Popcorn.spacing.s)) {
                PrimaryButton("Try again", actions.onRetry)
                SecondaryButton("Back", actions.onClose)
            }
        }
    }
}

/** Picture-in-picture is too small for controls: only a waiting ring on a small disc, or the reason it stopped. */
@Composable
private fun BoxScope.PictureInPictureOverlay(state: PlayerUiState) {
    val colors = Popcorn.colors
    val tokens = LocalPlayerTokens.current
    when (val phase = state.phase) {
        is PlayerPhase.Failed -> Box(Modifier.fillMaxSize().background(colors.mediaControl), contentAlignment = Alignment.Center) {
            Text(
                if (phase.gone) phase.message else "Playback stopped.",
                style = Popcorn.type.metadata,
                color = colors.onMedia,
                textAlign = TextAlign.Center,
                modifier = Modifier.padding(Popcorn.spacing.xs),
            )
        }
        else -> if (phase == PlayerPhase.Loading || state.buffering || state.switching) {
            Box(
                Modifier
                    .align(Alignment.Center)
                    .size(tokens.pictureInPictureDisc)
                    .clip(CircleShape)
                    .background(colors.mediaControl),
                contentAlignment = Alignment.Center,
            ) {
                CircularProgressIndicator(color = colors.onMedia, strokeWidth = 2.dp, modifier = Modifier.size(20.dp))
            }
        }
    }
}
