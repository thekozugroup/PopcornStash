package com.thekozugroup.popcornstash.feature.downloads

import com.thekozugroup.popcornstash.data.api.Category
import com.thekozugroup.popcornstash.data.api.ContentType
import com.thekozugroup.popcornstash.data.api.InboxAction
import com.thekozugroup.popcornstash.data.api.ItemDetail
import com.thekozugroup.popcornstash.data.api.Library
import com.thekozugroup.popcornstash.data.api.MediaKind
import com.thekozugroup.popcornstash.data.api.Place
import com.thekozugroup.popcornstash.data.api.Structure
import com.thekozugroup.popcornstash.data.api.Suggestion
import com.thekozugroup.popcornstash.data.api.Waiting
import com.thekozugroup.popcornstash.feature.library.captureClock
import java.util.Locale

// Where a waiting file can go and how it will be laid out there (docs/architecture.md
// sections 4 and 8, Inbox accept; the web's components/inbox/places.ts and MoveFields.tsx).
// The target is always what the person sees on screen: a library they can see that fits
// the file, or Other files. Accept sends exactly that.

/** A place a file can be sent: a library, or Other files ([library] null). */
data class ImportTarget(val library: Library?) {
    val place: Place get() = if (library != null) Place.Library else Place.OtherFiles
    val name: String get() = library?.name ?: "Other files"

    /** Stable key for pickers and saved choices. */
    val key: String get() = library?.let { "lib:${it.id}" } ?: OTHER_FILES_KEY

    companion object {
        const val OTHER_FILES_KEY = "other_files"
        val OtherFiles = ImportTarget(null)
    }
}

/** The owner's words for each kind of new file. */
fun claimWords(category: Category): String? = when (category) {
    Category.Movie -> "movies"
    Category.Episode -> "TV episodes"
    Category.HomeVideo -> "camera videos"
    Category.Photo -> "photos"
    else -> null
}

/** Photos and videos into a Photos library, videos into a Video library; nothing else fits a library. */
fun fileFits(kind: MediaKind, library: Library): Boolean = when (library.contentType) {
    ContentType.Photos -> kind == MediaKind.Photo || kind == MediaKind.Video
    ContentType.Video -> kind == MediaKind.Video
    ContentType.Unknown -> false
}

/** What a file becomes in a library (what the server will make of it). */
fun categoryIn(library: Library, wanted: Category): Category {
    if (library.contentType == ContentType.Photos) return Category.Photo
    if (library.structure == Structure.Episodes) return Category.Episode
    if (wanted == Category.Movie || wanted == Category.HomeVideo) return wanted
    return if (Category.HomeVideo in library.claims && Category.Movie !in library.claims) Category.HomeVideo else Category.Movie
}

/**
 * The person may send the file only to the library that claims its kind. The person who
 * added a file and owners may choose any library they see that fits, or Other files; anyone
 * else sees a waiting file only because a library they see takes its kind. The API does not
 * say who added a file, so for a member the row decides, as on the web: a suggestion that
 * names a library may be someone else's file and offers that library alone.
 */
fun isClaimOnly(item: ItemDetail, isOwner: Boolean): Boolean =
    !isOwner && item.suggestion?.place == Place.Library && item.suggestion.libraryId != null

/** Where this person may send the file, in the owner's order, Other files last. */
fun targetChoices(item: ItemDetail, libraries: List<Library>, isOwner: Boolean): List<ImportTarget> {
    if (isClaimOnly(item, isOwner)) {
        return libraries.filter { it.id == item.suggestion?.libraryId }.map { ImportTarget(it) }
    }
    return libraries.filter { fileFits(item.kind, it) }.map { ImportTarget(it) } + ImportTarget.OtherFiles
}

/** The suggestion as a target, when the person can see it. */
fun suggestedTarget(item: ItemDetail, libraries: List<Library>): ImportTarget? {
    val suggestion = item.suggestion ?: return null
    return when (suggestion.place) {
        Place.OtherFiles -> ImportTarget.OtherFiles
        Place.Library -> libraries.firstOrNull { it.id == suggestion.libraryId }?.let { ImportTarget(it) }
        else -> null
    }
}

