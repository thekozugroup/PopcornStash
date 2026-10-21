package com.thekozugroup.popcornstash.feature.settings

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.SnackbarHostState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.activity.compose.BackHandler
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.Role
import com.thekozugroup.popcornstash.data.api.User
import com.thekozugroup.popcornstash.data.repo.StashData
import com.thekozugroup.popcornstash.navigation.PageStackHost
import com.thekozugroup.popcornstash.navigation.TabPage
import com.thekozugroup.popcornstash.navigation.ViewModelStores
import com.thekozugroup.popcornstash.navigation.rememberPageStack
import com.thekozugroup.popcornstash.ui.components.TabSnackbarHost
import com.thekozugroup.popcornstash.update.AppUpdates
import kotlinx.coroutines.launch
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/** The pages of the Settings tab: the first page, and the library editor pushed on top of it. */
@Serializable
sealed interface SettingsRoute {
    @Serializable
    @SerialName("settings")
    data object Root : SettingsRoute

    /** Editing one library. */
    @Serializable
    @SerialName("edit-library")
    data class EditLibrary(val id: String) : SettingsRoute

    /** Making a library; [folder] when it is made from a folder that is already there. */
    @Serializable
    @SerialName("new-library")
    data class NewLibrary(val folder: String? = null) : SettingsRoute
}

/**
 * What Settings needs from the signed-in session to manage libraries. [pages] is the session's
 * store of page view models (AppContainer.sessionUi), shared with Home: they survive a tab
 * switch and a rotation, end with their page, and all end with the person's session.
 */
class SettingsServices(val api: ApiClient, val data: StashData, val pages: ViewModelStores)

/**
 * Settings, the right tab (android/DESIGN.md, Settings and Library editor). Its first page shows
 * the person, this server, the owner's libraries and folders, the web settings pages and sign
 * out; the library editor opens on top of it and closes back to it. Without [services] (the
 * shell's sample pages) the libraries are left out.
 *
 * [updates] is the app's updater, shown as "This app" on the first page (left out without it).
 *
 * [open] is an editor another tab asked for (Home's "Edit library" and "Make a library"): it
 * opens over the first page, once, and then [onOpened] clears the request. [uploadsRunning]
 * is how many share sheet uploads signing out would stop.
 */
@Composable
fun SettingsTab(
    page: TabPage,
    user: User,
    serverDisplay: String,
    serverVersion: String?,
    onOpenWeb: (WebSettingsPage) -> Unit,
    onSignOut: () -> Unit,
    services: SettingsServices? = null,
    open: SettingsRoute? = null,
    updates: AppUpdates? = null,
    onOpened: () -> Unit = {},
    uploadsRunning: Int = 0,
) {
    val stack = rememberPageStack<SettingsRoute>(SettingsRoute.Root, SettingsRoute.serializer())
    val stores = services?.pages ?: remember { ViewModelStores() }
    val snackbar = remember { SnackbarHostState() }
    val scope = rememberCoroutineScope()
    val owner = user.role == Role.Owner && services != null
    val show: (String) -> Unit = { message -> scope.launch { snackbar.showSnackbar(message) } }

    // Tapping Settings again closes the editor (the first page then scrolls itself to the top).
    var handledReselections by rememberSaveable { mutableIntStateOf(page.reselections) }
    LaunchedEffect(page.reselections) {
        if (page.reselections != handledReselections) {
            handledReselections = page.reselections
            stack.popToRoot()
        }
    }
    // Someone who stops being an owner leaves the editor.
    LaunchedEffect(owner) { if (!owner) stack.popToRoot() }
    // An editor another tab asked for replaces any editor already open; members get the first page.
    LaunchedEffect(open) {
        val route = open ?: return@LaunchedEffect
        if (stack.top.route != route) {
            stack.popToRoot()
            if (owner && route != SettingsRoute.Root) stack.push(route)
        }
        onOpened()
    }

    Box(Modifier.fillMaxSize()) {
        PageStackHost(stack, stores, name = "settings") { entry ->
            when (val route = entry.route) {
                SettingsRoute.Root -> SettingsFirstPage(
                    page = page,
                    user = user,
                    serverDisplay = serverDisplay,
                    serverVersion = serverVersion,
                    services = services?.takeIf { owner },
                    onOpenWeb = onOpenWeb,
                    onSignOut = onSignOut,
                    uploadsRunning = uploadsRunning,
                    updates = updates,
                    onOpenEditor = { stack.push(it) },
                    onNotice = show,
                )
                is SettingsRoute.EditLibrary, is SettingsRoute.NewLibrary -> if (services != null) {
                    LibraryEditorRoute(
                        route = route,
                        services = services,
                        page = page,
                        onBack = { stack.pop() },
                        onFinished = { message ->
                            stack.pop()
                            message?.let(show)
                        },
                    )
                }
            }
        }
        TabSnackbarHost(snackbar, page)
    }
}

