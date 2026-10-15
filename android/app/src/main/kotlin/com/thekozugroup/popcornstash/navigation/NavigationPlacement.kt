package com.thekozugroup.popcornstash.navigation

import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/** Current window space, including split screen and folded displays, decides navigation. */
enum class NavigationPlacement { Bottom, Right }

fun navigationPlacement(width: Dp, height: Dp): NavigationPlacement =
    if (width >= 600.dp && height >= 480.dp) NavigationPlacement.Right else NavigationPlacement.Bottom
