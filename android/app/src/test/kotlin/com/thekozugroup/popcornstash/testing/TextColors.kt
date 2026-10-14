package com.thekozugroup.popcornstash.testing

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.SemanticsNodeInteraction
import androidx.compose.ui.text.TextLayoutResult

/** The colour a text node is drawn in (find the node with `useUnmergedTree = true`). */
fun SemanticsNodeInteraction.drawnTextColor(): Color {
    val results = mutableListOf<TextLayoutResult>()
    val node = fetchSemanticsNode()
    val action = node.config[SemanticsActions.GetTextLayoutResult].action
    check(action != null && action(results) && results.isNotEmpty()) { "Not a text node: $node" }
    return results.first().layoutInput.style.color
}
