package com.thekozugroup.popcornstash.feature.settings

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.ApiException
import com.thekozugroup.popcornstash.data.api.Category
import com.thekozugroup.popcornstash.data.api.ContentType
import com.thekozugroup.popcornstash.data.api.FolderChoice
import com.thekozugroup.popcornstash.data.api.Library
import com.thekozugroup.popcornstash.data.api.LibraryChanges
import com.thekozugroup.popcornstash.data.api.LibraryCreate
import com.thekozugroup.popcornstash.data.api.Role
import com.thekozugroup.popcornstash.data.api.Structure
import com.thekozugroup.popcornstash.data.api.UnassignedFolder
import com.thekozugroup.popcornstash.data.api.User
import com.thekozugroup.popcornstash.data.api.Visibility
import com.thekozugroup.popcornstash.data.repo.Loadable
import com.thekozugroup.popcornstash.data.repo.StashData
import com.thekozugroup.popcornstash.ui.icons.LibraryIcons
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** The parts of the editor a server error can name (`error.field`, docs/architecture.md section 8). */
enum class EditorField { Name, Icon, Type, Claims, Folder, Visibility }

/** Where a server error's `field` sits in the form. */
fun editorFieldOf(field: String?): EditorField? = when (field) {
    "name" -> EditorField.Name
    "icon" -> EditorField.Icon
    "content_type", "structure" -> EditorField.Type
    "claims" -> EditorField.Claims
    "folder", "folder.mode", "folder.name" -> EditorField.Folder
    "visibility", "member_ids" -> EditorField.Visibility
    else -> null
}

enum class FolderMode { New, Existing }

/** The form as the owner has filled it in so far. */
data class LibraryDraft(
    val name: String = "",
    val icon: String = "film",
    /** The owner picked an icon: changing the type no longer changes it. */
    val iconTouched: Boolean = false,
    val contentType: ContentType = ContentType.Video,
    val structure: Structure? = Structure.OneOffs,
    val claims: List<Category> = emptyList(),
    val folderMode: FolderMode = FolderMode.New,
    val newFolder: String = "",
    /** The owner edited the folder name: it no longer follows the library's name. */
    val newFolderTouched: Boolean = false,
    val existingFolder: String? = null,
    val visibility: Visibility = Visibility.Everyone,
    val memberIds: List<String> = emptyList(),
) {
    companion object {
        /** An existing library as the form starts. */
        fun from(library: Library) = LibraryDraft(
            name = library.name,
            icon = library.icon,
            iconTouched = true,
            contentType = library.contentType,
            structure = library.structure,
            claims = ClaimKinds.filter { it in library.claims },
            folderMode = FolderMode.Existing,
            newFolderTouched = true,
            existingFolder = library.folder,
            visibility = library.visibility ?: Visibility.Everyone,
            memberIds = library.memberIds.orEmpty(),
        )

        /**
         * A new library. Made from a folder that is already there ([folderName]), it takes the
         * folder's name and suggested type and starts on "Only some people" with nobody chosen:
         * the folder's files were seen by owners only until now.
         */
        fun new(folder: UnassignedFolder?, folderName: String?): LibraryDraft {
            if (folderName == null) return LibraryDraft()
            val contentType = folder?.suggested?.contentType?.takeIf { it != ContentType.Unknown } ?: ContentType.Video
            return LibraryDraft(
                name = folderName.take(LIBRARY_NAME_MAX),
                icon = defaultIconFor(contentType),
                contentType = contentType,
                structure = if (contentType == ContentType.Video) folder?.suggested?.structure ?: Structure.OneOffs else null,
                folderMode = FolderMode.Existing,
                existingFolder = folderName,
                visibility = Visibility.Selected,
            )
        }
    }
}

sealed interface EditorLoad {
    data object Loading : EditorLoad

    /** [gone]: the library is not there any more, so trying again would not help. */
    data class Failed(val message: String, val gone: Boolean) : EditorLoad

    data object Ready : EditorLoad
}

/** Kinds the save would take from other libraries, to confirm first. */
data class ClaimMove(val kinds: List<Category>, val from: List<String>)

/** The editor closed after a save or a delete; [message] goes in a snackbar. */
data class EditorFinished(val message: String?)

