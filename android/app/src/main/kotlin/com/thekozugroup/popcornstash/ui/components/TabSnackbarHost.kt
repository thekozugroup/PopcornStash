package com.thekozugroup.popcornstash.ui.components

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.IconButton
import androidx.compose.material3.Snackbar
import androidx.compose.material3.SnackbarData
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.AbsoluteAlignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import com.thekozugroup.popcornstash.navigation.LocalToolbarRequests
import com.thekozugroup.popcornstash.navigation.TabPage
import com.thekozugroup.popcornstash.ui.icons.LucideIcon
import com.thekozugroup.popcornstash.ui.icons.PopcornIcons
import com.thekozugroup.popcornstash.ui.theme.Popcorn
import com.thekozugroup.popcornstash.ui.theme.PopcornPalette

/**
 * Where a tab's snackbars show (android/DESIGN.md, Snackbars): at the bottom of the window, the
 * snackbar's edge level with the floating toolbar's bottom edge and to its left, the toolbar
 * margin from the screen edge and from the bar, so a snackbar is never under the bar. On a wide
 * window it keeps its Material maximum width and stays beside the bar. Lay it over the tab's
 * pages, last in a Box.
 *
 * While a page has asked the toolbar to leave (the photo viewer), the tab's snackbars wait: one
 * on screen goes and comes back for its full time when the viewer closes, and one sent meanwhile
 * shows then. The viewer says what it has to say in its own snackbar, on top of its plate.
 */
@Composable
fun TabSnackbarHost(state: SnackbarHostState, page: TabPage, modifier: Modifier = Modifier) {
    if (LocalToolbarRequests.current.hidden) return
    Box(modifier.fillMaxSize().padding(page.snackbarPadding), contentAlignment = AbsoluteAlignment.BottomRight) {
        SnackbarHost(state, Modifier.testTag("snackbars")) { data -> PopcornSnackbar(data, Modifier.testTag("snackbar")) }
    }
}

/**
 * A snackbar in the inverse surface with 8dp corners. [light] is the zinc-50 one (the inverse of
 * a dark page, and of the photo viewer's black stage); otherwise zinc-900.
 *
 * Built on Material's content-slot snackbar, with the action and the dismiss button Material's
 * `Snackbar(snackbarData)` would add: that overload also puts 12dp of space of its own around the
 * surface, which would move the snackbar off the edges its host lines it up with.
 */
@Composable
fun PopcornSnackbar(data: SnackbarData, modifier: Modifier = Modifier, light: Boolean = Popcorn.colors.isDark) {
    val content = if (light) PopcornPalette.Zinc900 else PopcornPalette.Zinc50
    // The link blue for that surface, as the theme's inversePrimary.
    val actionColor = if (light) PopcornPalette.Blue600 else PopcornPalette.Blue400
    val visuals = data.visuals
    val label = visuals.actionLabel
    Snackbar(
        modifier = modifier,
        action = label?.let {
            {
                TextButton(onClick = data::performAction, colors = ButtonDefaults.textButtonColors(contentColor = actionColor)) {
                    Text(label)
                }
            }
        },
        dismissAction = if (visuals.withDismissAction) {
            { IconButton(onClick = data::dismiss) { LucideIcon(PopcornIcons.Close, contentDescription = "Dismiss") } }
        } else {
            null
        },
        shape = Popcorn.shapes.control,
        containerColor = if (light) PopcornPalette.Zinc50 else PopcornPalette.Zinc900,
        contentColor = content,
        actionContentColor = actionColor,
        dismissActionContentColor = content,
    ) {
        Text(visuals.message)
    }
}
