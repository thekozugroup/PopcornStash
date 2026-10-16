package com.thekozugroup.popcornstash.feature.player

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.ActivityInfo
import android.graphics.Color
import android.graphics.Rect
import android.os.Build
import android.os.Bundle
import com.thekozugroup.popcornstash.privacy.PrivateActivity
import androidx.activity.SystemBarStyle
import androidx.activity.addCallback
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.toAndroidRectF
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.boundsInWindow
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.LocalView
import androidx.core.content.ContextCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.media3.common.util.UnstableApi
import androidx.media3.ui.compose.ContentFrame
import androidx.media3.ui.compose.SURFACE_TYPE_SURFACE_VIEW
import androidx.window.layout.FoldingFeature
import androidx.window.layout.WindowInfoTracker
import androidx.window.layout.WindowLayoutInfo
import com.thekozugroup.popcornstash.AppContainer
import com.thekozugroup.popcornstash.MainActivity
import com.thekozugroup.popcornstash.PopcornStashApplication
import com.thekozugroup.popcornstash.data.session.SessionState
import com.thekozugroup.popcornstash.ui.theme.Popcorn

/**
 * The full-screen player (docs/architecture.md section 10, Item and player; android/DESIGN.md,
 * Player). Its own private activity, so it can turn to landscape and hide
 * the system bars without touching the tabs; the floating toolbar is never on it.
 *
 * - Opened with [openPlayer]. It is `singleTask`, so another request replaces what plays
 *   ([PlayerLauncher.request] from the new intent).
 * - Landscape, either way round as the phone turns; a video taller than wide turns upright.
 * - The system bars hide with the chrome and come back with it.
 * - Leaving the app pauses playback and locks its content. Picture-in-picture is disabled.
 * - Back or Close ends the player: the place is saved and the server's stream ended.
 * - It plays only for the signed-in person: signing out (or a 401) closes it.
 */
class PlayerActivity : PrivateActivity() {
    private val request = mutableStateOf<PlayerRequest?>(null)
    private val inPictureInPicture = mutableStateOf(false)
    private var controller: PlayerController? = null

    private val pictureInPictureControls = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            if (intent.action != PictureInPicture.ACTION_CONTROL) return
            if (intent.getBooleanExtra(PictureInPicture.EXTRA_PLAY, false)) controller?.play() else controller?.pause()
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
            navigationBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
        )
        super.onCreate(savedInstanceState)
        val first = PlayerLauncher.request(intent)
        if (first == null) {
            finish()
            return
        }
        request.value = first
        orient(portrait = false)
        ContextCompat.registerReceiver(
            this,
            pictureInPictureControls,
            IntentFilter(PictureInPicture.ACTION_CONTROL),
            ContextCompat.RECEIVER_NOT_EXPORTED,
        )
        addOnNewIntentListener { next -> PlayerLauncher.request(next)?.let { request.value = it } }
        addOnPictureInPictureModeChangedListener { info ->
            inPictureInPicture.value = info.isInPictureInPictureMode
            // Leaving picture-in-picture while stopped means the person closed the window.
            if (!info.isInPictureInPictureMode && lifecycle.currentState == Lifecycle.State.CREATED) finishAndRemoveTask()
        }
        onBackPressedDispatcher.addCallback(this) { closePlayer() }

        val container = (application as PopcornStashApplication).container
        setPrivateContent {
            val layoutFlow = remember { WindowInfoTracker.getOrCreate(this).windowLayoutInfo(this) }
            val layout by layoutFlow.collectAsStateWithLifecycle(initialValue = null)
            PlayerTheme {
                PlayerHost(container, request.value, inPictureInPicture.value, layout)
            }
        }
    }

    override fun onStop() {
        super.onStop()
        if (!isInPictureInPictureMode) controller?.onBackground()
    }

    override fun onDestroy() {
        if (request.value != null) unregisterReceiver(pictureInPictureControls)
        super.onDestroy()
    }

    /** Close or Back: the player ends; after picture-in-picture, the app comes back instead of the launcher. */
    fun closePlayer() {
        if (isTaskRoot) {
            startActivity(
                Intent(this, MainActivity::class.java)
                    .setAction(Intent.ACTION_MAIN)
                    .addCategory(Intent.CATEGORY_LAUNCHER)
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_NO_USER_ACTION),
            )
            finishAndRemoveTask()
        } else {
            finish()
        }
    }

    override fun onPrivateLocked() { controller?.onBackground() }
    override fun onPrivateUnlocked() { controller?.onForeground() }

    internal fun attach(controller: PlayerController?) {
        if (controller == null && !appLock.unlocked.value) this.controller?.onBackground()
        this.controller = controller
        if (appLock.unlocked.value) controller?.onForeground() else controller?.onBackground()
    }

    internal fun showSystemBars(show: Boolean) {
        val insets = WindowCompat.getInsetsController(window, window.decorView)
        insets.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        if (show) insets.show(WindowInsetsCompat.Type.systemBars()) else insets.hide(WindowInsetsCompat.Type.systemBars())
    }

    /** Landscape either way round as the phone turns, or upright for a video taller than wide. */
    internal fun orient(portrait: Boolean, foldable: Boolean = false) {
        val wanted = if (foldable) ActivityInfo.SCREEN_ORIENTATION_USER
            else if (portrait) ActivityInfo.SCREEN_ORIENTATION_USER_PORTRAIT else ActivityInfo.SCREEN_ORIENTATION_USER_LANDSCAPE
        if (requestedOrientation != wanted) requestedOrientation = wanted
    }

    internal fun updatePictureInPicture(state: PlayerUiState, videoBounds: Rect?) {
        if (!PictureInPicture.supported(this)) return
        val playing = state.phase == PlayerPhase.Ready && state.playWhenReady
        val params = PictureInPicture.params(
            context = this,
            playing = playing,
            allowAutoEnter = false,
            controllable = state.phase == PlayerPhase.Ready,
            aspect = PictureInPicture.aspect(state.videoWidth, state.videoHeight),
            sourceRect = videoBounds?.let { PictureInPicture.fittedRect(it, state.videoWidth, state.videoHeight) },
        )
        runCatching { setPictureInPictureParams(params) }
    }


}

