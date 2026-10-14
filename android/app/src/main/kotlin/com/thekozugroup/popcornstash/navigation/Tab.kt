package com.thekozugroup.popcornstash.navigation

import com.thekozugroup.popcornstash.ui.icons.LucideGlyph
import com.thekozugroup.popcornstash.ui.icons.PopcornIcons

/**
 * The app's three destinations, in toolbar order from left to right. Home, in the
 * centre, is the start tab. The order decides which way a tab change slides.
 */
enum class Tab(val label: String, val glyph: LucideGlyph) {
    Downloads("Downloads", PopcornIcons.Downloads),
    Home("Home", PopcornIcons.Home),
    Settings("Settings", PopcornIcons.Settings),
    ;

    companion object {
        val Start = Home
    }
}
