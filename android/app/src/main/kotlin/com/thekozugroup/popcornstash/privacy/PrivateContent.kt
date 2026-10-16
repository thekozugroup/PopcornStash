package com.thekozugroup.popcornstash.privacy

import androidx.compose.runtime.Composable
import androidx.compose.runtime.saveable.rememberSaveableStateHolder

/** Remove every private window on lock, keeping navigation and form state in memory. */
@Composable
internal fun PrivateContent(unlocked: Boolean, locked: @Composable () -> Unit, content: @Composable () -> Unit) {
    val state = rememberSaveableStateHolder()
    if (unlocked) state.SaveableStateProvider("private") { content() } else locked()
}
