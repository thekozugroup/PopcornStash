package com.thekozugroup.popcornstash.screenshots

import android.graphics.Bitmap
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.getUnclippedBoundsInRoot
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.junit4.ComposeTestRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import com.github.takahirom.roborazzi.RoborazziOptions
import com.github.takahirom.roborazzi.captureRoboImage
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import org.junit.Assert.assertTrue
import kotlin.math.roundToInt

/** A mid-size phone (Pixel 7 class): 411 x 891 dp at 420 dpi. */
const val PHONE = "w411dp-h891dp-xxhdpi"

/** The same phone held sideways (the player, the shell turned). */
const val PHONE_LANDSCAPE = "w891dp-h411dp-land-xxhdpi"

/** Where a screenshot is kept (committed, reviewed, and compared by verifyRoborazziDebug). */
fun screenshot(name: String): String = "src/test/screenshots/$name.png"

/**
 * A close-up of the floating toolbar with 12dp of the page around it, so the Downloads badge
 * (which stands out past the bar's top-left corner), the selection ring and the shadow show whole.
 */
fun ComposeTestRule.captureToolbarCloseUp(name: String, options: RoborazziOptions = RoborazziOptions()) {
    val bar = onNodeWithTag("toolbar").getUnclippedBoundsInRoot()
    val page = onRoot().captureToImage().asAndroidBitmap()
    val px = density.density
    val around = 12 * px
    val left = (bar.left.value * px - around).roundToInt().coerceAtLeast(0)
    val top = (bar.top.value * px - around).roundToInt().coerceAtLeast(0)
    val right = (bar.right.value * px + around).roundToInt().coerceAtMost(page.width)
    val bottom = (bar.bottom.value * px + around).roundToInt().coerceAtMost(page.height)
    Bitmap.createBitmap(page, left, top, right - left, bottom - top).captureRoboImage(screenshot(name), options)
}

/**
 * Fails when a control, a progress bar or a line of text on the page lies under the floating
 * toolbar, or within [clearance] above it: what a page scrolled to its end must show
 * (android/DESIGN.md, Floating toolbar: its last row 16dp above the bar).
 */
fun ComposeTestRule.assertNothingUnderTheToolbar(clearance: Dp = 0.dp) {
    val toolbar = onNodeWithTag("toolbar").fetchSemanticsNode().boundsInRoot
    // With a clearance, half a pixel of slack for rounding: a row that ends exactly [clearance] above the bar passes.
    val bar = if (clearance > 0.dp) toolbar.copy(top = toolbar.top - clearance.value * density.density + 0.5f) else toolbar
    val onPage = (
        hasClickAction() or
            SemanticsMatcher.keyIsDefined(SemanticsProperties.Text) or
            SemanticsMatcher.keyIsDefined(SemanticsProperties.ProgressBarRangeInfo)
        ) and
        !hasAnyAncestor(hasTestTag("toolbar")) and !hasTestTag("toolbar")
    val under = onAllNodes(onPage).fetchSemanticsNodes()
        .filter { node -> node.boundsInRoot.let { it.width > 0f && it.height > 0f && it.overlaps(bar) } }
    assertTrue(
        "Under the toolbar, or within $clearance above it, at $toolbar: " + under.joinToString { node ->
            (node.config.getOrElseNullable(SemanticsProperties.Text) { null }?.joinToString() ?: "a control") + " at ${node.boundsInRoot}"
        },
        under.isEmpty(),
    )
}
