package com.thekozugroup.popcornstash.qa

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.*
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.media3.common.MediaItem
import androidx.media3.common.Player
import androidx.media3.common.util.UnstableApi
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.DefaultRenderersFactory
import androidx.media3.exoplayer.mediacodec.MediaCodecUtil
import androidx.media3.ui.compose.ContentFrame
import androidx.window.layout.WindowInfoTracker
import com.thekozugroup.popcornstash.R
import com.thekozugroup.popcornstash.feature.player.*
import com.thekozugroup.popcornstash.navigation.AppShell
import com.thekozugroup.popcornstash.ui.components.PrimaryButton
import com.thekozugroup.popcornstash.ui.theme.Popcorn
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.onEach

/** Debug-only native layout fixture. No account, token, server or family data is read. */
@androidx.annotation.OptIn(UnstableApi::class)
class VisualQaActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            val context = LocalContext.current
            val player = remember { ExoPlayer.Builder(context,
                DefaultRenderersFactory(context).apply {
                    // Optional emulator workaround; production keeps its real device decoder policy.
                    if (intent.getBooleanExtra("software_decode", false)) setMediaCodecSelector { mime, secure, tunneled ->
                        MediaCodecUtil.getDecoderInfos(mime, secure, tunneled).filter { it.softwareOnly }
                    }
                }).build().apply {
                setMediaItem(MediaItem.fromUri("android.resource://${context.packageName}/${R.raw.qa_clip}"))
                repeatMode = Player.REPEAT_MODE_ONE
                prepare()
            } }
            DisposableEffect(player) { onDispose { player.release() } }
            var showPlayer by remember { mutableStateOf(intent.getBooleanExtra("player", false)) }
            var playing by remember { mutableStateOf(false) }
            var position by remember { mutableLongStateOf(0) }
            var chrome by remember { mutableStateOf(true) }
            LaunchedEffect(player) { while (true) { position = player.currentPosition; delay(250) } }
            val flow = remember { WindowInfoTracker.getOrCreate(this).windowLayoutInfo(this)
                .onEach { android.util.Log.i("PopcornLayoutQa", it.toString()) } }
            val layout by flow.collectAsStateWithLifecycle(initialValue = null)
            if (showPlayer) PlayerTheme {
                PlayerScreen(
                    PlayerUiState(title = "Playback preview", phase = PlayerPhase.Ready,
                        playWhenReady = playing, durationMs = 12000, videoWidth = 640, videoHeight = 360,
                        recommendations = listOf(com.thekozugroup.popcornstash.data.api.Item(id = "qa-next", title = "Paper Boats", kind = com.thekozugroup.popcornstash.data.api.MediaKind.Video, durationS = 6.0))),
                    { PlayerClock(position, 12000) },
                    PlayerActions(onClose = { player.pause(); playing = false; showPlayer = false },
                        onPlayVideo = { android.util.Log.i("PopcornPlayerQa", "Selected $it"); player.seekTo(0) },
                        onTogglePlay = { playing = !playing; player.playWhenReady = playing },
                        onSeek = { target -> player.seekTo(target); android.util.Log.i("PopcornPlayerQa", "Seek target=$target position=${player.currentPosition} playing=${player.playWhenReady}") }),
                    chromeVisible = chrome, onChromeVisibleChange = { chrome = it }, flexHinge = layout?.flexHinge(),
                ) { ContentFrame(player = player, modifier = it) }
            } else if (intent.getBooleanExtra("conversion", false)) PopcornTheme {
                val policy = com.thekozugroup.popcornstash.data.api.ConversionPolicy()
                var draft by remember { mutableStateOf(policy) }
                com.thekozugroup.popcornstash.feature.settings.LibraryConversionPage(
                    com.thekozugroup.popcornstash.feature.settings.ConversionUi(com.thekozugroup.popcornstash.data.api.LibraryConversion(policy), draft),
                    { draft = it }, {}, {}, { finish() })
            } else if (intent.getBooleanExtra("torrent_files", false)) {
                TorrentReviewPreview()
            } else if (intent.getBooleanExtra("torrent_rules", false)) PopcornTheme {
                var add by remember { mutableStateOf(com.thekozugroup.popcornstash.feature.downloads.AddLinkState(
                    text = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567")) }
                Box(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing)) {
                com.thekozugroup.popcornstash.feature.downloads.SheetBody("torrent rules preview") {
                    com.thekozugroup.popcornstash.feature.downloads.AddLinkContent(add, viaVpn = true,
                        onText = { add = add.copy(text = it) }, onAdd = {}, onCancel = { finish() },
                        onFileType = { kind -> add = add.copy(fileTypes = if (kind in add.fileTypes) add.fileTypes - kind else add.fileTypes + kind) },
                        onRules = { add = add.copy(rules = it) })
                }
                }
            } else PopcornTheme {
                AppShell { page ->
                    Column(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing).padding(Popcorn.spacing.l)) {
                        Text(page.tab.label, style = Popcorn.type.headline, color = Popcorn.colors.text)
                        Spacer(Modifier.height(Popcorn.spacing.l))
                        Text("Native layout preview", style = Popcorn.type.title, color = Popcorn.colors.text)
                        Spacer(Modifier.height(Popcorn.spacing.s))
                        Text("Phone, tablet and foldable navigation share the same controls.", style = Popcorn.type.body, color = Popcorn.colors.textMuted)
                        Spacer(Modifier.height(Popcorn.spacing.l))
                        PrimaryButton("Open playback preview", onClick = { showPlayer = true })
                    }
                }
            }
        }
    }
}

