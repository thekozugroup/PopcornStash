package com.thekozugroup.popcornstash.navigation

import androidx.compose.runtime.saveable.SaverScope
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import com.thekozugroup.popcornstash.feature.home.HomeRoute
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotSame
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

/** A tab's own back stack, saved with the tab, and its pages' view model stores. */
class PageStackTest {
    private val library = HomeRoute.Library("lib-kids", "Kids", "baby")
    private val show = HomeRoute.Show("lib-kids", "Kids", "baby", "Tidepool Detectives")
    private val item = HomeRoute.Item("e-1", "Low tide")

    @Test
    fun pagesOpenOnTopAndCloseBackToTheFirst() {
        val stack = PageStack.of<HomeRoute>(HomeRoute.Root)
        assertFalse("the first page never closes", stack.pop())
        stack.push(library)
        stack.push(show)
        assertTrue(stack.forward)
        assertEquals(show, stack.top.route)
        assertTrue(stack.pop())
        assertFalse(stack.forward)
        assertEquals(library, stack.top.route)
        stack.push(item)
        assertTrue(stack.popToRoot())
        assertEquals(1, stack.depth)
        assertFalse(stack.popToRoot())
    }

    @Test
    fun twoVisitsToOnePlaceAreTwoPages() {
        val stack = PageStack.of<HomeRoute>(HomeRoute.Root)
        stack.push(item)
        stack.push(item)
        assertEquals(2, stack.entries.drop(1).map { it.id }.distinct().size)
    }

    @Test
    fun aPlaceThatWentAwayClosesWithThePagesAboveIt() {
        val stack = PageStack.of<HomeRoute>(HomeRoute.Root)
        stack.push(library)
        stack.push(show)
        stack.push(item)
        assertTrue(stack.popFrom { it == library })
        assertEquals(listOf<HomeRoute>(HomeRoute.Root), stack.entries.map { it.route })
        assertFalse("nothing to close", stack.popFrom { it == library })
        assertFalse("the first page stays", stack.popFrom { it == HomeRoute.Root })
    }

    @Test
    fun theStackIsSavedAndRestoredWithItsRoutesAndIds() {
        val stack = PageStack.of<HomeRoute>(HomeRoute.Root)
        stack.push(library)
        stack.push(show)
        val saver = PageStack.saver(HomeRoute.serializer())
        val saved = with(saver) { SaverScope { true }.save(stack) }!!
        val restored = saver.restore(saved)!!
        assertEquals(stack.entries.toList(), restored.entries.toList())
        restored.push(item)
        assertEquals("ids keep counting up", 3L, restored.top.id)
        assertEquals(null, saver.restore("not json"))
    }

    class Counter : ViewModel() {
        var cleared = false

        override fun onCleared() {
            cleared = true
        }
    }

    private fun counter(stores: ViewModelStores, key: String): Counter =
        ViewModelProvider.create(stores.owner(key), viewModelFactory { initializer { Counter() } })[Counter::class]

    @Test
    fun aPageKeepsItsViewModelsUntilItClosesOrTheSessionEnds() {
        val stores = ViewModelStores()
        val first = counter(stores, "home/1")
        assertSame("the same page gets the same view model back", first, counter(stores, "home/1"))
        val other = counter(stores, "home/2")
        stores.clear("home/1")
        assertTrue(first.cleared)
        assertNotSame(first, counter(stores, "home/1"))

        stores.retainOnly("home/", setOf("home/1"))
        assertTrue("a page from a lost stack is let go", other.cleared)
        val settings = counter(stores, "settings/0")
        stores.clearAll()
        assertTrue(settings.cleared)
        assertFalse(stores.contains("home/1"))
    }
}