@Composable
private fun PlayerActivity.PlayerHost(container: AppContainer, request: PlayerRequest?, inPictureInPicture: Boolean, layout: WindowLayoutInfo?) {
    val sessions by container.sessions.state.collectAsStateWithLifecycle()
    when (val current = sessions) {
        SessionState.Restoring -> Box(Modifier.fillMaxSize().background(Popcorn.colors.stage))
        is SessionState.SignedOut -> LaunchedEffect(Unit) { finish() }
        is SessionState.SignedIn -> key(current.session.id) {
            val model: PlayerViewModel = viewModel(
                key = "player ${current.session.id}",
                factory = PlayerViewModel.factory(application, current.session),
            )
            DisposableEffect(model) {
                attach(model.controller)
                onDispose { attach(null) }
            }
            PlayerRoute(model, request, inPictureInPicture, layout)
        }
    }
}

@androidx.annotation.OptIn(UnstableApi::class)
@Composable
private fun PlayerActivity.PlayerRoute(model: PlayerViewModel, request: PlayerRequest?, inPictureInPicture: Boolean, layout: WindowLayoutInfo?) {
    val controller = model.controller
    if (request != null) LaunchedEffect(request) { controller.open(request) }
    val state by controller.state.collectAsStateWithLifecycle()
    var chromeVisible by rememberSaveable { mutableStateOf(true) }
    var videoBounds by remember { mutableStateOf<Rect?>(null) }

    val flexHinge = layout?.flexHinge()
    val foldable = layout?.displayFeatures?.any { it is FoldingFeature } == true
    val barsShown = !inPictureInPicture && (flexHinge != null || chromeVisible || state.phase != PlayerPhase.Ready)
    LaunchedEffect(barsShown) { showSystemBars(barsShown) }
    LaunchedEffect(state.portrait, foldable) { orient(state.portrait, foldable) }
    LaunchedEffect(state.phase, state.playWhenReady, state.videoWidth, state.videoHeight, videoBounds) {
        updatePictureInPicture(state, videoBounds)
    }
    val view = LocalView.current
    val keepAwake = state.phase == PlayerPhase.Ready && state.playWhenReady
    DisposableEffect(keepAwake) {
        view.keepScreenOn = keepAwake
        onDispose { view.keepScreenOn = false }
    }

    PlayerScreen(
        state = state,
        clock = { PlayerClock(controller.positionMs, controller.bufferedMs) },
        actions = PlayerActions(
            onClose = ::closePlayer,
            onTogglePlay = controller::togglePlay,
            onSeek = controller::seekTo,
            onChooseAudio = controller::chooseAudio,
            onChooseQuality = controller::chooseQuality,
            onPlayNext = controller::playNext,
            onPlayAgain = controller::playAgain,
            onRetry = controller::retry,
        ),
        chromeVisible = chromeVisible,
        onChromeVisibleChange = { chromeVisible = it },
        inPictureInPicture = inPictureInPicture,
        flexHinge = flexHinge,
    ) { modifier ->
        ContentFrame(
            player = model.engine.player,
            modifier = modifier.onGloballyPositioned { coordinates ->
                val bounds = coordinates.boundsInWindow().toAndroidRectF()
                videoBounds = Rect(bounds.left.toInt(), bounds.top.toInt(), bounds.right.toInt(), bounds.bottom.toInt())
            },
            surfaceType = SURFACE_TYPE_SURFACE_VIEW,
            contentScale = ContentScale.Fit,
            keepContentOnReset = true,
        )
    }
}
