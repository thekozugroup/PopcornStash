package com.thekozugroup.popcornstash.feature.settings

import com.thekozugroup.popcornstash.data.api.Category
import com.thekozugroup.popcornstash.data.api.ContentType
import com.thekozugroup.popcornstash.data.api.FolderState
import com.thekozugroup.popcornstash.data.api.Library
import com.thekozugroup.popcornstash.data.api.Role
import com.thekozugroup.popcornstash.data.api.Structure
import com.thekozugroup.popcornstash.data.api.UnassignedFolder
import com.thekozugroup.popcornstash.data.api.User
import com.thekozugroup.popcornstash.data.api.Visibility
import com.thekozugroup.popcornstash.feature.library.MediaFormat
import java.text.NumberFormat

// Libraries in the owner's words, the same words as the web's Settings, Libraries and its
// editor (web/src/lib/libraries.ts, LibraryEditorPage.tsx; docs/architecture.md section 8).

/** The most libraries a stash can have (docs/architecture.md section 8, Libraries). */
const val LIBRARY_LIMIT = 24

/** Longest library name the server takes. */
const val LIBRARY_NAME_MAX = 40

/** The kinds of new file a library can take, in the editor's order. */
val ClaimKinds: List<Category> = listOf(Category.Movie, Category.Episode, Category.HomeVideo, Category.Photo)

/** The owner's word for a kind of new file, as a plural in a sentence: "TV episodes". */
fun claimWords(kind: Category): String = when (kind) {
    Category.Movie -> "movies"
    Category.Episode -> "TV episodes"
    Category.HomeVideo -> "camera videos"
    Category.Photo -> "photos"
    else -> "other files"
}

/** The checkbox label for a kind: "Camera videos". */
fun claimLabel(kind: Category): String = claimWords(kind).replaceFirstChar { it.uppercase() }

/** Whether a kind of new file fits a library of this type and structure (section 4, claims). */
fun claimFits(kind: Category, contentType: ContentType, structure: Structure?): Boolean = when (kind) {
    Category.Photo -> contentType == ContentType.Photos
    Category.Episode -> contentType == ContentType.Video && structure == Structure.Episodes
    Category.Movie, Category.HomeVideo -> contentType == ContentType.Video && structure == Structure.OneOffs
    else -> false
}

fun fittingClaims(contentType: ContentType, structure: Structure?): List<Category> =
    ClaimKinds.filter { claimFits(it, contentType, structure) }

/** "movies", "movies and camera videos", "Ana, Ben and Tomas". */
fun joinWords(words: List<String>): String = when (words.size) {
    0 -> ""
    1 -> words[0]
    else -> words.dropLast(1).joinToString(", ") + " and " + words.last()
}

/** "Takes TV episodes", "Takes nothing new". */
fun takesLine(claims: List<Category>): String {
    val known = ClaimKinds.filter { it in claims }
    return if (known.isEmpty()) "Takes nothing new" else "Takes ${joinWords(known.map(::claimWords))}"
}

/** "Videos, arranged as series": what a library that holds files is, said as a fact (it cannot change). */
fun holdsLine(contentType: ContentType, structure: Structure?): String = when {
    contentType == ContentType.Photos -> "Photos"
    structure == Structure.Episodes -> "Videos, arranged as series"
    else -> "Videos, arranged as one-offs"
}

/** "Series", "One-offs" or "Photos": what a library is. */
fun kindLine(contentType: ContentType, structure: Structure?): String = when {
    contentType == ContentType.Photos -> "Photos"
    structure == Structure.Episodes -> "Series"
    else -> "One-offs"
}

/**
 * "Everyone", "Only Ana and Tomas", "Only owners". [people] is null while the list of people
 * has not loaded; a library shown to some people then reads "Only some people".
 */
fun whoSeesLine(visibility: Visibility?, memberIds: List<String>?, people: List<User>?): String {
    if (visibility != Visibility.Selected) return "Everyone"
    if (people == null) return if (memberIds.isNullOrEmpty()) "Only owners" else "Only some people"
    val names = memberIds.orEmpty().mapNotNull { id -> people.firstOrNull { it.id == id && it.role == Role.Member }?.displayName }
    return if (names.isEmpty()) "Only owners" else "Only ${joinWords(names)}"
}

/** The warning beside a library whose folder cannot be used right now (section 3.1). */
fun folderProblem(state: FolderState?): String? = when (state) {
    FolderState.Missing -> "Folder missing"
    FolderState.ReadOnly -> "Folder read-only"
    else -> null
}

/** "1 video", "3,214 photos", "2 other files". */
fun countOf(count: Long, singular: String): String {
    val number = NumberFormat.getIntegerInstance().format(count)
    return if (count == 1L) "$number $singular" else "$number ${singular}s"
}

