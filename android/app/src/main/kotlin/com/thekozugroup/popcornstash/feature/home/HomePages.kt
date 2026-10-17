package com.thekozugroup.popcornstash.feature.home

import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.Dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import com.thekozugroup.popcornstash.data.api.MediaKind
import com.thekozugroup.popcornstash.feature.item.ItemActions
import com.thekozugroup.popcornstash.feature.item.ItemScreen
import com.thekozugroup.popcornstash.feature.item.ItemViewModel
import com.thekozugroup.popcornstash.feature.library.LibraryActions
import com.thekozugroup.popcornstash.feature.library.LibraryScreen
import com.thekozugroup.popcornstash.feature.library.LibraryServices
import com.thekozugroup.popcornstash.feature.library.LibraryViewModel
import com.thekozugroup.popcornstash.feature.library.OtherFilesActions
import com.thekozugroup.popcornstash.feature.library.OtherFilesScreen
import com.thekozugroup.popcornstash.feature.library.OtherFilesViewModel
import com.thekozugroup.popcornstash.feature.library.ShowActions
import com.thekozugroup.popcornstash.feature.library.ShowScreen
import com.thekozugroup.popcornstash.feature.library.ShowViewModel
import com.thekozugroup.popcornstash.feature.library.displayTitle
import com.thekozugroup.popcornstash.feature.library.openInAnotherApp
import com.thekozugroup.popcornstash.feature.photos.PhotoViewerOverlay
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** A library page, with the photo viewer over it when a photo of its timeline is open. */
@Composable
internal fun LibraryPage(route: HomeRoute.Library, bottomClearance: Dp, services: LibraryServices, isOwner: Boolean, nav: HomeNavigator) {
    val model: LibraryViewModel = viewModel(
        factory = viewModelFactory {
            initializer {
                val seed = services.data.home.value.valueOrNull?.libraries?.firstOrNull { it.library.id == route.id }?.library
                LibraryViewModel(services.api, services.events, services.reconnects, route.id, seed)
            }
        },
    )
    val ui by model.ui.collectAsStateWithLifecycle()
    LaunchedEffect(ui.gone) { if (ui.gone) nav.closeGone(route, "${route.name} is not available any more.") }
    var viewer by rememberSaveable { mutableStateOf<String?>(null) }
    val format = services.format
    LibraryScreen(
        name = route.name,
        icon = route.icon,
        ui = ui,
        isOwner = isOwner,
        bottomClearance = bottomClearance,
        format = format,
        actions = LibraryActions(
            back = nav::back,
            search = { nav.open(HomeRoute.Files(route.id, route.name, false)) },
            folders = { nav.open(HomeRoute.Files(route.id, route.name, true)) },
            retry = model::retry,
            loadMore = model::loadMore,
            sort = model::setSort,
            openItem = { nav.open(HomeRoute.Item(it.id, format.displayTitle(it))) },
            openShow = { show -> nav.open(HomeRoute.Show(route.id, ui.library?.name ?: route.name, ui.library?.icon ?: route.icon, show)) },
            openPhoto = { viewer = it.id },
            play = { nav.play(it.id, false) },
            edit = { nav.manageLibrary(route.id) },
        ),
    )
    val open = viewer
    val timeline = ui.content as? LibraryViewModel.Content.Timeline
    if (open != null && timeline != null) {
        PhotoViewerOverlay(
            photos = timeline.photos,
            startId = open,
            services = services,
            onClose = { viewer = null },
            onDetails = { nav.open(HomeRoute.Item(it.id, format.displayTitle(it))) },
            onPlay = { nav.play(it.id, false) },
            onNearEnd = model::loadMore,
        )
    }
}

