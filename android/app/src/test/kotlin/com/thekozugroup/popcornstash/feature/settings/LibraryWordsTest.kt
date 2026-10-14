package com.thekozugroup.popcornstash.feature.settings

import com.thekozugroup.popcornstash.data.api.Category
import com.thekozugroup.popcornstash.data.api.ContentType
import com.thekozugroup.popcornstash.data.api.FolderState
import com.thekozugroup.popcornstash.data.api.Structure
import com.thekozugroup.popcornstash.data.api.Visibility
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** The owner's words for libraries, the same as the web's. */
class LibraryWordsTest {
    private val libraries = SettingsFixtures.libraries()
    private val movies = libraries.first { it.name == "Movies" }
    private val kids = libraries.first { it.name == "Kids" }
    private val folders = SettingsFixtures.folders()

    @Test
    fun kindsFitTheirTypeAndStructure() {
        assertEquals(listOf(Category.Movie, Category.HomeVideo), fittingClaims(ContentType.Video, Structure.OneOffs))
        assertEquals(listOf(Category.Episode), fittingClaims(ContentType.Video, Structure.Episodes))
        assertEquals(listOf(Category.Photo), fittingClaims(ContentType.Photos, null))
        assertFalse(claimFits(Category.Other, ContentType.Video, Structure.OneOffs))
        assertFalse(claimFits(Category.Photo, ContentType.Video, Structure.OneOffs))
    }

    @Test
    fun rowLinesSayWhatALibraryIsTakesAndWhoSeesIt() {
        assertEquals("One-offs", kindLine(ContentType.Video, Structure.OneOffs))
        assertEquals("Series", kindLine(ContentType.Video, Structure.Episodes))
        assertEquals("Photos", kindLine(ContentType.Photos, null))
        assertEquals("Takes movies", takesLine(listOf(Category.Movie)))
        assertEquals("Takes movies and camera videos", takesLine(listOf(Category.HomeVideo, Category.Movie)))
        assertEquals("Takes nothing new", takesLine(listOf(Category.Unknown)))
        val people = SettingsFixtures.people()
        assertEquals("Everyone", whoSeesLine(Visibility.Everyone, listOf("u-tomas"), people))
        assertEquals("Only Tomas", whoSeesLine(Visibility.Selected, listOf("u-tomas"), people))
        assertEquals("Only Tomas and Ines", whoSeesLine(Visibility.Selected, listOf("u-tomas", "u-ines"), people))
        // Owners are never listed, and a person who left is not named.
        assertEquals("Only owners", whoSeesLine(Visibility.Selected, listOf("u-owner", "u-gone"), people))
        assertEquals("Only some people", whoSeesLine(Visibility.Selected, listOf("u-tomas"), null))
        assertEquals("Only owners", whoSeesLine(Visibility.Selected, emptyList(), null))
    }

    @Test
    fun countsAndJoins() {
        assertEquals("1 video", countOf(1, "video"))
        assertEquals("3,214 photos", countOf(3214, "photo"))
        assertEquals("2 other files", countOf(2, "other file"))
        assertEquals("", joinWords(emptyList()))
        assertEquals("Ana, Ben and Tomas", joinWords(listOf("Ana", "Ben", "Tomas")))
        assertEquals(listOf("12 videos", "20 photos", "4 other files"), folderCounts(folders[0]))
        assertEquals(listOf("Nothing indexed yet"), folderCounts(folders[0].copy(videoCount = 0, photoCount = 0, otherCount = 0)))
    }

    @Test
    fun eachKindSaysWhereItGoesNow() {
        assertEquals("Sorted into Movies now", claimDescription(movies, null, checked = false))
        assertEquals("Moves here from Movies", claimDescription(movies, null, checked = true))
        assertEquals("Sorted here from now on", claimDescription(null, null, checked = true))
        assertEquals("Not sorted anywhere: these wait in Inbox", claimDescription(null, kids.id, checked = false))
        assertEquals("Sorted here now", claimDescription(movies, movies.id, checked = true))
        assertEquals("Sorted here now. Unchecked, new ones wait in Inbox", claimDescription(movies, movies.id, checked = false))
    }

    @Test
    fun anExistingFolderSaysWhatStaysAndWhatTheTypeWillNotShow() {
        val family = folders[0]
        // A video series shows videos only: the photos and other files stay out of it.
        assertEquals(
            "Files stay in Family 2019/. Nothing is moved. 20 photos and 4 other files there will not show in this library.",
            existingFolderSentence("Family 2019", family, ContentType.Video, Structure.Episodes, emptyList()),
        )
        // One-offs that take camera videos show the photos too.
        assertEquals(
            "Files stay in Family 2019/. Nothing is moved. 4 other files there will not show in this library.",
            existingFolderSentence("Family 2019", family, ContentType.Video, Structure.OneOffs, listOf(Category.HomeVideo)),
        )
        // One-offs that take movies only show videos.
        assertEquals(20L to 4L, hiddenCounts(family, ContentType.Video, Structure.OneOffs, listOf(Category.Movie)))
        assertEquals(0L to 0L, hiddenCounts(folders[1], ContentType.Photos, null, emptyList()))
        assertEquals("Files stay in Grandma's scans/. Nothing is moved.", existingFolderSentence("Grandma's scans", folders[1], ContentType.Photos, null, emptyList()))
        assertEquals("12 videos and 20 photos", shownWords(family, ContentType.Photos, null, emptyList()))
        assertNull(shownWords(family.copy(videoCount = 0, photoCount = 0), ContentType.Photos, null, emptyList()))
    }

    @Test
    fun whoWillSeeAFoldersFiles() {
        assertEquals("Everyone will see its 312 photos.", whoWillSeeSentence("312 photos", Visibility.Everyone, emptyList()))
        assertEquals("Only owners will see its 312 photos.", whoWillSeeSentence("312 photos", Visibility.Selected, emptyList()))
        assertEquals("Tomas and Ines will see its 312 photos.", whoWillSeeSentence("312 photos", Visibility.Selected, listOf("Tomas", "Ines")))
    }

    @Test
    fun foldersAndDeletingInWords() {
        assertEquals("Folder missing", folderProblem(FolderState.Missing))
        assertEquals("Folder read-only", folderProblem(FolderState.ReadOnly))
        assertNull(folderProblem(FolderState.Ok))
        assertEquals("Takes Movies out of the app. Its files stay in Movies/ and only owners will see them.", deleteSentence(movies))
        assertEquals("Movies is no longer in the app. Its files stay in Movies/.", deletedNotice(movies))
        assertEquals("image", defaultIconFor(ContentType.Photos))
        assertEquals("film", defaultIconFor(ContentType.Video))
    }

    @Test
    fun ownersGetTheirWebPagesAndMembersTheirOwn() {
        val owner = webSettingsPages(com.thekozugroup.popcornstash.data.api.Role.Owner).map { it.label }
        assertEquals(listOf("Sorting and trash", "VPN", "People", "Devices", "Security", "System"), owner)
        val member = webSettingsPages(com.thekozugroup.popcornstash.data.api.Role.Member).map { it.path }
        assertEquals(listOf("settings/devices", "settings/security"), member)
        assertTrue("libraries are managed in the app", "Libraries" !in owner)
    }

    @Test
    fun signingOutNamesTheUploadsItStops() {
        val base = "PopcornStash forgets your sign-in and everything it kept on this phone."
        assertEquals(base, signOutSentence(0))
        assertEquals("$base 1 upload stops.", signOutSentence(1))
        assertEquals("$base 3 uploads stop.", signOutSentence(3))
    }
}