/** What is indexed in a folder that is not a library: "124 videos", "12 photos"; "Nothing indexed yet". */
fun folderCounts(folder: UnassignedFolder): List<String> {
    val counts = listOfNotNull(
        folder.videoCount.takeIf { it > 0 }?.let { countOf(it, "video") },
        folder.photoCount.takeIf { it > 0 }?.let { countOf(it, "photo") },
        folder.otherCount.takeIf { it > 0 }?.let { countOf(it, "other file") },
    )
    return counts.ifEmpty { listOf("Nothing indexed yet") }
}

/** The icon a new library starts on until the owner picks one. */
fun defaultIconFor(contentType: ContentType): String = if (contentType == ContentType.Photos) "image" else "film"

/**
 * Where a kind of new file goes now, beside its checkbox in the editor. [holder] is the library
 * that takes it today, [thisLibraryId] the library being edited (null while making one).
 */
fun claimDescription(holder: Library?, thisLibraryId: String?, checked: Boolean): String {
    val isHere = thisLibraryId != null && holder?.id == thisLibraryId
    return when {
        checked && holder != null && !isHere -> "Moves here from ${holder.name}"
        checked && isHere -> "Sorted here now"
        checked -> "Sorted here from now on"
        isHere -> "Sorted here now. Unchecked, new ones wait in Inbox"
        holder != null -> "Sorted into ${holder.name} now"
        else -> "Not sorted anywhere: these wait in Inbox"
    }
}

/** How many of a folder's photos and other files a library of this type and claims will not show (section 4). */
fun hiddenCounts(folder: UnassignedFolder, contentType: ContentType, structure: Structure?, claims: List<Category>): Pair<Long, Long> {
    val moviesOnly = Category.Movie in claims && Category.HomeVideo !in claims
    val photosShown = contentType == ContentType.Photos || (structure == Structure.OneOffs && !moviesOnly)
    return (if (photosShown) 0L else folder.photoCount) to folder.otherCount
}

/** "124 videos and 12 photos": what the library will show of a folder; null when nothing. */
fun shownWords(folder: UnassignedFolder, contentType: ContentType, structure: Structure?, claims: List<Category>): String? {
    val (hiddenPhotos, _) = hiddenCounts(folder, contentType, structure, claims)
    val parts = listOfNotNull(
        folder.videoCount.takeIf { it > 0 }?.let { countOf(it, "video") },
        (folder.photoCount - hiddenPhotos).takeIf { it > 0 }?.let { countOf(it, "photo") },
    )
    return parts.takeIf { it.isNotEmpty() }?.let(::joinWords)
}

/** Folder names up to this long are kept on one line inside a sentence. */
private const val FOLDER_KEPT_WHOLE = 24

/**
 * A folder as a sentence names it, "Grandma's Slides/": a name that fits on a line is never
 * split across two ("Grandma's" at the end of one line, "Slides/" on the next); a longer one
 * wraps at its spaces like the rest of the sentence.
 */
fun folderInSentence(name: String): String =
    (if (name.length <= FOLDER_KEPT_WHOLE) MediaFormat.unbroken(name) else name) + "/"

/** "Files stay in Holidays/. Nothing is moved. 12 photos there will not show in this library." */
fun existingFolderSentence(folderName: String, folder: UnassignedFolder?, contentType: ContentType, structure: Structure?, claims: List<Category>): String {
    val stays = "Files stay in ${folderInSentence(folderName)}. Nothing is moved."
    if (folder == null) return stays
    val (photos, other) = hiddenCounts(folder, contentType, structure, claims)
    if (photos == 0L && other == 0L) return stays
    val hidden = joinWords(listOfNotNull(photos.takeIf { it > 0 }?.let { countOf(it, "photo") }, other.takeIf { it > 0 }?.let { countOf(it, "other file") }))
    return "$stays $hidden there will not show in this library."
}

/** Who will see an existing folder's files once it is a library: "Only owners will see its 124 videos." */
fun whoWillSeeSentence(shown: String, visibility: Visibility, chosenNames: List<String>): String = when {
    visibility == Visibility.Everyone -> "Everyone will see its $shown."
    chosenNames.isEmpty() -> "Only owners will see its $shown."
    else -> "${joinWords(chosenNames)} will see its $shown."
}

/** What deleting a library does, for its confirmation. */
fun deleteSentence(library: Library): String =
    "Takes ${library.name} out of the app. Its files stay in ${library.folder?.let(::folderInSentence) ?: "its folder/"} and only owners will see them."

/** The note after a library was deleted. */
fun deletedNotice(library: Library): String =
    "${library.name} is no longer in the app. Its files stay in ${library.folder?.let(::folderInSentence) ?: "its folder/"}."
