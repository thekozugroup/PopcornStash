package com.thekozugroup.popcornstash.ui.icons

/**
 * The icons an owner can give a library: the server's curated list (`GET /api/libraries/icons`,
 * docs/architecture.md section 8, `LIBRARY_ICONS` in stash-core), in picker order, each with the
 * plain name the web uses too (web/src/lib/libraryIcons.ts) for the picker's tooltip and
 * TalkBack. A name the app does not know (a newer server) draws [fallback], the `folder` glyph.
 */
object LibraryIcons {
    data class Entry(val name: String, val glyph: LucideGlyph, val label: String)

    val entries: List<Entry> = listOf(
        Entry("film", LucideGlyphs.Film, "Film"),
        Entry("clapperboard", LucideGlyphs.Clapperboard, "Clapperboard"),
        Entry("tv", LucideGlyphs.Tv, "TV"),
        Entry("popcorn", LucideGlyphs.Popcorn, "Popcorn"),
        Entry("projector", LucideGlyphs.Projector, "Projector"),
        Entry("video", LucideGlyphs.Video, "Video camera"),
        Entry("drama", LucideGlyphs.Drama, "Theatre masks"),
        Entry("image", LucideGlyphs.Image, "Picture"),
        Entry("images", LucideGlyphs.Images, "Pictures"),
        Entry("camera", LucideGlyphs.Camera, "Camera"),
        Entry("aperture", LucideGlyphs.Aperture, "Aperture"),
        Entry("users", LucideGlyphs.Users, "People"),
        Entry("baby", LucideGlyphs.Baby, "Baby"),
        Entry("heart", LucideGlyphs.Heart, "Heart"),
        Entry("cake", LucideGlyphs.Cake, "Cake"),
        Entry("party-popper", LucideGlyphs.PartyPopper, "Party popper"),
        Entry("graduation-cap", LucideGlyphs.GraduationCap, "Graduation cap"),
        Entry("gift", LucideGlyphs.Gift, "Gift"),
        Entry("plane", LucideGlyphs.Plane, "Plane"),
        Entry("tree-palm", LucideGlyphs.TreePalm, "Palm tree"),
        Entry("mountain-snow", LucideGlyphs.MountainSnow, "Mountain"),
        Entry("tent", LucideGlyphs.Tent, "Tent"),
        Entry("sun", LucideGlyphs.Sun, "Sun"),
        Entry("snowflake", LucideGlyphs.Snowflake, "Snowflake"),
        Entry("gamepad-2", LucideGlyphs.Gamepad2, "Game controller"),
        Entry("music", LucideGlyphs.Music, "Music"),
        Entry("book-open", LucideGlyphs.BookOpen, "Book"),
        Entry("trophy", LucideGlyphs.Trophy, "Trophy"),
        Entry("paw-print", LucideGlyphs.PawPrint, "Paw print"),
        Entry("bike", LucideGlyphs.Bike, "Bike"),
        Entry("star", LucideGlyphs.Star, "Star"),
    )

    val names: List<String> = entries.map { it.name }

    /**
     * What a name the app does not know draws. Not in the list an owner picks from, so a folder
     * glyph never stands for a library's chosen icon (folders not in a library use it too).
     */
    val fallback: Entry = Entry("folder", LucideGlyphs.Folder, "Folder")

    private val byName = entries.associateBy { it.name }

    /** The entry for [name], or the folder entry for a name this app does not know. */
    fun forName(name: String?): Entry = byName[name] ?: fallback

    fun glyph(name: String?): LucideGlyph = forName(name).glyph
}
