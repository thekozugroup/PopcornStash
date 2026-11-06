package com.thekozugroup.popcornstash.feature.player

import androidx.compose.ui.unit.IntRect
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.LayoutDirection
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class SeekPreviewTest {
    @Test fun samplesBoundedPositionsWithoutPassingTheVideoEnd() {
        assertEquals(6L, previewSecond(7900, 60000))
        assertEquals(58L, previewSecond(999000, 60000))
        assertEquals(0L, previewSecond(-1000, 60000))
        assertEquals(0L, previewSecond(0, 500))
        assertEquals(3600L, previewSecond(3600000, 7200000))
    }

    @Test fun previewStaysInsideTheWindowAtEitherEdgeAndInRtl() {
        val provider = PreviewPosition(12)
        for (direction in LayoutDirection.entries) {
            for (x in listOf(0, 180, 390)) {
                val at = provider.calculatePosition(IntRect(x, 200, x + 16, 216), IntSize(400, 800), direction, IntSize(160, 120))
                assertTrue(at.x in 0..240)
                assertEquals(68, at.y)
            }
        }
    }
}
