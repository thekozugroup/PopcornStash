package com.thekozugroup.popcornstash.privacy

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import com.thekozugroup.popcornstash.ui.components.PrimaryButton
import com.thekozugroup.popcornstash.ui.icons.LucideGlyphs
import com.thekozugroup.popcornstash.ui.icons.LucideIcon
import com.thekozugroup.popcornstash.ui.theme.Popcorn

/** Contains no server address, account name, poster, download name or other private content. */
@Composable
fun AppLockScreen(secure: Boolean, busy: Boolean, problem: String?, onUnlock: () -> Unit, onSetLock: () -> Unit) {
    Column(
        modifier = Modifier.fillMaxSize().background(Popcorn.colors.page).safeDrawingPadding()
            .verticalScroll(rememberScrollState()).padding(Popcorn.spacing.gutter),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Column(Modifier.widthIn(max = 400.dp).fillMaxWidth()) {
            LucideIcon(LucideGlyphs.Popcorn, contentDescription = null, tint = Popcorn.colors.text, size = 40.dp)
            Spacer(Modifier.height(Popcorn.spacing.xl))
            Text(if (secure) "PopcornStash is locked" else "Protect your library", style = Popcorn.type.headline,
                color = Popcorn.colors.text, modifier = Modifier.semantics { heading() })
            Spacer(Modifier.height(Popcorn.spacing.xs))
            Text(if (secure) "Enter your device PIN, password or pattern to continue."
                else "Set a PIN, password or pattern in your phone Settings before opening PopcornStash.",
                style = Popcorn.type.body, color = Popcorn.colors.textMuted)
            if (problem != null) {
                Spacer(Modifier.height(Popcorn.spacing.m))
                Text(problem, style = Popcorn.type.body, color = Popcorn.colors.error,
                    modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite })
            }
            Spacer(Modifier.height(Popcorn.spacing.l))
            PrimaryButton(if (secure) "Unlock" else "Set up screen lock", if (secure) onUnlock else onSetLock,
                modifier = Modifier.fillMaxWidth(), busy = busy)
        }
    }
}