/** Local manifest fixture exercises the real controller and sheet, without network access. */
@Composable
private fun TorrentReviewPreview() {
    val scope = rememberCoroutineScope()
    var started by remember { mutableStateOf(false) }
    val controller = remember {
        com.thekozugroup.popcornstash.feature.downloads.TorrentFilesController(
            load = { com.thekozugroup.popcornstash.data.api.TorrentFiles(ready = true, reviewRequired = true, files = listOf(
                com.thekozugroup.popcornstash.data.api.TorrentFile(2, "Jane.Doe/Feature.mkv", 900_000_000L, com.thekozugroup.popcornstash.data.api.TorrentFileKind.Video, true),
                com.thekozugroup.popcornstash.data.api.TorrentFile(5, "Jane.Doe/English.srt", 54_000L, com.thekozugroup.popcornstash.data.api.TorrentFileKind.Subtitle, true),
                com.thekozugroup.popcornstash.data.api.TorrentFile(8, "Jane.Doe/trailer.mkv", 100_000_000L, com.thekozugroup.popcornstash.data.api.TorrentFileKind.Video, true),
                com.thekozugroup.popcornstash.data.api.TorrentFile(9, "Jane.Doe/poster.jpg", 430_000L, com.thekozugroup.popcornstash.data.api.TorrentFileKind.Image, false),
            )) },
            save = { _, _ -> }, scope = scope, onSaved = { started = true },
            start = { _, indices -> android.util.Log.i("PopcornTorrentQa", "Started indices=$indices") },
        )
    }
    LaunchedEffect(controller) { controller.open(com.thekozugroup.popcornstash.data.api.Download(id = "preview", kind = com.thekozugroup.popcornstash.data.api.DownloadKind.Torrent,
        name = "Example collection", state = com.thekozugroup.popcornstash.data.api.DownloadState.Paused)) }
    val state by controller.state.collectAsStateWithLifecycle()
    PopcornTheme {
        Box(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing)) {
            Text(if (started) "Download started" else "Torrent preview", color = Popcorn.colors.text)
            state?.let {
                com.thekozugroup.popcornstash.feature.downloads.TorrentFilesSheet(it, controller::toggle, controller::submit, controller::close, controller::refresh,
                    onDraft = controller::draft, onType = controller::type, onApply = controller::applyFilters)
            }
        }
    }
}
