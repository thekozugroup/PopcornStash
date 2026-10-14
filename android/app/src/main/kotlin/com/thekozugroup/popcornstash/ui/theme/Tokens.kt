package com.thekozugroup.popcornstash.ui.theme

import androidx.compose.runtime.Immutable
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/** Spacing steps (android/DESIGN.md, Spacing). */
@Immutable
data class PopcornSpacing(
    val xxs: Dp = 4.dp,
    val xs: Dp = 8.dp,
    val s: Dp = 12.dp,
    val m: Dp = 16.dp,
    /** The page gutter, the same 20 as the web app on phones. */
    val gutter: Dp = 20.dp,
    val l: Dp = 24.dp,
    val xl: Dp = 32.dp,
    /** Between Home's sections. */
    val xxl: Dp = 40.dp,
    /** Every touch target is at least this tall and wide. */
    val minTouch: Dp = 48.dp,
)

/**
 * The floating toolbar (android/DESIGN.md, Floating toolbar): one column of squares on the
 * right edge of the screen. The outer radius is derived, never set: square radius plus
 * padding, so the selected square sits concentric in the bar's corners.
 */
@Immutable
data class ToolbarTokens(
    /** Size of each square target on a normal phone. */
    val square: Dp = 52.dp,
    /** The squares shrink on short windows, never below this. */
    val minSquare: Dp = 48.dp,
    val squareRadius: Dp = 12.dp,
    /**
     * A `surface` ring around the selection square, outside its edge. Over a bright picture the
     * dark glass turns grey and Action Blue alone is 1.79:1 against it; next to the ring it keeps
     * 3.38:1 in dark and 5.25:1 in light, whatever the glass shows.
     */
    val indicatorRing: Dp = 2.dp,
    /** Space between the squares; 0 merges them into one bar. */
    val gap: Dp = 0.dp,
    /** Space between the squares and the bar's edge. */
    val padding: Dp = 4.dp,
    val icon: Dp = 24.dp,
    val elevation: Dp = 8.dp,
    val border: Dp = 1.dp,
    /** The frosted glass's blur radius: text and edges behind the bar turn to soft colour. */
    val glassBlur: Dp = 24.dp,
    /** How much more saturated the colours behind the glass read through its tint. */
    val glassSaturation: Float = 1.8f,
    /**
     * Distance from the right edge of the screen and from the navigation bar inset under it, and
     * between the bar and a snackbar beside it. With gesture navigation the system's back-swipe
     * zone is usually wider than this, so the bar's outer part lies in it; that is fine, because
     * the bar takes only taps and long presses, which the system passes on.
     */
    val margin: Dp = 16.dp,
    /** Between the bar's edge and a square's tooltip, which opens to the left of the bar. */
    val tooltipGap: Dp = 8.dp,
    /** Between the end of a page's content and the top of the bar, once the page is scrolled to its end. */
    val contentGap: Dp = 16.dp,
    /** The ready-to-import count on the Downloads square. */
    val badgeHeight: Dp = 18.dp,
    /** A ring in the toolbar's colour that keeps the badge apart from the icon under it. */
    val badgeRing: Dp = 2.dp,
    /** How far the badge sits out from the square's top-left corner (the side away from the screen edge). */
    val badgeOffset: Dp = 4.dp,
) {
    val outerRadius: Dp get() = squareRadius + padding

    /** How wide the bar is: one square and its padding. */
    val thickness: Dp get() = square + padding * 2

    /** How tall the bar is with [count] squares stacked in it. */
    fun length(count: Int): Dp = square * count + gap * (count - 1) + padding * 2

    /**
     * Space a page leaves at its end (plus the navigation bar inset) so that, scrolled to the end,
     * its last row sits [contentGap] above a bar of [count] squares.
     */
    fun clearance(count: Int): Dp = length(count) + margin + contentGap

    /** Space a snackbar leaves on the right (plus the right inset) so it sits beside the bar, not under it. */
    val besideClearance: Dp get() = thickness + margin * 2
}

/** Tile widths for shelves and minimum widths for grids (android/DESIGN.md, Tiles and shelves). */
@Immutable
data class MediaTokens(
    val shelfPoster: Dp = 132.dp,
    val shelfFrame: Dp = 232.dp,
    val shelfPhoto: Dp = 112.dp,
    val shelfGap: Dp = 14.dp,
    val gridPosterMin: Dp = 104.dp,
    val gridFrameMin: Dp = 160.dp,
    val gridPhotoMin: Dp = 96.dp,
    val gridGap: Dp = 12.dp,
    val gridRowGap: Dp = 20.dp,
    val photoGap: Dp = 3.dp,
    val captionGap: Dp = 10.dp,
    /** The frame beside an episode's name on a show page. */
    val episodeFrame: Dp = 128.dp,
    /** The square of a library's or a file's glyph in a row. */
    val rowGlyph: Dp = 40.dp,
    val progressHeight: Dp = 4.dp,
    val placeholderIcon: Dp = 22.dp,
)

/**
 * Home's continue-watching carousel (android/DESIGN.md, Home): each card is the
 * gutter-to-gutter width minus [peek], so the next card shows at the edge, [gap] apart.
 */
@Immutable
data class CarouselTokens(
    val peek: Dp = 32.dp,
    val gap: Dp = 12.dp,
    /** Width over height of the card's frame. */
    val frameRatio: Float = 16f / 9f,
)

/** Lucide stroke widths (android/DESIGN.md, Touch and icons). */
object IconStroke {
    const val Regular = 1.75f
    const val Large = 1.5f
}
