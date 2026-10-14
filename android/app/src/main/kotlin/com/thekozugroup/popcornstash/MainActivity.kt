package com.thekozugroup.popcornstash

import android.app.Activity
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.browser.customtabs.CustomTabsIntent
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.core.net.toUri
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import com.thekozugroup.popcornstash.data.api.ApiException
import com.thekozugroup.popcornstash.data.auth.CredentialManagerAuthenticator
import com.thekozugroup.popcornstash.data.auth.ServerAddress
import com.thekozugroup.popcornstash.data.session.Session
import com.thekozugroup.popcornstash.data.session.SessionState
import com.thekozugroup.popcornstash.feature.downloads.DownloadsTab
import com.thekozugroup.popcornstash.feature.home.HomeTab
import com.thekozugroup.popcornstash.feature.library.FileFetcher
import com.thekozugroup.popcornstash.feature.library.LibraryServices
import com.thekozugroup.popcornstash.feature.library.connectionLost
import com.thekozugroup.popcornstash.feature.library.reconnects
import com.thekozugroup.popcornstash.feature.settings.SettingsTab
import com.thekozugroup.popcornstash.feature.signin.SignInFlow
import com.thekozugroup.popcornstash.feature.signin.SignInViewModel
import com.thekozugroup.popcornstash.navigation.AppShell
import com.thekozugroup.popcornstash.navigation.Tab
import com.thekozugroup.popcornstash.ui.media.LocalPictures
import com.thekozugroup.popcornstash.ui.media.LocalStashImages
import com.thekozugroup.popcornstash.ui.media.Pictures
import com.thekozugroup.popcornstash.ui.theme.Popcorn
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import kotlinx.coroutines.launch
import java.io.File

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        val container = (application as PopcornStashApplication).container
        setContent {
            PopcornTheme {
                PopcornStashApp(container, this)
            }
        }
    }
}

/** Sign-in while nobody is signed in; the three tabs once someone is. */
@Composable
fun PopcornStashApp(container: AppContainer, activity: Activity) {
    val state by container.sessions.state.collectAsStateWithLifecycle()
    val context = LocalContext.current
    val openUrl: (String) -> Unit = { url ->
        // The browser has its own sign-in; the app's token is never handed to it.
        CustomTabsIntent.Builder().setShowTitle(true).build().launchUrl(context, url.toUri())
    }
    when (val current = state) {
        SessionState.Restoring -> Box(Modifier.fillMaxSize().background(Popcorn.colors.page))
        is SessionState.SignedOut -> {
            val model: SignInViewModel = viewModel(
                key = "sign-in ${current.generation}",
                factory = viewModelFactory {
                    initializer { SignInViewModel(container.signInDependencies(), current.lastServer, current.notice) }
                },
            )
            SignInFlow(model.controller, openUrl, passkeys = { CredentialManagerAuthenticator(activity) })
        }
        is SessionState.SignedIn -> key(current.session.id) {
            val scope = rememberCoroutineScope()
            SignedInApp(
                session = current.session,
                ui = container.sessionUi(current.session),
                openUrl = openUrl,
                onSignOut = { scope.launch { container.sessions.signOut() } },
            )
        }
    }
}

/** The three tabs for [session]; everything here is made again for the next person. */
@Composable
fun SignedInApp(session: Session, ui: SessionUi, openUrl: (String) -> Unit, onSignOut: () -> Unit) {
    val user by session.user.collectAsStateWithLifecycle()
    val context = LocalContext.current
    val library = remember(session, ui) {
        LibraryServices(
            api = session.api,
            data = session.data,
            events = session.events.events,
            reconnects = session.events.status.reconnects(),
            files = FileFetcher(session.http),
            openedDir = File(context.cacheDir, "opened"),
            pages = ui.pages,
            connectionLost = session.events.status.connectionLost(),
        )
    }
    val readyToImport by session.data.readyToImport.collectAsStateWithLifecycle()
    var version by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(session) {
        session.data.refreshHome()
        version = try {
            session.api.health().version
        } catch (_: ApiException) {
            null
        }
    }
    val pictures = remember(session) { Pictures(session.server) }
    fun webPage(vararg segments: String) =
        session.server.newBuilder().apply { segments.forEach { addPathSegment(it) } }.build().toString()
    CompositionLocalProvider(LocalStashImages provides ui.images, LocalPictures provides pictures) {
        AppShell(readyToImport = readyToImport.toInt()) { page ->
            when (page.tab) {
                Tab.Downloads -> DownloadsTab(page)
                Tab.Home -> HomeTab(
                    page = page,
                    services = library,
                    user = user,
                    // The player plugs in here; until it does, the web app plays the item.
                    onPlay = { itemId, _ -> openUrl(webPage("item", itemId)) },
                    // Library settings open on the web until Settings manages libraries in the app.
                    onManageLibrary = { id ->
                        openUrl(if (id == null) webPage("settings", "libraries", "new") else webPage("settings", "libraries", id))
                    },
                )
                Tab.Settings -> SettingsTab(
                    page = page,
                    user = user,
                    serverDisplay = ServerAddress.display(session.server),
                    serverVersion = version,
                    onOpenWeb = { web -> openUrl(session.api.url("/${web.path}").toString()) },
                    onSignOut = onSignOut,
                )
            }
        }
    }
}
