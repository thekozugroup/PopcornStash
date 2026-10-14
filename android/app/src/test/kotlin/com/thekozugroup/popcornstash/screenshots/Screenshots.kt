package com.thekozugroup.popcornstash.screenshots

/** A mid-size phone (Pixel 7 class): 411 x 891 dp at 420 dpi. */
const val PHONE = "w411dp-h891dp-xxhdpi"

/** Where a screenshot is kept (committed, reviewed, and compared by verifyRoborazziDebug). */
fun screenshot(name: String): String = "src/test/screenshots/$name.png"
