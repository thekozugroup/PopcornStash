package com.thekozugroup.popcornstash.navigation

import com.thekozugroup.popcornstash.ui.icons.LucideGlyph
import com.thekozugroup.popcornstash.ui.icons.PopcornIcons

/**
 * The app's four destinations, in toolbar order from top to bottom. Home is the start tab. The order decides which way a tab change slides.
 */
enum class Tab(val label: String, val glyph: LucideGlyph) {
    Downloads("Downloads", PopcornIcons.Downloads),
    Home("Home", PopcornIcons.Home),
    Search("Search", PopcornIcons.Search),
    Settings("Settings", PopcornIcons.Settings),
    ;

    companion object {
        val Start = Home
    }
}