@Composable
private fun SettingsFirstPage(
    page: TabPage,
    user: User,
    serverDisplay: String,
    serverVersion: String?,
    services: SettingsServices?,
    onOpenWeb: (WebSettingsPage) -> Unit,
    onSignOut: () -> Unit,
    uploadsRunning: Int,
    updates: AppUpdates?,
    onOpenEditor: (SettingsRoute) -> Unit,
    onNotice: (String) -> Unit,
) {
    val listState = rememberLazyListState()
    val (appUpdates, updateActions) = updates?.let { rememberAppUpdateSection(it) } ?: (null to AppUpdateActions())
    if (services == null) {
        SettingsRootPage(
            page,
            user,
            serverDisplay,
            serverVersion,
            libraries = null,
            onOpenWeb = onOpenWeb,
            onSignOut = onSignOut,
            listState = listState,
            uploadsRunning = uploadsRunning,
            appUpdates = appUpdates,
            updateActions = updateActions,
        )
        return
    }
    val model: LibrarySettingsViewModel = viewModel(
        key = "library settings",
        factory = viewModelFactory { initializer { LibrarySettingsViewModel(services.api, services.data) } },
    )
    val controller = model.controller
    val state by controller.state.collectAsStateWithLifecycle()
    // Fresh each time the page shows: another device may have changed the libraries.
    LaunchedEffect(controller) { controller.refresh() }
    LaunchedEffect(controller) { controller.notices.collect(onNotice) }
    SettingsRootPage(
        page = page,
        user = user,
        serverDisplay = serverDisplay,
        serverVersion = serverVersion,
        libraries = state,
        onOpenWeb = onOpenWeb,
        onSignOut = onSignOut,
        listState = listState,
        uploadsRunning = uploadsRunning,
        appUpdates = appUpdates,
        updateActions = updateActions,
        actions = LibrarySettingsActions(
            onRetry = controller::refresh,
            onOpen = { onOpenEditor(SettingsRoute.EditLibrary(it.id)) },
            onNew = { folder -> onOpenEditor(SettingsRoute.NewLibrary(folder)) },
            onMove = { library, step -> controller.move(library.id, step) },
            onDelete = controller::delete,
        ),
    )
}

@Composable
private fun LibraryEditorRoute(
    route: SettingsRoute,
    services: SettingsServices,
    page: TabPage,
    onBack: () -> Unit,
    onFinished: (String?) -> Unit,
) {
    val id = (route as? SettingsRoute.EditLibrary)?.id
    val folder = (route as? SettingsRoute.NewLibrary)?.folder
    val model: LibraryEditorViewModel = viewModel(
        key = "library editor",
        factory = viewModelFactory { initializer { LibraryEditorViewModel(services.api, services.data, id, folder) } },
    )
    val controller = model.controller
    val state by controller.state.collectAsStateWithLifecycle()
    LaunchedEffect(state.finished) { state.finished?.let { onFinished(it.message) } }
    var conversion by rememberSaveable(id) { mutableStateOf(false) }
    if (conversion && id != null) {
        BackHandler { conversion = false }
        LibraryConversionRoute(services.api, id, { conversion = false }, page.bottomClearance)
    } else {
        LibraryEditorPage(state = state, actions = controller, onBack = onBack, bottomClearance = page.bottomClearance, onConversion = { conversion = true })
    }
}
