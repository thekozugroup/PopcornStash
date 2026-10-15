package com.thekozugroup.popcornstash.feature.player

import androidx.window.layout.FoldingFeature
import androidx.window.layout.WindowLayoutInfo

/** Horizontal half-open fold, in window pixels. Both panes keep clear of its full bounds. */
data class FlexHinge(val top: Int, val bottom: Int)

fun WindowLayoutInfo.flexHinge(): FlexHinge? = displayFeatures
    .filterIsInstance<FoldingFeature>()
    .firstOrNull { it.state == FoldingFeature.State.HALF_OPENED && it.orientation == FoldingFeature.Orientation.HORIZONTAL }
    ?.bounds?.let { FlexHinge(it.top, it.bottom) }
