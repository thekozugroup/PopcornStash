package com.thekozugroup.popcornstash.ui.components

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Snackbar
import androidx.compose.material3.SnackbarData
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.runtime.Composable
import androidx.compose.ui.AbsoluteAlignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import com.thekozugroup.popcornstash.navigation.TabPage
import com.thekozugroup.popcornstash.ui.theme.Popcorn
import com.thekozugroup.popcornstash.ui.theme.PopcornPalette

/**
 * Where a tab's snackbars show (android/DESIGN.md, Snackbars): at the bottom of the window, level
 * with the floating toolbar's bottom edge and to its left, the toolbar margin from the screen
 * edge and from the bar, so a snackbar is never under the bar. On a wide window it keeps its
 * Material maximum width and stays beside the bar. Lay it over the tab's pages, last in a Box.
 */
@Composable
fun TabSnackbarHost(state: SnackbarHostState, page: TabPage, modifier: Modifier = Modifier) {
    Box(modifier.fillMaxSize().padding(page.snackbarPadding), contentAlignment = AbsoluteAlignment.BottomRight) {
        SnackbarHost(state, Modifier.testTag("snackbars")) { data -> PopcornSnackbar(data) }
    }
}

/**
 * A snackbar in the inverse surface with 8dp corners. [light] is the zinc-50 one (the inverse of
 * a dark page, and of the photo viewer's black stage); otherwise zinc-900.
 */
@Composable
fun PopcornSnackbar(data: SnackbarData, modifier: Modifier = Modifier, light: Boolean = Popcorn.colors.isDark) {
    Snackbar(
        snackbarData = data,
        shape = Popcorn.shapes.control,
        containerColor = if (light) PopcornPalette.Zinc50 else PopcornPalette.Zinc900,
        contentColor = if (light) PopcornPalette.Zinc900 else PopcornPalette.Zinc50,
        // The link blue for that surface, as the theme's inversePrimary.
        actionColor = if (light) PopcornPalette.Blue600 else PopcornPalette.Blue400,
        modifier = modifier,
    )
}
