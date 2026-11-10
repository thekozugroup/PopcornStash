package com.thekozugroup.popcornstash.feature.home

import androidx.activity.compose.BackHandler
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.saveable.rememberSaveableStateHolder
import androidx.compose.runtime.setValue
import androidx.compose.ui.unit.Dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import com.thekozugroup.popcornstash.data.api.Library
import com.thekozugroup.popcornstash.feature.library.DownloadFoldersScreen
import com.thekozugroup.popcornstash.feature.library.LibraryActions
import com.thekozugroup.popcornstash.feature.library.LibraryFeedActions
import com.thekozugroup.popcornstash.feature.library.LibraryFeedScreen
import com.thekozugroup.popcornstash.feature.library.LibraryFeedViewModel
import com.thekozugroup.popcornstash.feature.library.LibraryScreen
import com.thekozugroup.popcornstash.feature.library.LibraryServices
import com.thekozugroup.popcornstash.feature.library.LibraryViewModel
import com.thekozugroup.popcornstash.feature.library.VideoLibraryMenu
import com.thekozugroup.popcornstash.feature.library.VideoLibraryView
import com.thekozugroup.popcornstash.feature.library.displayTitle

/** View choice, collection and each view's scroll/search state survive recreation and Back. */
@Composable
internal fun VideoLibraryPage(route: HomeRoute.Library, library: Library, bottomClearance: Dp, services: LibraryServices, isOwner: Boolean, nav: HomeNavigator) {
    var view by rememberSaveable(route.id) { mutableStateOf(VideoLibraryView.Feed) }
    var collectionKey by rememberSaveable(route.id) { mutableStateOf<String?>(null) }
    var collectionName by rememberSaveable(route.id) { mutableStateOf("") }
    val states = rememberSaveableStateHolder()
    val format = services.format
    fun showView(next: VideoLibraryView) { collectionKey = null; view = next }
    fun back() {
        when {
            collectionKey != null -> collectionKey = null
            view != VideoLibraryView.Feed -> view = VideoLibraryView.Feed
            else -> nav.back()
        }
    }
    fun gone() = nav.closeGone(route, "${library.name} is not available any more.")
    fun open(item: com.thekozugroup.popcornstash.data.api.Item) = nav.open(HomeRoute.Item(item.id, format.displayTitle(item)))
    fun files() = nav.open(HomeRoute.Files(route.id, library.name, true))
    BackHandler(view != VideoLibraryView.Feed || collectionKey != null) { back() }
    val key = collectionKey
    states.SaveableStateProvider(if (key != null) "collection:$key" else view.name) {
        when {
            key != null || view == VideoLibraryView.Folders -> DownloadFoldersScreen(
                route.id, if (key != null) collectionName else library.name, services, isOwner, bottomClearance,
                onBack = ::back, onAllVideos = { showView(VideoLibraryView.All) }, onFiles = ::files, onOpen = ::open,
                initialKey = key.orEmpty(), initialCollapsed = key != null,
                onFolderView = if (key != null) ({ showView(VideoLibraryView.Folders) }) else null,
                onGone = ::gone, viewActions = { VideoLibraryMenu(if (key != null) VideoLibraryView.Feed else view, ::showView) },
            )
            view == VideoLibraryView.Feed -> {
                val model: LibraryFeedViewModel = viewModel(key = "video-feed", factory = viewModelFactory {
                    initializer { LibraryFeedViewModel(services.api, services.events, services.reconnects, route.id) }
                })
                val ui by model.ui.collectAsStateWithLifecycle()
                val lost by services.connectionLost.collectAsStateWithLifecycle(false)
                LaunchedEffect(ui.gone) { if (ui.gone) gone() }
                LaunchedEffect(model) { if (model.ui.value.feed != null && !model.ui.value.loading) model.refresh() }
                OnReturn { model.refresh() }
                LibraryFeedScreen(library, ui, bottomClearance, LibraryFeedActions(
                    back = ::back, view = ::showView, retry = model::refresh, loadMore = model::loadMore,
                    openItem = ::open, openCollection = { collectionKey = it.key; collectionName = it.name },
                    resume = { nav.play(it.id, false) }, startOver = { nav.play(it.id, true) },
                    search = { nav.open(HomeRoute.Files(route.id, library.name, false)) }, edit = { nav.manageLibrary(route.id) },
                ), isOwner = isOwner, connectionLost = lost, format = format)
            }
            else -> {
                val model: LibraryViewModel = viewModel(key = "all-videos", factory = viewModelFactory {
                    initializer { LibraryViewModel(services.api, services.events, services.reconnects, route.id, library) }
                })
                val ui by model.ui.collectAsStateWithLifecycle()
                LaunchedEffect(ui.gone) { if (ui.gone) gone() }
                LibraryScreen(library.name, library.icon, ui, isOwner, bottomClearance, LibraryActions(
                    back = ::back, retry = model::retry, loadMore = model::loadMore, sort = model::setSort,
                    openItem = ::open, openShow = { nav.open(HomeRoute.Show(route.id, library.name, library.icon, it)) },
                    play = { nav.play(it.id, false) }, edit = { nav.manageLibrary(route.id) },
                    search = { nav.open(HomeRoute.Files(route.id, library.name, false)) }, folders = ::files,
                ), format = format, viewActions = { VideoLibraryMenu(view, ::showView) })
            }
        }
    }
}