data class LibraryEditorUi(
    val isNew: Boolean,
    /** The library being edited; null while making one. */
    val libraryId: String? = null,
    val load: EditorLoad = EditorLoad.Loading,
    /** The library being edited, as the server last sent it. */
    val library: Library? = null,
    val draft: LibraryDraft? = null,
    /** The server's icon list, in picker order. */
    val icons: List<String> = LibraryIcons.names,
    /** Folders that are not a library yet (making a library only). */
    val folders: List<UnassignedFolder> = emptyList(),
    /** Why the folders could not be listed, when making a library from one. */
    val foldersError: String? = null,
    val people: List<User> = emptyList(),
    /** Every library, for which one takes each kind of new file now. */
    val libraries: List<Library> = emptyList(),
    val errors: Map<EditorField, String> = emptyMap(),
    /** A save that failed without naming a field. */
    val formError: String? = null,
    val deleteError: String? = null,
    val saving: Boolean = false,
    val deleting: Boolean = false,
    val confirmingClaims: ClaimMove? = null,
    val confirmingDelete: Boolean = false,
    val finished: EditorFinished? = null,
) {
    val busy: Boolean get() = saving || deleting

    /** Type and structure can change only while the library holds nothing (section 8, Edit). */
    val locked: Boolean get() = !isNew && library?.isEmpty == false

    /** The people an owner can share a library with; owners see every library anyway. */
    val members: List<User> get() = people.filter { it.role == Role.Member }

    /** The library that takes [kind] now, if any. */
    fun holderOf(kind: Category): Library? = libraries.firstOrNull { kind in it.claims }

    /** The folder chosen when making a library from one that is already there. */
    val chosenFolder: UnassignedFolder?
        get() = draft?.takeIf { isNew && it.folderMode == FolderMode.Existing }?.existingFolder
            ?.let { name -> folders.firstOrNull { it.name == name } }

    /** The stash already has as many libraries as it can. */
    val atLimit: Boolean get() = isNew && libraries.size >= LIBRARY_LIMIT

    /** The library's name from the list, for the top bar while the library itself loads. */
    val knownName: String? get() = libraryId?.let { id -> libraries.firstOrNull { it.id == id }?.name }
}

/**
 * Making or editing one library (android/DESIGN.md, Library editor), with the web editor's
 * fields, rules and words: name and icon, what it holds (locked once it holds files), the kinds
 * of new file it takes (taking one from another library is confirmed first), its folder (new or
 * one already there; fixed once made), and who sees it. Errors land under the field the server
 * names. Deleting never touches the files.
 */