/**
 * What the row shows: the person's own choice ([chosenKey], once they changed it), else
 * the suggestion when it is one of the choices, else the only choice there is.
 */
fun rowTarget(choices: List<ImportTarget>, suggested: ImportTarget?, chosenKey: String?, chosen: Boolean): ImportTarget? {
    if (chosen) return choices.firstOrNull { it.key == chosenKey }
    if (suggested != null) choices.firstOrNull { it.key == suggested.key }?.let { return it }
    return choices.singleOrNull()
}

/** The row's reason line from the server's `waiting`, or null when there is nothing to add. */
fun waitingReason(suggestion: Suggestion?, isOwner: Boolean): String? {
    if (suggestion == null) return "PopcornStash could not tell what this file is."
    return when (suggestion.waiting) {
        Waiting.ChooseLibrary -> {
            val words = claimWords(suggestion.category)
            if (isOwner && words != null) "No library takes $words yet." else "Choose a library for this file."
        }
        Waiting.FolderUnavailable -> "The library’s folder is not available."
        Waiting.LowConfidence -> suggestion.reason.ifBlank { null }
        else -> null
    }
}

// ------------------------------------------------------------- the edit sheet ----

/** The edit sheet's fields as typed. */
data class MoveForm(
    val targetKey: String?,
    /** In a One-offs library: a movie or a camera video. */
    val category: Category,
    val title: String,
    val year: String,
    val show: String,
    val season: String,
    val episode: String,
)

enum class MoveField { Target, Title, Year, Show, Season, Episode }

fun initialMoveForm(item: ItemDetail, target: ImportTarget?): MoveForm {
    val s = item.suggestion
    return MoveForm(
        targetKey = target?.key,
        category = s?.category ?: item.category,
        title = s?.title?.ifBlank { null } ?: item.title,
        year = (s?.year ?: item.year)?.toString().orEmpty(),
        show = (s?.showName ?: item.showName).orEmpty(),
        season = (s?.season ?: item.season)?.toString().orEmpty(),
        episode = (s?.episode ?: item.episode)?.toString().orEmpty(),
    )
}

private fun number(text: String): Int? = text.trim().takeIf { it.isNotEmpty() }?.toIntOrNull()

private fun isBadNumber(text: String): Boolean = text.trim().isNotEmpty() && text.trim().toIntOrNull()?.takeIf { it >= 0 } == null

/** The kind the file becomes at [target] with these details. */
fun categoryAt(target: ImportTarget, form: MoveForm): Category =
    target.library?.let { categoryIn(it, form.category) } ?: Category.Other

/** The live preview: "Will move to Kids", where it lands (owners) and what is still missing. */
data class MovePreview(
    val title: String,
    val folder: String?,
    val fileName: String?,
    val hint: String?,
    /** The field [hint] asks for; while that field shows its own error, the hint is left out. */
    val asksFor: MoveField? = null,
) {
    /** The hint, unless the field it asks for already says so under it (said once: DESIGN.md). */
    fun hintBeside(errors: Map<MoveField, String>): String? = hint?.takeIf { asksFor == null || asksFor !in errors }
}

private fun cleanName(value: String): String =
    value.replace(Regex("[\\\\/:*?\"<>|]+"), " ").replace(Regex("\\s+"), " ").trim()

private fun episodeCode(season: Int, episode: Int) = String.format(Locale.ROOT, "S%02dE%02d", season, episode)

