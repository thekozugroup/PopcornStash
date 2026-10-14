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
 * The floating toolbar (android/DESIGN.md, Floating toolbar). The outer radius is
 * derived, never set: square radius plus padding, so the selected square sits
 * concentric in the bar's corners.
 */
@Immutable
data class ToolbarTokens(
    /** Size of each square target on a normal phone. */
    val square: Dp = 52.dp,
    /** The squares shrink on narrow screens, never below this. */
    val minSquare: Dp = 48.dp,
    val squareRadius: Dp = 12.dp,
    /** Space between the squares; 0 merges them into one bar. */
    val gap: Dp = 0.dp,
    /** Space between the squares and the bar's edge. */
    val padding: Dp = 4.dp,
    val icon: Dp = 24.dp,
    val elevation: Dp = 8.dp,
    val border: Dp = 1.dp,
    /** Distance from the screen edges and from the navigation bar inset. */
    val margin: Dp = 16.dp,
    /** The ready-to-import count on the Downloads square. */
    val badgeHeight: Dp = 18.dp,
    /** A ring in the toolbar's colour that keeps the badge apart from the icon under it. */
    val badgeRing: Dp = 2.dp,
    /** How far the badge sits out from the square's top-end corner. */
    val badgeOffset: Dp = 4.dp,
) {
    val outerRadius: Dp get() = squareRadius + padding
    val height: Dp get() = square + padding * 2

    /** Space a page leaves at its end so its last row clears the toolbar (plus the navigation bar inset). */
    val clearance: Dp get() = height + margin + 16.dp
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
