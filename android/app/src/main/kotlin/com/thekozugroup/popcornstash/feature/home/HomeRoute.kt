package com.thekozugroup.popcornstash.feature.home

import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHostState
import com.thekozugroup.popcornstash.navigation.PageStack
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * The pages of the Home tab's stack. Routes carry ids and the names the top bar shows at
 * once; everything else is read from the server when the page opens.
 */
@Serializable
sealed interface HomeRoute {
    /** Home itself: continue watching, then every library. */
    @Serializable
    @SerialName("home")
    data object Root : HomeRoute

    @Serializable
    @SerialName("library")
    data class Library(val id: String, val name: String, val icon: String) : HomeRoute

    @Serializable
    @SerialName("show")
    data class Show(val libraryId: String, val libraryName: String, val icon: String, val name: String) : HomeRoute

    @Serializable
    @SerialName("item")
    data class Item(val id: String, val title: String) : HomeRoute

    @Serializable
    @SerialName("other_files")
    data object OtherFiles : HomeRoute
}

/** The library a page belongs to, when it belongs to one. */
val HomeRoute.libraryId: String?
    get() = when (this) {
        is HomeRoute.Library -> id
        is HomeRoute.Show -> libraryId
        else -> null
    }

/**
 * What the Home pages may do outside themselves: open and close pages in the tab's stack,
 * say something in a snackbar, hand an item to the player, and (owners) open a library's
 * settings.
 */
class HomeNavigator(
    private val stack: PageStack<HomeRoute>,
    private val scope: CoroutineScope,
    private val snackbar: SnackbarHostState,
    /** Plays the item with this id: from where the person stopped, or from the start. */
    val play: (itemId: String, fromStart: Boolean) -> Unit,
    /** Owners: the settings of the library with this id, or a new library for null. */
    val manageLibrary: (libraryId: String?) -> Unit,
) {
    fun open(route: HomeRoute) = stack.push(route)

    fun back() {
        stack.pop()
    }

    fun message(text: String) {
        scope.launch { snackbar.showSnackbar(text, duration = SnackbarDuration.Short) }
    }

    /**
     * A page whose content went away (deleted, or no longer shared): it closes, with the pages
     * above it, and [text] says why. Nothing happens when that page is already closed.
     */
    fun closeGone(route: HomeRoute, text: String) {
        if (stack.popFrom { it == route }) message(text)
    }
}
