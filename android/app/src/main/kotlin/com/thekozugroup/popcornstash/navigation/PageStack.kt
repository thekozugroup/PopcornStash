package com.thekozugroup.popcornstash.navigation

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.AnimatedContentTransitionScope
import androidx.compose.animation.ContentTransform
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.Saver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.saveable.rememberSaveableStateHolder
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.lifecycle.ViewModelStore
import androidx.lifecycle.ViewModelStoreOwner
import androidx.lifecycle.viewmodel.compose.LocalViewModelStoreOwner
import com.thekozugroup.popcornstash.ui.theme.Popcorn
import kotlinx.serialization.KSerializer
import kotlinx.serialization.Serializable
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.json.Json

/** One page in a tab's stack: [id] tells two visits to the same place apart. */
@Serializable
data class PageEntry<R>(val id: Long, val route: R)

/**
 * A tab's own back stack (android/DESIGN.md, Tab transitions): the first page stays at the
 * bottom; pages open on top of it and close back to it. Saved with the tab's state, so the
 * pages a person opened come back after another tab, a rotation or the process being
 * recreated.
 */
@Stable
class PageStack<R : Any> internal constructor(initial: List<PageEntry<R>>, private var nextId: Long) {
    private val stack = mutableStateListOf<PageEntry<R>>().apply { addAll(initial) }

    val entries: List<PageEntry<R>> get() = stack

    val top: PageEntry<R> get() = stack.last()

    val depth: Int get() = stack.size

    /** Whether the last change opened a page (true) or closed one; decides the slide direction. */
    var forward by mutableStateOf(true)
        private set

    fun push(route: R) {
        forward = true
        stack.add(PageEntry(nextId++, route))
    }

    /** Closes the top page; false on the first page, which never closes. */
    fun pop(): Boolean {
        if (stack.size <= 1) return false
        forward = false
        stack.removeAt(stack.lastIndex)
        return true
    }

    /** Closes every page above the first; false when there was nothing to close. */
    fun popToRoot(): Boolean {
        if (stack.size <= 1) return false
        forward = false
        stack.removeRange(1, stack.size)
        return true
    }

    /** Closes [route]'s page and every page above it (a library that went away, with its show and items). */
    fun popFrom(predicate: (R) -> Boolean): Boolean {
        val index = stack.indexOfFirst { predicate(it.route) }
        if (index <= 0) return false
        forward = false
        stack.removeRange(index, stack.size)
        return true
    }

    companion object {
        fun <R : Any> of(root: R) = PageStack(listOf(PageEntry(0, root)), 1)

        private val json = Json { ignoreUnknownKeys = true }

        /** Keeps the stack as JSON, routes included ([routes] must describe every route). */
        fun <R : Any> saver(routes: KSerializer<R>): Saver<PageStack<R>, String> {
            val list = ListSerializer(PageEntry.serializer(routes))
            return Saver(
                save = { stack -> json.encodeToString(list, stack.entries.toList()) },
                restore = { saved ->
                    runCatching { json.decodeFromString(list, saved) }.getOrNull()
                        ?.takeIf { it.isNotEmpty() }
                        ?.let { entries -> PageStack(entries, entries.maxOf { it.id } + 1) }
                },
            )
        }
    }
}

/** A page stack that starts on [root] and is saved with the caller's state. */
@Composable
fun <R : Any> rememberPageStack(root: R, routes: KSerializer<R>): PageStack<R> =
    rememberSaveable(saver = PageStack.saver(routes)) { PageStack.of(root) }

/**
 * The view model stores of the pages in every tab's stack, one per page. They outlive a tab
 * switch and a rotation (the pages' data stays loaded) and are cleared when their page closes
 * and, all at once, when the person's session ends.
 */
class ViewModelStores {
    private val stores = LinkedHashMap<String, ViewModelStore>()

    @Synchronized
    fun owner(key: String): ViewModelStoreOwner {
        val store = stores.getOrPut(key) { ViewModelStore() }
        return object : ViewModelStoreOwner {
            override val viewModelStore: ViewModelStore = store
        }
    }

    @Synchronized
    fun contains(key: String): Boolean = key in stores

    @Synchronized
    fun clear(key: String) {
        stores.remove(key)?.clear()
    }

    /** Clears the stores under [prefix] that are not in [keep] (pages from a stack that is gone). */
    @Synchronized
    fun retainOnly(prefix: String, keep: Set<String>) {
        val stale = stores.keys.filter { it.startsWith(prefix) && it !in keep }
        stale.forEach { clear(it) }
    }

    @Synchronized
    fun clearAll() {
        val all = stores.values.toList()
        stores.clear()
        all.forEach { it.clear() }
    }
}

/**
 * Shows the top page of [stack]: pages open and close with the push transition (a short
 * shift with a fade; a crossfade under reduce motion), system Back closes the top page,
 * and every page keeps its saved state (scroll) and its own view models ([stores], under
 * "[name]/<page id>") until it closes.
 */
@Composable
fun <R : Any> PageStackHost(
    stack: PageStack<R>,
    stores: ViewModelStores,
    name: String,
    modifier: Modifier = Modifier,
    transition: (AnimatedContentTransitionScope<PageEntry<R>>.(forward: Boolean) -> ContentTransform)? = null,
    content: @Composable (PageEntry<R>) -> Unit,
) {
    val holder = rememberSaveableStateHolder()
    val density = LocalDensity.current
    val reduceMotion = Popcorn.reduceMotion
    fun keyOf(entry: PageEntry<R>) = "$name/${entry.id}"

    BackHandler(enabled = stack.depth > 1) { stack.pop() }

    // A stack made fresh (the activity started again) has no use for the stores of pages it lost.
    LaunchedEffect(stores) { stores.retainOnly("$name/", stack.entries.map(::keyOf).toSet()) }

    AnimatedContent(
        targetState = stack.top,
        contentKey = { it.id },
        transitionSpec = {
            transition?.invoke(this, stack.forward) ?: pushTransition(stack.forward, density, reduceMotion)
        },
        label = "$name pages",
        modifier = modifier.fillMaxSize(),
    ) { entry ->
        holder.SaveableStateProvider(entry.id) {
            val owner = remember(entry.id) { stores.owner(keyOf(entry)) }
            CompositionLocalProvider(LocalViewModelStoreOwner provides owner) {
                content(entry)
            }
        }
        DisposableEffect(entry.id) {
            onDispose {
                // Gone from the stack (not just off screen for another tab): forget it for good.
                if (stack.entries.none { it.id == entry.id }) {
                    holder.removeState(entry.id)
                    stores.clear(keyOf(entry))
                }
            }
        }
    }
}
