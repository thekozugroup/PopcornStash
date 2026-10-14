package com.thekozugroup.popcornstash.feature.downloads

import com.thekozugroup.popcornstash.data.api.Category
import com.thekozugroup.popcornstash.data.api.InboxAction
import com.thekozugroup.popcornstash.data.api.Place
import com.thekozugroup.popcornstash.data.api.Suggestion
import com.thekozugroup.popcornstash.data.api.Waiting
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** Where a waiting file may go (docs/architecture.md section 8, Inbox accept; web places.ts). */
class ImportTargetsTest {
    private val s = DownloadsSamples

    @Test
    fun aVideoFitsEveryVideoAndPhotosLibraryThenOtherFiles() {
        val names = targetChoices(s.lowConfidence, s.libraries, isOwner = true).map { it.name }
        assertEquals(listOf("Movies", "Shows", "Kids", "Home Videos", "Photos", "Other files"), names)
    }

    @Test
    fun aPhotoFitsOnlyPhotosLibrariesAndADocumentOnlyOtherFiles() {
        assertEquals(listOf("Photos", "Other files"), targetChoices(s.photo, s.libraries, true).map { it.name })
        assertEquals(listOf("Other files"), targetChoices(s.document, s.libraries, true).map { it.name })
    }

    @Test
    fun someoneElsesFileOffersAMemberOnlyTheLibraryThatClaimsIt() {
        assertTrue(isClaimOnly(s.episode, isOwner = false))
        assertEquals(listOf("Shows"), targetChoices(s.episode, s.libraries, isOwner = false).map { it.name })
        // With no claiming library, the member can only be seeing it because they added it.
        val unclaimed = s.episode.copy(suggestion = s.episode.suggestion!!.copy(place = null, libraryId = null, waiting = Waiting.ChooseLibrary))
        assertFalse(isClaimOnly(unclaimed, isOwner = false))
        assertEquals(6, targetChoices(unclaimed, s.libraries, isOwner = false).size)
    }

    @Test
    fun theRowShowsTheChoiceThenTheSuggestionThenTheOnlyPlace() {
        val choices = targetChoices(s.lowConfidence, s.libraries, true)
        val suggested = suggestedTarget(s.lowConfidence, s.libraries)
        assertEquals("Movies", rowTarget(choices, suggested, null, chosen = false)?.name)
        assertEquals("Kids", rowTarget(choices, suggested, "lib:lib-kids", chosen = true)?.name)
        assertNull(rowTarget(choices, null, null, chosen = false))
        val document = targetChoices(s.document, s.libraries, true)
        assertEquals("Other files", rowTarget(document, null, null, chosen = false)?.name)
    }

    @Test
    fun aSuggestionForALibraryThePersonCannotSeeShowsNoLibrary() {
        val hidden = s.lowConfidence.copy(suggestion = s.lowConfidence.suggestion!!.copy(libraryId = "lib-hidden"))
        assertNull(suggestedTarget(hidden, s.libraries))
    }

    @Test
    fun theReasonLineFollowsWaiting() {
        val choose = Suggestion(category = Category.Episode, waiting = Waiting.ChooseLibrary)
        assertEquals("No library takes TV episodes yet.", waitingReason(choose, isOwner = true))
        assertEquals("Choose a library for this file.", waitingReason(choose, isOwner = false))
        assertEquals("The library’s folder is not available.", waitingReason(s.photo.suggestion, true))
        assertEquals("Not sure this is a movie.", waitingReason(s.lowConfidence.suggestion, true))
        assertNull(waitingReason(s.episode.suggestion, true))
        assertEquals("PopcornStash could not tell what this file is.", waitingReason(null, true))
    }

    @Test
    fun aSeriesWithoutAShowOrEpisodeNeedsDetailsFirst() {
        val shows = ImportTarget(s.shows)
        assertFalse(needsDetails(shows, s.episode))
        assertTrue(needsDetails(shows, s.lowConfidence))
        assertFalse(needsDetails(ImportTarget(s.movies), s.lowConfidence))
    }

    @Test
    fun aPlainAcceptNamesThePlaceAndLibraryTheRowShows() {
        assertEquals(InboxAction("accept", Place.Library, "lib-movies"), acceptAction(ImportTarget(s.movies)))
        assertEquals(InboxAction("accept", Place.OtherFiles, null), acceptAction(ImportTarget.OtherFiles))
    }

