package com.thekozugroup.popcornstash.feature.library

import kotlinx.coroutines.Job
import java.util.concurrent.atomic.AtomicBoolean

/**
 * A next page asked for while the page itself was still loading (the person reached the end of
 * the list as the first page came in, or during a reload after an event). Instead of dropping
 * the request, [after] runs it once that load has ended, so the next page comes from the cursor
 * the fresh page carries. A load that is cancelled (a new sort, a newer reload) drops it; the
 * person's next scroll asks again.
 */
internal class PendingMore {
    private val wanted = AtomicBoolean(false)

    /** Runs [retry] once, after [load] completes without being cancelled. */
    fun after(load: Job, retry: () -> Unit) {
        wanted.set(true)
        // Called at once when the load has already ended, so a request is never lost between the two.
        load.invokeOnCompletion { cause -> if (cause == null && wanted.getAndSet(false)) retry() }
    }

    fun clear() = wanted.set(false)
}