fun previewMove(target: ImportTarget, form: MoveForm, item: ItemDetail, showFolder: Boolean): MovePreview {
    val name = item.relPath.substringAfterLast('/')
    val extension = name.substringAfterLast('.', "").let { if (it.isEmpty() || it == name) "" else ".$it" }
    val title = "Will move to ${target.name}"
    val root = target.library?.folder ?: if (target.library == null) "Other" else null
    fun folderOf(parts: List<String>) = if (showFolder && root != null) (listOf(root) + parts).joinToString("/") + "/" else null
    return when (categoryAt(target, form)) {
        Category.Movie -> {
            val clean = cleanName(form.title)
            if (clean.isEmpty()) return MovePreview(title, null, null, "Add a title to see where it goes.", MoveField.Title)
            val year = number(form.year)
            val folder = if (year != null) "$clean ($year)" else clean
            MovePreview(title, folderOf(listOf(folder)), "$folder$extension", null)
        }
        Category.Episode -> {
            val show = cleanName(form.show)
            if (show.isEmpty()) return MovePreview(title, null, null, "Add the show name to see where it goes.", MoveField.Show)
            val episode = number(form.episode)
                ?: return MovePreview(title, null, null, "Add the episode number to see where it goes.", MoveField.Episode)
            val season = number(form.season) ?: 1
            MovePreview(
                title,
                folderOf(listOf(show, String.format(Locale.ROOT, "Season %02d", season))),
                "$show - ${episodeCode(season, episode)}$extension",
                null,
            )
        }
        Category.HomeVideo, Category.Photo -> {
            // The month on the camera's clock, as the server files it (docs/architecture.md section 4,
            // Capture time): the suggestion's, else the file's own; never moved to this phone's zone.
            val suggestion = item.suggestion
            val taken = captureClock(suggestion?.takenLocal, suggestion?.takenAt) ?: captureClock(item.takenLocal, item.takenAt)
            val month = taken?.let { listOf(it.year.toString(), String.format(Locale.ROOT, "%02d", it.monthValue)) }
            val hint = when {
                month != null -> null
                categoryAt(target, form) == Category.Photo -> "Filed by the month it was taken."
                else -> "Filed by the month it was recorded."
            }
            MovePreview(title, month?.let(::folderOf), null, hint)
        }
        else -> MovePreview(title, folderOf(emptyList()), null, null)
    }
}

/** Checks the form; the request to send, or the errors to show under the fields. */
data class MoveCheck(val errors: Map<MoveField, String>, val action: InboxAction?)

fun validateMove(form: MoveForm, choices: List<ImportTarget>): MoveCheck {
    val target = choices.firstOrNull { it.key == form.targetKey }
        ?: return MoveCheck(mapOf(MoveField.Target to "Choose a library for this file."), null)
    val errors = mutableMapOf<MoveField, String>()
    val category = categoryAt(target, form)
    if (category == Category.Movie) {
        if (form.title.isBlank()) errors[MoveField.Title] = "Enter the movie title."
        val year = number(form.year)
        if (form.year.isNotBlank() && (year == null || year !in 1900..2099)) errors[MoveField.Year] = "Use a four-digit year, like 1995."
    }
    if (category == Category.Episode) {
        if (form.show.isBlank()) errors[MoveField.Show] = "Enter the show name."
        if (isBadNumber(form.season)) errors[MoveField.Season] = "Enter the season as a number."
        if (form.episode.isBlank() || isBadNumber(form.episode)) errors[MoveField.Episode] = "Enter the episode number."
    }
    if (errors.isNotEmpty()) return MoveCheck(errors, null)
    val base = InboxAction(action = "accept", place = target.place, libraryId = target.library?.id)
    val action = when (category) {
        Category.Movie -> base.copy(category = Category.Movie, title = form.title.trim(), year = number(form.year))
        Category.Episode -> base.copy(
            category = Category.Episode,
            showName = form.show.trim(),
            season = number(form.season) ?: 1,
            episode = number(form.episode),
        )
        Category.HomeVideo -> base.copy(category = Category.HomeVideo)
        else -> base
    }
    return MoveCheck(emptyMap(), action)
}

/** Where a server error that names a request field goes in the form. */
fun fieldOfServerError(field: String?): MoveField? = when (field) {
    "library_id", "place" -> MoveField.Target
    "title" -> MoveField.Title
    "year" -> MoveField.Year
    "show_name" -> MoveField.Show
    "season" -> MoveField.Season
    "episode" -> MoveField.Episode
    else -> null
}

/** A series needs a show and an episode; without them a plain Accept would be refused. */
fun needsDetails(target: ImportTarget, item: ItemDetail): Boolean {
    if (target.library?.structure != Structure.Episodes) return false
    val form = initialMoveForm(item, target)
    return form.show.isBlank() || number(form.episode) == null
}

/** The request for a plain Accept: exactly the place and library the row shows. */
fun acceptAction(target: ImportTarget): InboxAction =
    InboxAction(action = "accept", place = target.place, libraryId = target.library?.id)
