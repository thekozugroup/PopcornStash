package com.thekozugroup.popcornstash.ui.components

import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.dp
import com.thekozugroup.popcornstash.data.api.Item
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ShelfThumbnailTest {
    private val portrait = Item(id = "portrait", title = "Portrait", width = 1080, height = 1920)
    private val landscape = portrait.copy(width = 1920, height = 1080)

    @Test
    fun portraitCroppedToFrameRunsOutOfWidthAt225Pixels() {
        // The 400px portrait thumbnail is 225px wide, even though the frame is horizontal.
        assertFalse(TileShape.Frame.needsLargeThumbnail(portrait, 225))
        assertTrue(TileShape.Frame.needsLargeThumbnail(portrait, 226))
        assertTrue(TileShape.Frame.needsLargeThumbnail(portrait, 232))
    }

    @Test
    fun sameLandscapeFrameUsesSmallAtSingleDensityAndLargeAtDoubleDensity() {
        val width = 232.dp
        assertFalse(TileShape.Frame.needsLargeThumbnail(landscape, with(Density(1f)) { width.roundToPx() }))
        assertTrue(TileShape.Frame.needsLargeThumbnail(landscape, with(Density(2f)) { width.roundToPx() }))
    }

    @Test
    fun landscapeCroppedToPosterCanRunOutOfHeight() {
        // A 400 × 225 thumbnail can cover a 150 × 225 poster.
        assertFalse(TileShape.Poster.needsLargeThumbnail(landscape, 150))
        assertTrue(TileShape.Poster.needsLargeThumbnail(landscape, 151))
    }

    @Test
    fun missingOrInvalidDimensionsChooseLargeAndTinyOriginalsUseSmall() {
        assertTrue(TileShape.Frame.needsLargeThumbnail(null, 232))
        assertTrue(TileShape.Frame.needsLargeThumbnail(portrait.copy(width = null), 232))
        assertTrue(TileShape.Frame.needsLargeThumbnail(portrait.copy(height = 0), 232))
        assertFalse(TileShape.Frame.needsLargeThumbnail(portrait.copy(width = 100, height = 200), 232))
    }
}
