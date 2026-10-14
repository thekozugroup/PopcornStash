package com.thekozugroup.popcornstash.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import com.thekozugroup.popcornstash.ui.theme.Popcorn

/**
 * A load that did not work (android/DESIGN.md, Loading and errors): one sentence and
 * "Try again", where the content would be. [title] names what did not load when a page has
 * more than one part ("Files to import could not be loaded"); it is read with the sentence.
 */
@Composable
fun LoadFailed(message: String, onRetry: () -> Unit, modifier: Modifier = Modifier, title: String? = null) {
    Column(
        modifier
            .fillMaxWidth()
            .padding(horizontal = Popcorn.spacing.gutter, vertical = Popcorn.spacing.xl)
            .semantics(mergeDescendants = title != null) { liveRegion = LiveRegionMode.Polite },
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        if (title != null) {
            Text(title, style = Popcorn.type.section, color = Popcorn.colors.text, textAlign = TextAlign.Center)
            Spacer(Modifier.height(Popcorn.spacing.xxs))
        }
        Text(
            message,
            style = Popcorn.type.body,
            color = Popcorn.colors.textMuted,
            textAlign = TextAlign.Center,
            modifier = Modifier.widthIn(max = 360.dp),
        )
        Spacer(Modifier.height(Popcorn.spacing.m))
        SecondaryButton("Try again", onClick = onRetry)
    }
}

/**
 * The single row under the top bar while the server is out of reach: "Not connected to the
 * server. Trying again." with the warning dot.
 */
@Composable
fun ConnectionLostRow(modifier: Modifier = Modifier) {
    Row(
        modifier
            .fillMaxWidth()
            .padding(horizontal = Popcorn.spacing.gutter, vertical = Popcorn.spacing.xs)
            .semantics(mergeDescendants = true) { liveRegion = LiveRegionMode.Polite },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.size(8.dp).background(Popcorn.colors.warningDot, Popcorn.shapes.avatar))
        Spacer(Modifier.width(Popcorn.spacing.xs))
        Text("Not connected to the server. Trying again.", style = Popcorn.type.metadata, color = Popcorn.colors.warning)
    }
}