    @Test
    fun aMovieNeedsATitleAndAPlausibleYear() {
        val choices = targetChoices(s.lowConfidence, s.libraries, true)
        val form = initialMoveForm(s.lowConfidence, ImportTarget(s.movies)).copy(title = " ", year = "95")
        val check = validateMove(form, choices)
        assertEquals(mapOf(MoveField.Title to "Enter the movie title.", MoveField.Year to "Use a four-digit year, like 1995."), check.errors)
        val good = validateMove(form.copy(title = "Paper Boats ", year = "2011"), choices)
        assertEquals(
            InboxAction("accept", Place.Library, "lib-movies", category = Category.Movie, title = "Paper Boats", year = 2011),
            good.action,
        )
    }

    @Test
    fun anEpisodeNeedsAShowAndAnEpisodeNumberAndSeasonOneByDefault() {
        val choices = targetChoices(s.lowConfidence, s.libraries, true)
        val form = initialMoveForm(s.lowConfidence, ImportTarget(s.kids))
        assertEquals(
            mapOf(MoveField.Show to "Enter the show name.", MoveField.Episode to "Enter the episode number."),
            validateMove(form.copy(show = "", episode = ""), choices).errors,
        )
        val action = validateMove(form.copy(show = "Puddle Club", season = "", episode = "3"), choices).action
        assertEquals(
            InboxAction("accept", Place.Library, "lib-kids", category = Category.Episode, showName = "Puddle Club", season = 1, episode = 3),
            action,
        )
    }

    @Test
    fun withoutATargetTheFormAsksForALibrary() {
        val form = initialMoveForm(s.lowConfidence, null)
        assertEquals(mapOf(MoveField.Target to "Choose a library for this file."), validateMove(form, emptyList()).errors)
    }

    @Test
    fun aCameraVideoInAOneOffsLibrarySaysSo() {
        val choices = targetChoices(s.lowConfidence, s.libraries, true)
        // A file the classifier could not name becomes the library's own kind; a suggested movie stays a movie.
        val suggestedMovie = initialMoveForm(s.lowConfidence, ImportTarget(s.homeVideos))
        assertEquals(Category.Movie, categoryAt(ImportTarget(s.homeVideos), suggestedMovie))
        val form = suggestedMovie.copy(category = Category.Other)
        assertEquals(Category.HomeVideo, categoryAt(ImportTarget(s.homeVideos), form))
        assertEquals(
            InboxAction("accept", Place.Library, "lib-home-videos", category = Category.HomeVideo),
            validateMove(form, choices).action,
        )
    }

    @Test
    fun thePreviewSaysWhereItGoesAndOwnersSeeTheFolder() {
        val movie = initialMoveForm(s.lowConfidence, ImportTarget(s.movies)).copy(title = "Paper Boats", year = "2011")
        val owner = previewMove(ImportTarget(s.movies), movie, s.lowConfidence, showFolder = true)
        assertEquals(MovePreview("Will move to Movies", "Movies/Paper Boats (2011)/", "Paper Boats (2011).mkv", null), owner)
        val member = previewMove(ImportTarget(s.movies), movie, s.lowConfidence, showFolder = false)
        assertNull(member.folder)
        val episode = previewMove(ImportTarget(s.shows), initialMoveForm(s.episode, ImportTarget(s.shows)), s.episode, true)
        assertEquals("Shows/Puddle Club/Season 01/", episode.folder)
        assertEquals("Puddle Club - S01E01.mkv", episode.fileName)
        val photo = previewMove(ImportTarget(s.photos), initialMoveForm(s.photo, ImportTarget(s.photos)), s.photo, true)
        assertEquals("Photos/2026/10/", photo.folder)
        val other = previewMove(ImportTarget.OtherFiles, initialMoveForm(s.document, ImportTarget.OtherFiles), s.document, true)
        assertEquals(MovePreview("Will move to Other files", "Other/", null, null), other)
        val untitled = previewMove(ImportTarget(s.movies), movie.copy(title = ""), s.lowConfidence, true)
        assertEquals("Add a title to see where it goes.", untitled.hint)
    }

    @Test
    fun serverFieldsMapOntoTheForm() {
        assertEquals(MoveField.Target, fieldOfServerError("library_id"))
        assertEquals(MoveField.Show, fieldOfServerError("show_name"))
        assertNull(fieldOfServerError(null))
    }
}
