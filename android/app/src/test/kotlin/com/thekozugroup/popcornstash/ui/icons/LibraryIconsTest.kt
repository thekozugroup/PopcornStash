package com.thekozugroup.popcornstash.ui.icons

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

/**
 * The app's library icons are the server's curated list (stash-core LIBRARY_ICONS), in the same
 * order, with the same plain names as the web's (web/src/lib/libraryIcons.ts). Gradle declares both
 * files as inputs of the unit tests, so a change to either runs this again.
 */
class LibraryIconsTest {
    private val repoRoot = File(System.getProperty("popcornstash.repoRoot") ?: "../..")

    private fun serverIcons(): List<String> {
        val defs = repoRoot.resolve("crates/stash-core/src/library/defs.rs").readText()
        val block = defs.substringAfter("pub const LIBRARY_ICONS: &[&str] = &[").substringBefore("];")
        return Regex("\"([a-z0-9-]+)\"").findAll(block).map { it.groupValues[1] }.toList()
    }

    /** `film: {icon: Film, label: 'Film'}` and `'party-popper': {icon: PartyPopper, label: 'Party popper'}`. */
    private fun webLabels(): List<Pair<String, String>> {
        val web = repoRoot.resolve("web/src/lib/libraryIcons.ts").readText()
        val table = web.substringAfter("const ICONS: Record<string, IconEntry> = {").substringBefore("};")
        return Regex("""'?([a-z0-9-]+)'?: \{icon: \w+, label: '([^']+)'\}""").findAll(table)
            .map { it.groupValues[1] to it.groupValues[2] }
            .toList()
    }

    @Test
    fun sameNamesInTheSameOrderAsTheServer() {
        val server = serverIcons()
        assertEquals(31, server.size)
        assertEquals(server, LibraryIcons.names)
    }

    @Test
    fun samePlainNamesAsTheWeb() {
        assertEquals(webLabels(), LibraryIcons.entries.map { it.name to it.label })
    }

    @Test
    fun aFolderIsNeverAChosenIcon() {
        // The folder glyph means "not a library" and "an icon this app does not know".
        assertTrue("folder" !in serverIcons())
        assertTrue("folder" !in LibraryIcons.names)
        assertEquals(LucideGlyphs.Folder, LibraryIcons.fallback.glyph)
    }

    @Test
    fun aNameTheAppDoesNotKnowDrawsAFolder() {
        assertEquals("folder", LibraryIcons.forName("rocket-from-a-newer-server").name)
        assertEquals("folder", LibraryIcons.forName(null).name)
        assertEquals(LucideGlyphs.Film, LibraryIcons.glyph("film"))
    }

    @Test
    fun everyIconHasAPlainNameAndADrawing() {
        LibraryIcons.entries.forEach { entry ->
            assertTrue(entry.name, entry.label.isNotBlank() && entry.label.first().isUpperCase())
            assertTrue(entry.name, entry.glyph.paths.isNotEmpty())
            assertEquals(entry.name, entry.glyph.name)
        }
    }

    @Test
    fun noLibraryIconLooksLikeAFixedPlace() {
        val fixed = listOf(PopcornIcons.Home, PopcornIcons.OtherFiles, PopcornIcons.ReadyToImport, PopcornIcons.Downloads, PopcornIcons.Settings)
        fixed.forEach { assertTrue(it.name, it.name !in LibraryIcons.names) }
    }
}
