package com.thekozugroup.popcornstash.testing

import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.ViewModelStore
import com.thekozugroup.popcornstash.data.api.StashJson
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.test.TestDispatcher
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.setMain
import kotlinx.serialization.KSerializer
import kotlinx.serialization.builtins.ListSerializer
import mockwebserver3.MockWebServer
import mockwebserver3.RecordedRequest
import org.junit.rules.TestWatcher
import org.junit.runner.Description
import java.util.concurrent.TimeUnit
import kotlin.reflect.KClass

/**
 * Runs view models' `viewModelScope` (Dispatchers.Main) on a test dispatcher for each test, and
 * clears the view models made with [keep] when the test ends, before Main is reset: an answer
 * that arrives after the test then finds its work cancelled instead of a missing Main.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class MainDispatcherRule(val dispatcher: TestDispatcher = UnconfinedTestDispatcher()) : TestWatcher() {
    private val store = ViewModelStore()
    private var made = 0

    override fun starting(description: Description) = Dispatchers.setMain(dispatcher)

    override fun finished(description: Description) {
        store.clear()
        Dispatchers.resetMain()
    }

    /** Makes a view model that is cleared at the end of the test. */
    fun <T : ViewModel> track(type: KClass<T>, make: () -> T): T {
        val factory = object : ViewModelProvider.Factory {
            @Suppress("UNCHECKED_CAST")
            override fun <VM : ViewModel> create(modelClass: Class<VM>): VM = make() as VM
        }
        return ViewModelProvider.create(store, factory)["model ${made++}", type]
    }
}

/** A view model cleared at the end of the test (see [MainDispatcherRule]). */
inline fun <reified T : ViewModel> MainDispatcherRule.keep(noinline make: () -> T): T = track(T::class, make)

/** Waits (in real time: answers come over a real socket) until the state matches. */
suspend fun <T> StateFlow<T>.awaitUntil(predicate: (T) -> Boolean): T = withinRealSeconds { first(predicate) }

/** The JSON the server would send for [value]. */
fun <T> json(serializer: KSerializer<T>, value: T): String = StashJson.encodeToString(serializer, value)

fun <T> jsonList(serializer: KSerializer<T>, values: List<T>): String = StashJson.encodeToString(ListSerializer(serializer), values)

/** The next request, or null when none comes within [millis]. */
fun MockWebServer.nextOrNull(millis: Long = 300): RecordedRequest? = takeRequest(millis, TimeUnit.MILLISECONDS)