/** A show page. */
@Composable
internal fun ShowPage(route: HomeRoute.Show, bottomClearance: Dp, services: LibraryServices, nav: HomeNavigator) {
    val model: ShowViewModel = viewModel(
        factory = viewModelFactory {
            initializer { ShowViewModel(services.api, services.events, services.reconnects, route.libraryId, route.name) }
        },
    )
    val ui by model.ui.collectAsStateWithLifecycle()
    LaunchedEffect(ui.gone) { if (ui.gone) nav.closeGone(route, "${route.name} is not in ${route.libraryName} any more.") }
    OnReturn { model.refresh() }
    val format = services.format
    ShowScreen(
        name = route.name,
        icon = route.icon,
        ui = ui,
        bottomClearance = bottomClearance,
        format = format,
        actions = ShowActions(
            back = nav::back,
            retry = model::retry,
            selectSeason = model::selectSeason,
            play = { episode, fromStart -> nav.play(episode.id, fromStart) },
            openEpisode = { nav.open(HomeRoute.Item(it.id, format.displayTitle(it))) },
        ),
    )
}

/** An item page; a photo's picture opens the viewer on that photo alone. */
@Composable
internal fun ItemPage(route: HomeRoute.Item, bottomClearance: Dp, services: LibraryServices, nav: HomeNavigator) {
    val model: ItemViewModel = viewModel(
        factory = viewModelFactory { initializer { ItemViewModel(services.api, services.events, services.reconnects, route.id) } },
    )
    val ui by model.ui.collectAsStateWithLifecycle()
    LaunchedEffect(model) { model.messages.collect(nav::message) }
    LaunchedEffect(ui.gone) { if (ui.gone) nav.closeGone(route, "That file is not available any more.") }
    OnReturn { model.refresh() }
    var viewing by rememberSaveable { mutableStateOf(false) }
    val format = services.format
    val detail = ui.item.valueOrNull
    val home by services.data.home.collectAsStateWithLifecycle()
    val icon = detail?.libraryId?.let { id -> home.valueOrNull?.libraries?.firstOrNull { it.library.id == id }?.library?.icon }
    ItemScreen(
        title = route.title,
        ui = ui,
        bottomClearance = bottomClearance,
        libraryIcon = icon,
        format = format,
        actions = ItemActions(
            back = nav::back,
            retry = model::retry,
            play = { fromStart -> nav.play(route.id, fromStart) },
            toggleFavourite = model::toggleFavourite,
            openNext = { nav.open(HomeRoute.Item(it.id, format.displayTitle(it))) },
            viewPhoto = { viewing = true },
        ),
    )
    if (viewing && detail != null && detail.kind == MediaKind.Photo) {
        PhotoViewerOverlay(
            photos = listOf(detail.item),
            startId = detail.id,
            services = services,
            onClose = { viewing = false },
            onDetails = { viewing = false },
            onPlay = {},
        )
    }
}

/**
 * Other files. A tapped file is fetched into a private copy and handed to another app; the
 * copy is deleted when the person comes back (and at the next start, and on sign-out).
 */
@Composable
internal fun OtherFilesPage(bottomClearance: Dp, services: LibraryServices, isOwner: Boolean, nav: HomeNavigator) {
    val model: OtherFilesViewModel = viewModel(
        factory = viewModelFactory {
            initializer { OtherFilesViewModel(services.api, services.events, services.reconnects, services.files, services.openedDir) }
        },
    )
    val ui by model.ui.collectAsStateWithLifecycle()
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var handedOver by rememberSaveable { mutableStateOf(false) }
    LaunchedEffect(model) { model.messages.collect(nav::message) }
    LaunchedEffect(model) {
        model.opened.collect { opened ->
            if (openInAnotherApp(context, opened)) {
                handedOver = true
            } else {
                withContext(Dispatchers.IO) { opened.file.parentFile?.deleteRecursively() }
                nav.message("No app on this phone opens this kind of file.")
            }
        }
    }
    LifecycleEventEffect(Lifecycle.Event.ON_RESUME) {
        if (handedOver) {
            handedOver = false
            scope.launch(Dispatchers.IO) { services.openedDir.deleteRecursively() }
        }
    }
    OtherFilesScreen(
        ui = ui,
        isOwner = isOwner,
        bottomClearance = bottomClearance,
        format = services.format,
        actions = OtherFilesActions(
            back = nav::back,
            retry = model::retry,
            loadMore = model::loadMore,
            sort = model::setSort,
            open = model::open,
        ),
    )
}
