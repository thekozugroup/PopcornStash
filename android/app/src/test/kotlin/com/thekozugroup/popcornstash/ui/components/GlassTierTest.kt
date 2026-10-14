package com.thekozugroup.popcornstash.ui.components

import android.os.Build
import org.junit.Assert.assertEquals
import org.junit.Test

/** Which glass each Android release draws (android/DESIGN.md, Floating toolbar). */
class GlassTierTest {
    @Test
    fun androidNineToElevenKeepTheOpaqueBar() {
        assertEquals(GlassTier.Opaque, glassTierFor(sdkInt = 28, sdkIntFull = 0))
        assertEquals(GlassTier.Opaque, glassTierFor(sdkInt = 29, sdkIntFull = 0))
        assertEquals(GlassTier.Opaque, glassTierFor(sdkInt = 30, sdkIntFull = 0))
    }

    @Test
    fun androidTwelveToSeventeenQpr1CaptureThePages() {
        assertEquals(GlassTier.Captured, glassTierFor(sdkInt = 31, sdkIntFull = 0))
        assertEquals(GlassTier.Captured, glassTierFor(sdkInt = 35, sdkIntFull = 0))
        assertEquals(GlassTier.Captured, glassTierFor(sdkInt = 36, sdkIntFull = 3_600_000))
        assertEquals(GlassTier.Captured, glassTierFor(sdkInt = 36, sdkIntFull = 3_600_001))
        assertEquals(GlassTier.Captured, glassTierFor(sdkInt = 37, sdkIntFull = 3_700_000))
        assertEquals(GlassTier.Captured, glassTierFor(sdkInt = 37, sdkIntFull = 3_700_001))
    }

    @Test
    fun androidSeventeenQpr2AndLaterUseThePlatformBackdrop() {
        assertEquals(GlassTier.Backdrop, glassTierFor(sdkInt = 37, sdkIntFull = 3_700_002))
        assertEquals(GlassTier.Backdrop, glassTierFor(sdkInt = 37, sdkIntFull = 3_700_003))
        assertEquals(GlassTier.Backdrop, glassTierFor(sdkInt = 38, sdkIntFull = 3_800_000))
    }

    /** A full version without the matching major (an inconsistent report) never picks the 37.2 API. */
    @Test
    fun theMajorVersionMustAgree() {
        assertEquals(GlassTier.Captured, glassTierFor(sdkInt = 36, sdkIntFull = 3_700_002))
    }

    /** Medium or high contrast in the system settings asks for stronger edges: a solid bar on every release. */
    @Test
    fun moreContrastKeepsTheOpaqueBar() {
        assertEquals(GlassTier.Opaque, glassTierFor(sdkInt = 34, sdkIntFull = 0, moreContrast = true))
        assertEquals(GlassTier.Opaque, glassTierFor(sdkInt = 36, sdkIntFull = 3_600_000, moreContrast = true))
        assertEquals(GlassTier.Opaque, glassTierFor(sdkInt = 37, sdkIntFull = 3_700_002, moreContrast = true))
    }

    @Test
    fun theBackdropReleaseIsAndroidSeventeenQpr2() {
        assertEquals(Build.VERSION_CODES_FULL.CINNAMON_BUN_2, BACKDROP_SDK_INT_FULL)
    }
}