class LibraryEditorController(
    private val api: ApiClient,
    private val data: StashData,
    private val scope: CoroutineScope,
    private val libraryId: String?,
    private val fromFolder: String? = null,
) : LibraryEditorActions {
    private val _state = MutableStateFlow(LibraryEditorUi(isNew = libraryId == null, libraryId = libraryId))
    val state: StateFlow<LibraryEditorUi> = _state.asStateFlow()

    init {
        scope.launch { data.libraries.collect(::onLibraries) }
        // Who takes each kind now must be current before the owner moves one.
        data.refreshLibraries()
        load()
    }

    /** Loads the library (or the folders, for a new one), the icon list and the people. */
    override fun load() {
        _state.update { it.copy(load = EditorLoad.Loading) }
        scope.launch {
            try {
                val icons = api.libraryIcons()
                if (icons.isNotEmpty()) _state.update { it.copy(icons = icons) }
            } catch (_: ApiException) {
                // The app's own copy of the list is the same list.
            }
        }
        scope.launch {
            try {
                val people = api.users()
                _state.update { it.copy(people = people) }
            } catch (_: ApiException) {
                // Without the names, "Only some people" still works for the people already chosen.
            }
        }
        scope.launch {
            if (libraryId == null) loadNew() else loadExisting(libraryId)
        }
    }

    private suspend fun loadNew() {
        try {
            val folders = api.unassignedFolders()
            _state.update { state ->
                state.copy(
                    folders = folders,
                    foldersError = null,
                    draft = state.draft ?: LibraryDraft.new(folders.firstOrNull { it.name == fromFolder }, fromFolder),
                    load = EditorLoad.Ready,
                )
            }
        } catch (e: ApiException) {
            // The folders are needed only to take one; a new folder still works.
            _state.update { state ->
                state.copy(
                    foldersError = if (fromFolder != null) e.message else null,
                    draft = state.draft ?: LibraryDraft.new(null, fromFolder),
                    load = EditorLoad.Ready,
                )
            }
        }
    }

    private suspend fun loadExisting(id: String) {
        try {
            val library = api.library(id)
            _state.update { state ->
                state.copy(library = library, draft = state.draft ?: LibraryDraft.from(library), load = EditorLoad.Ready)
            }
        } catch (e: ApiException) {
            val gone = e.isNotFound
            _state.update {
                it.copy(load = EditorLoad.Failed(if (gone) "This library is not here any more." else e.message, gone))
            }
        }
    }

    private fun onLibraries(libraries: Loadable<List<Library>>) {
        val list = libraries.valueOrNull ?: return
        _state.update { state ->
            val current = libraryId?.let { id -> list.firstOrNull { it.id == id } }
            // Deleted somewhere else while open: say so instead of offering a save that cannot work.
            val gone = libraryId != null && libraries is Loadable.Ready && !libraries.refreshing && current == null &&
                state.load == EditorLoad.Ready && state.finished == null && !state.deleting
            state.copy(
                libraries = list,
                // Whether it holds files can change while the form is open (a scan, a sort).
                library = if (current != null && state.library != null) state.library.copy(isEmpty = current.isEmpty) else state.library,
                load = if (gone) EditorLoad.Failed("This library is not here any more.", gone = true) else state.load,
            )
        }
    }

    // ------------------------------------------------------------------ editing ----

    private fun edit(change: (LibraryEditorUi, LibraryDraft) -> LibraryDraft?) = _state.update { state ->
        val draft = state.draft
        if (draft == null || state.busy) return@update state
        val next = change(state, draft) ?: return@update state
        state.copy(draft = next, errors = emptyMap())
    }

    override fun setName(value: String) = edit { _, draft ->
        val name = value.take(LIBRARY_NAME_MAX)
        draft.copy(name = name, newFolder = if (draft.newFolderTouched) draft.newFolder else name.trim())
    }

    override fun setIcon(icon: String) = edit { _, draft -> draft.copy(icon = icon, iconTouched = true) }

    /** Videos or photos; for videos, series or one-offs. Kinds that no longer fit are unchecked. */
    override fun setType(contentType: ContentType, structure: Structure?) = edit { state, draft ->
        if (state.locked) return@edit null
        val fixedStructure = if (contentType == ContentType.Video) structure ?: draft.structure ?: Structure.OneOffs else null
        draft.copy(
            contentType = contentType,
            structure = fixedStructure,
            claims = draft.claims.filter { claimFits(it, contentType, fixedStructure) },
            icon = if (draft.iconTouched) draft.icon else defaultIconFor(contentType),
        )
    }

    override fun toggleClaim(kind: Category) = edit { _, draft ->
        if (!claimFits(kind, draft.contentType, draft.structure)) return@edit null
        val claims = if (kind in draft.claims) draft.claims - kind else draft.claims + kind
        draft.copy(claims = ClaimKinds.filter { it in claims })
    }

    override fun setFolderMode(mode: FolderMode) = edit { state, draft ->
        if (!state.isNew || (mode == FolderMode.Existing && state.folders.isEmpty())) return@edit null
        draft.copy(folderMode = mode)
    }

    override fun setNewFolder(value: String) = edit { _, draft -> draft.copy(newFolder = value, newFolderTouched = true) }

    /**
     * Takes a folder that is already there. Its files were seen by owners only, so the library
     * starts on "Only some people" (section 8, Create, visibility), and an empty name takes the folder's.
     */
    override fun chooseFolder(name: String) = edit { state, draft ->
        val folder = state.folders.firstOrNull { it.name == name } ?: return@edit null
        draft.copy(
            existingFolder = folder.name,
            name = draft.name.ifBlank { folder.name.take(LIBRARY_NAME_MAX) },
            visibility = if (draft.visibility == Visibility.Everyone) Visibility.Selected else draft.visibility,
        )
    }

    override fun setVisibility(visibility: Visibility) = edit { _, draft -> draft.copy(visibility = visibility) }

    override fun toggleMember(id: String) = edit { _, draft ->
        draft.copy(memberIds = if (id in draft.memberIds) draft.memberIds - id else draft.memberIds + id)
    }

    // ------------------------------------------------------------------- saving ----

    /** Checks the form, asks to confirm taking kinds from other libraries, then saves. */
    override fun save() {
        val state = _state.value
        val draft = state.draft ?: return
        if (state.busy || state.load != EditorLoad.Ready) return
        val errors = validate(state, draft)
        if (errors.isNotEmpty()) {
            _state.update { it.copy(errors = errors) }
            return
        }
        val moving = draft.claims.mapNotNull { kind -> state.holderOf(kind)?.takeIf { it.id != state.library?.id }?.let { kind to it } }
        if (moving.isNotEmpty()) {
            _state.update { it.copy(confirmingClaims = ClaimMove(moving.map { it.first }, moving.map { it.second.name }.distinct())) }
            return
        }
        commit()
    }

    override fun confirmClaims() {
        if (_state.value.confirmingClaims == null) return
        _state.update { it.copy(confirmingClaims = null) }
        commit()
    }

    override fun cancelClaims() = _state.update { it.copy(confirmingClaims = null) }

    private fun validate(state: LibraryEditorUi, draft: LibraryDraft): Map<EditorField, String> = buildMap {
        if (draft.name.isBlank()) put(EditorField.Name, "Give the library a name.")
        if (state.isNew && draft.folderMode == FolderMode.New && draft.newFolder.isBlank()) put(EditorField.Folder, "Name the new folder.")
        if (state.isNew && draft.folderMode == FolderMode.Existing && draft.existingFolder == null) put(EditorField.Folder, "Choose a folder.")
    }

    private fun commit() {
        val state = _state.value
        val draft = state.draft ?: return
        _state.update { it.copy(saving = true, formError = null, errors = emptyMap()) }
        scope.launch {
            try {
                val message = if (state.isNew) create(draft) else state.library?.let { update(it, draft) }
                _state.update { it.copy(saving = false, finished = EditorFinished(message)) }
            } catch (e: ApiException) {
                val field = editorFieldOf(e.field)
                _state.update {
                    if (field != null) {
                        it.copy(saving = false, errors = mapOf(field to e.message))
                    } else {
                        it.copy(saving = false, formError = e.message)
                    }
                }
            }
        }
    }

    private suspend fun create(draft: LibraryDraft): String {
        val created = api.createLibrary(
            LibraryCreate(
                name = draft.name.trim(),
                icon = draft.icon,
                contentType = draft.contentType,
                structure = if (draft.contentType == ContentType.Video) draft.structure else null,
                claims = draft.claims,
                folder = when (draft.folderMode) {
                    FolderMode.New -> FolderChoice("new", draft.newFolder.trim())
                    FolderMode.Existing -> FolderChoice("existing", draft.existingFolder.orEmpty())
                },
                visibility = draft.visibility,
                memberIds = if (draft.visibility == Visibility.Selected) draft.memberIds else emptyList(),
            ),
        )
        data.refreshLibraries().join()
        return "${created.name} is ready."
    }

    /** Sends only what changed; null when nothing did. */
    private suspend fun update(library: Library, draft: LibraryDraft): String? {
        val changes = changesBetween(library, draft) ?: return null
        api.editLibrary(library.id, changes)
        data.refreshLibraries().join()
        return "Changes saved."
    }

    // ----------------------------------------------------------------- deleting ----

    override fun askDelete() = _state.update { if (it.library != null && !it.busy) it.copy(confirmingDelete = true, deleteError = null) else it }

    override fun cancelDelete() = _state.update { it.copy(confirmingDelete = false) }

    /** Takes the library out of the app. Its folder and files stay where they are. */
    override fun confirmDelete() {
        val library = _state.value.library ?: return
        if (_state.value.busy) return
        _state.update { it.copy(confirmingDelete = false, deleting = true, deleteError = null) }
        scope.launch {
            try {
                api.deleteLibrary(library.id)
                _state.update { it.copy(deleting = false, finished = EditorFinished(deletedNotice(library))) }
                data.refreshLibraries()
            } catch (e: ApiException) {
                _state.update { it.copy(deleting = false, deleteError = e.message) }
            }
        }
    }

    companion object {
        /** The edit for `PATCH /api/libraries/:id`: only the fields that changed; null when none did. */
        fun changesBetween(library: Library, draft: LibraryDraft): LibraryChanges? {
            val name = draft.name.trim().takeIf { it != library.name }
            val icon = draft.icon.takeIf { it != library.icon }
            val structure = if (draft.contentType == ContentType.Video) draft.structure else null
            val typeChanged = draft.contentType != library.contentType || structure != library.structure
            val claimsChanged = draft.claims.toSet() != library.claims.filter { it in ClaimKinds }.toSet()
            val visibility = draft.visibility.takeIf { it != (library.visibility ?: Visibility.Everyone) }
            val members = draft.memberIds.takeIf { it.toSet() != library.memberIds.orEmpty().toSet() }
            val changes = LibraryChanges(
                name = name,
                icon = icon,
                contentType = if (typeChanged) draft.contentType else null,
                structure = if (typeChanged) structure else null,
                structureSet = typeChanged,
                // A type change always names the claims, so the server checks them against the new type.
                claims = if (claimsChanged || typeChanged) draft.claims else null,
                visibility = visibility,
                memberIds = members,
            )
            return changes.takeIf { it != LibraryChanges() }
        }
    }
}

/** Keeps one editor's form across tab switches and rotation; it ends when its page closes. */
class LibraryEditorViewModel(api: ApiClient, data: StashData, libraryId: String?, fromFolder: String?) : ViewModel() {
    val controller = LibraryEditorController(api, data, viewModelScope, libraryId, fromFolder)
}
