package com.thekozugroup.popcornstash.feature.home

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Snackbar
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import com.thekozugroup.popcornstash.data.api.MediaKind
import com.thekozugroup.popcornstash.data.api.Role
import com.thekozugroup.popcornstash.data.api.User
import com.thekozugroup.popcornstash.data.repo.Loadable
import com.thekozugroup.popcornstash.feature.library.LibraryServices
import com.thekozugroup.popcornstash.feature.library.displayTitle
import com.thekozugroup.popcornstash.feature.photos.PhotoViewerOverlay
import com.thekozugroup.popcornstash.navigation.PageStackHost
import com.thekozugroup.popcornstash.navigation.TabPage
import com.thekozugroup.popcornstash.navigation.rememberPageStack
import com.thekozugroup.popcornstash.ui.theme.Popcorn

/**
 * Home, the start tab: Home itself, and the pages it opens (a library, a show, an item,
 * Other files), each on top of the last with its own state (android/DESIGN.md, Tab
 * transitions). Tapping Home again closes every page above Home, or scrolls Home to the top.
 * A library that the person no longer sees closes, with its pages, and a snackbar says so.
 *
 * [onPlay] hands an item to the player; [onManageLibrary] opens a library's settings (owners).
 * [open] is a page another tab asked for (a finished download's library): it opens on top of
 * whatever Home shows, once, and then [onOpened] clears the request.
 */
@Composable
fun HomeTab(
    page: TabPage,
    services: LibraryServices,
    user: User,
    onPlay: (itemId: String, fromStart: Boolean) -> Unit,
    onManageLibrary: (libraryId: String?) -> Unit,
    open: HomeRoute? = null,
    onOpened: () -> Unit = {},
) {
    val stack = rememberPageStack<HomeRoute>(HomeRoute.Root, HomeRoute.serializer())
    val snackbar = remember { SnackbarHostState() }
    val scope = rememberCoroutineScope()
    val play by rememberUpdatedState(onPlay)
    val manage by rememberUpdatedState(onManageLibrary)
    val nav = remember(stack, snackbar) {
        HomeNavigator(stack, scope, snackbar, play = { id, fromStart -> play(id, fromStart) }, manageLibrary = { manage(it) })
    }
    val isOwner = user.role == Role.Owner

    // Tapping Home while on it: close the pages above Home, or scroll Home to the top.
    var handled by rememberSaveable { mutableIntStateOf(page.reselections) }
    var rootReselections by rememberSaveable { mutableIntStateOf(0) }
    LaunchedEffect(page.reselections) {
        if (page.reselections != handled) {
            handled = page.reselections
            if (!stack.popToRoot()) rootReselections++
        }
    }

    // A page another tab asked for, unless it is already the one showing.
    LaunchedEffect(open) {
        val route = open ?: return@LaunchedEffect
        if (route == HomeRoute.Root) stack.popToRoot() else if (stack.top.route != route) stack.push(route)
        onOpened()
    }

    // Libraries the person stops seeing (deleted, or no longer shared) close with their pages.
    LaunchedEffect(services) { services.data.refreshLibraries() }
    val libraries by services.data.libraries.collectAsStateWithLifecycle()
    LaunchedEffect(libraries) {
        val visible = (libraries as? Loadable.Ready)?.value?.map { it.id }?.toSet() ?: return@LaunchedEffect
        val lost = stack.entries.firstOrNull { entry -> entry.route.libraryId?.let { it !in visible } == true }?.route
            ?: return@LaunchedEffect
        nav.closeGone(lost, "${lost.libraryName} is not available any more.")
    }

    Box(Modifier.fillMaxSize()) {
        PageStackHost(stack, services.pages, name = "home") { entry ->
            when (val route = entry.route) {
                HomeRoute.Root -> HomeRootPage(page.copy(reselections = rootReselections), services, isOwner, nav)
                is HomeRoute.Library -> LibraryPage(route, page.bottomClearance, services, isOwner, nav)
                is HomeRoute.Show -> ShowPage(route, page.bottomClearance, services, nav)
                is HomeRoute.Item -> ItemPage(route, page.bottomClearance, services, nav)
                HomeRoute.OtherFiles -> OtherFilesPage(page.bottomClearance, services, isOwner, nav)
            }
        }
        SnackbarHost(
            snackbar,
            modifier = Modifier
                .align(Alignment.BottomCenter)
                .padding(bottom = page.bottomClearance - 8.dp),
        ) { data ->
            Snackbar(data, shape = Popcorn.shapes.control)
        }
    }
}

private val HomeRoute.libraryName: String
    get() = when (this) {
        is HomeRoute.Library -> name
        is HomeRoute.Show -> libraryName
        else -> "This library"
    }

/** Home itself, with the photo viewer over it when a photo on a shelf is open. */
@Composable
private fun HomeRootPage(page: TabPage, services: LibraryServices, isOwner: Boolean, nav: HomeNavigator) {
    val model: HomeViewModel = viewModel(
        factory = viewModelFactory { initializer { HomeViewModel(services.data, services.reconnects, services.connectionLost) } },
    )
    val home by model.home.collectAsStateWithLifecycle()
    val connectionLost by model.connectionLost.collectAsStateWithLifecycle()
    OnReturn { model.refresh() }
    // The open photo: its library and its id, so it survives a rotation.
    var viewer by rememberSaveable { mutableStateOf<Pair<String, String>?>(null) }
    val format = services.format
    HomeScreen(
        page = page,
        home = home,
        isOwner = isOwner,
        connectionLost = connectionLost,
        format = format,
        actions = HomeActions(
            resume = { nav.play(it.id, false) },
            startOver = { nav.play(it.id, true) },
            openItem = { nav.open(HomeRoute.Item(it.id, format.displayTitle(it))) },
            openLibrary = { nav.open(HomeRoute.Library(it.id, it.name, it.icon)) },
            openShow = { library, show -> nav.open(HomeRoute.Show(library.id, library.name, library.icon, show)) },
            openPhoto = { library, item ->
                if (item.kind == MediaKind.Video) nav.play(item.id, false) else viewer = library.id to item.id
            },
            openOtherFiles = { nav.open(HomeRoute.OtherFiles) },
            makeLibrary = { nav.manageLibrary(null) },
            retry = model::refresh,
        ),
    )
    val open = viewer
    if (open != null) {
        val photos = home.valueOrNull?.libraries?.firstOrNull { it.library.id == open.first }?.recent.orEmpty()
        PhotoViewerOverlay(
            photos = photos,
            startId = open.second,
            services = services,
            onClose = { viewer = null },
            onDetails = { nav.open(HomeRoute.Item(it.id, format.displayTitle(it))) },
            onPlay = { nav.play(it.id, false) },
            onMessage = nav::message,
        )
    }
}

/**
 * Runs [action] each time the app comes back to this page from elsewhere (the player, another
 * app), not when the page first shows.
 */
@Composable
fun OnReturn(action: () -> Unit) {
    val current by rememberUpdatedState(action)
    var stopped by remember { mutableStateOf(false) }
    LifecycleEventEffect(Lifecycle.Event.ON_STOP) { stopped = true }
    LifecycleEventEffect(Lifecycle.Event.ON_START) {
        if (stopped) {
            stopped = false
            current()
        }
    }
}
