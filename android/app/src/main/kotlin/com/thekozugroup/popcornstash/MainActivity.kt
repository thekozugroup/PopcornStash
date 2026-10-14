package com.thekozugroup.popcornstash

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.browser.customtabs.CustomTabsIntent
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
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
import com.thekozugroup.popcornstash.feature.settings.SettingsTab
import com.thekozugroup.popcornstash.feature.signin.SignInFlow
import com.thekozugroup.popcornstash.feature.signin.SignInViewModel
import com.thekozugroup.popcornstash.navigation.AppShell
import com.thekozugroup.popcornstash.navigation.OpenTab
import com.thekozugroup.popcornstash.navigation.Tab
import com.thekozugroup.popcornstash.share.UploadQueue
import com.thekozugroup.popcornstash.ui.theme.Popcorn
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import kotlinx.coroutines.launch

class MainActivity : ComponentActivity() {
    /** A tab asked for by a notification or the share sheet ([OpenTab]), until the shell shows it. */
    private var requestedTab by mutableStateOf<Tab?>(null)

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        if (savedInstanceState == null) requestedTab = OpenTab.from(intent)
        val container = (application as PopcornStashApplication).container
        setContent {
            PopcornTheme {
                PopcornStashApp(container, this, requestedTab, onOpenedTab = { requestedTab = null })
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        OpenTab.from(intent)?.let { requestedTab = it }
    }
}

/** Sign-in while nobody is signed in; the three tabs once someone is. */
@Composable
fun PopcornStashApp(container: AppContainer, activity: Activity, openTab: Tab? = null, onOpenedTab: () -> Unit = {}) {
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
                uploads = container.uploads,
                openUrl = openUrl,
                onSignOut = { scope.launch { container.sessions.signOut() } },
                openTab = openTab,
                onOpenedTab = onOpenedTab,
            )
        }
    }
}

/** The three tabs for [session]; everything here is made again for the next person. */
@Composable
fun SignedInApp(
    session: Session,
    uploads: UploadQueue,
    openUrl: (String) -> Unit,
    onSignOut: () -> Unit,
    openTab: Tab? = null,
    onOpenedTab: () -> Unit = {},
) {
    val user by session.user.collectAsStateWithLifecycle()
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
    AppShell(readyToImport = readyToImport.toInt(), openTab = openTab, onOpenedTab = onOpenedTab) { page ->
        when (page.tab) {
            Tab.Downloads -> DownloadsTab(page, session, uploads, openUrl)
            Tab.Home -> HomeTab(page)
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
