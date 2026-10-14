package com.thekozugroup.popcornstash.feature.settings

import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.Category
import com.thekozugroup.popcornstash.data.api.ContentType
import com.thekozugroup.popcornstash.data.api.LibraryChanges
import com.thekozugroup.popcornstash.data.api.StashEvent
import com.thekozugroup.popcornstash.data.api.Structure
import com.thekozugroup.popcornstash.data.api.Visibility
import com.thekozugroup.popcornstash.data.api.baseHttpClient
import com.thekozugroup.popcornstash.data.repo.StashData
import com.thekozugroup.popcornstash.testing.bodyText
import com.thekozugroup.popcornstash.testing.path
import com.thekozugroup.popcornstash.testing.withinRealSeconds
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import mockwebserver3.MockWebServer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import com.thekozugroup.popcornstash.feature.settings.SettingsFixtures as F

/** Making and editing a library, with the web editor's rules, against a server shaped like the real one. */
@OptIn(ExperimentalCoroutinesApi::class)
class LibraryEditorControllerTest {
    private val server = MockWebServer()
    private val events = MutableSharedFlow<StashEvent>(extraBufferCapacity = 16)
    private lateinit var routes: Routes
    private lateinit var api: ApiClient

    @Before
    fun setUp() {
        server.start()
        api = ApiClient(server.url("/"), baseHttpClient("t"))
    }

    @After
    fun tearDown() = server.close()

    private fun serve(routes: Routes) {
        this.routes = routes
        server.dispatcher = routes
    }

    private fun TestScope.editor(id: String? = null, folder: String? = null): LibraryEditorController {
        val scope = CoroutineScope(UnconfinedTestDispatcher(testScheduler))
        return LibraryEditorController(api, StashData(api, events, scope, settleMillis = 0), scope, id, folder)
    }

    private suspend fun LibraryEditorController.until(check: (LibraryEditorUi) -> Boolean): LibraryEditorUi =
        withinRealSeconds { state.first(check) }

    /** Loaded, with the libraries and people it needs to say who takes what. */
    private suspend fun LibraryEditorController.ready(libraries: Int = 4): LibraryEditorUi =
        until { it.load == EditorLoad.Ready && it.libraries.size == libraries && it.people.isNotEmpty() }

    private fun json(text: String): JsonElement = Json.parseToJsonElement(text)

    @Test
    fun aNewLibraryStartsBlankAndItsFolderFollowsTheName() = runTest {
        serve(F.ownerServer())
        val editor = editor()
        val state = editor.ready()
        assertEquals(LibraryDraft(), state.draft)
        assertEquals(3, state.folders.size)
        assertEquals(listOf("Tomas", "Ines"), state.members.map { it.displayName })
        editor.setName("  Kids")
        assertEquals("Kids", editor.state.value.draft!!.newFolder)
        editor.setNewFolder("Kids stuff")
        editor.setName("Kids films")
        assertEquals("Kids stuff", editor.state.value.draft!!.newFolder)
        // At most 40 characters.
        editor.setName("x".repeat(60))
        assertEquals(LIBRARY_NAME_MAX, editor.state.value.draft!!.name.length)
    }

    @Test
    fun aLibraryMadeFromAFolderTakesItsNameAndTypeAndStartsShownToNobody() = runTest {
        serve(F.ownerServer())
        val editor = editor(folder = "Family 2019")
        val draft = editor.ready().draft!!
        assertEquals("Family 2019", draft.name)
        assertEquals(ContentType.Photos, draft.contentType)
        assertNull(draft.structure)
        assertEquals("image", draft.icon)
        assertEquals(FolderMode.Existing, draft.folderMode)
        assertEquals("Family 2019", draft.existingFolder)
        assertEquals(Visibility.Selected, draft.visibility)
        assertTrue(draft.memberIds.isEmpty())
        assertEquals("Family 2019", editor.state.value.chosenFolder?.name)
    }

    @Test
    fun choosingAnExistingFolderMakesItPrivateUntilTheOwnerChooses() = runTest {
        serve(F.ownerServer())
        val editor = editor()
        editor.ready()
        editor.setFolderMode(FolderMode.Existing)
        editor.chooseFolder("Grandma's scans")
        val draft = editor.state.value.draft!!
        assertEquals("Grandma's scans", draft.existingFolder)
        assertEquals("Grandma's scans", draft.name)
        assertEquals(Visibility.Selected, draft.visibility)
        // The owner can still share it with everyone.
        editor.setVisibility(Visibility.Everyone)
        assertEquals(Visibility.Everyone, editor.state.value.draft!!.visibility)
        // A folder that is not in the list is not taken.
        editor.chooseFolder("Nope")
        assertEquals("Grandma's scans", editor.state.value.draft!!.existingFolder)
    }

    @Test
    fun withNoFolderToTakeOnlyANewFolderIsOffered() = runTest {
        serve(F.ownerServer().only("GET", "/api/libraries/folders", F.NO_FOLDERS))
        val editor = editor()
        editor.ready()
        editor.setFolderMode(FolderMode.Existing)
        assertEquals(FolderMode.New, editor.state.value.draft!!.folderMode)
    }

    @Test
    fun changingTheTypeUnchecksKindsThatNoLongerFitAndTheIconFollowsUntilPicked() = runTest {
        serve(F.ownerServer())
        val editor = editor()
        editor.ready()
        editor.toggleClaim(Category.Movie)
        editor.toggleClaim(Category.Photo) // does not fit one-offs
        assertEquals(listOf(Category.Movie), editor.state.value.draft!!.claims)
        editor.setType(ContentType.Photos, null)
        var draft = editor.state.value.draft!!
        assertTrue(draft.claims.isEmpty())
        assertNull(draft.structure)
        assertEquals("image", draft.icon)
        editor.setIcon("camera")
        editor.setType(ContentType.Video, Structure.Episodes)
        draft = editor.state.value.draft!!
        assertEquals(Structure.Episodes, draft.structure)
        assertEquals("camera", draft.icon)
    }

    @Test
    fun aLibraryThatHoldsFilesKeepsItsType() = runTest {
        serve(F.ownerServer().on("GET", "/api/libraries/${F.MOVIES_ID}", F.MOVIES))
        val editor = editor(F.MOVIES_ID)
        val state = editor.ready()
        assertTrue(state.locked)
        assertEquals("Movies", state.draft!!.name)
        assertEquals(listOf(Category.Movie), state.draft!!.claims)
        editor.setType(ContentType.Photos, null)
        assertEquals(ContentType.Video, editor.state.value.draft!!.contentType)
    }

    @Test
    fun makingALibrarySendsTheFormAndFinishes() = runTest {
        serve(F.ownerServer().on("POST", "/api/libraries", F.KIDS, code = 201))
        val editor = editor()
        editor.ready()
        editor.setName("Kids")
        editor.setIcon("baby")
        editor.setType(ContentType.Video, Structure.Episodes)
        editor.setVisibility(Visibility.Selected)
        editor.toggleMember("u-tomas")
        editor.save()
        val request = routes.nextWrite()
        assertEquals("POST /api/libraries", "${request.method} ${request.path}")
        assertEquals(
            json(
                """{"name":"Kids","icon":"baby","content_type":"video","structure":"episodes","claims":[],
                "folder":{"mode":"new","name":"Kids"},"visibility":"selected","member_ids":["u-tomas"]}""",
            ),
            json(request.bodyText),
        )
        assertEquals(EditorFinished("Kids is ready."), editor.until { it.finished != null }.finished)
    }

    @Test
    fun takingAKindFromAnotherLibraryIsConfirmedFirst() = runTest {
        serve(F.ownerServer().on("POST", "/api/libraries", F.KIDS, code = 201))
        val editor = editor()
        editor.ready()
        editor.setName("Films")
        editor.toggleClaim(Category.Movie)
        editor.save()
        val move = editor.state.value.confirmingClaims
        assertEquals(ClaimMove(listOf(Category.Movie), listOf("Movies")), move)
        editor.cancelClaims()
        assertNull(editor.state.value.confirmingClaims)
        assertFalse(editor.state.value.saving)
        editor.save()
        editor.confirmClaims()
        val request = routes.nextWrite()
        assertEquals(json("""["movie"]"""), json(request.bodyText).let { (it as kotlinx.serialization.json.JsonObject)["claims"]!! })
        editor.until { it.finished != null }
    }

    @Test
    fun anEmptyNameOrFolderIsCaughtBeforeAnythingIsSent() = runTest {
        serve(F.ownerServer())
        val editor = editor()
        editor.ready()
        editor.save()
        assertEquals(
            mapOf(EditorField.Name to "Give the library a name.", EditorField.Folder to "Name the new folder."),
            editor.state.value.errors,
        )
        editor.setName("Kids")
        assertTrue("editing clears the errors", editor.state.value.errors.isEmpty())
        editor.setFolderMode(FolderMode.Existing)
        editor.save()
        assertEquals(mapOf(EditorField.Folder to "Choose a folder."), editor.state.value.errors)
        assertTrue(routes.drain().none { it.startsWith("POST") })
    }

    @Test
    fun theServersMessageLandsUnderTheFieldItNames() = runTest {
        serve(
            F.ownerServer()
                .on("POST", "/api/libraries", """{"error":{"code":"conflict","message":"A library called Movies already exists.","field":"name"}}""", 409)
                .on("POST", "/api/libraries", """{"error":{"code":"conflict","message":"Nothing more fits: a stash can have 24 libraries."}}""", 409),
        )
        val editor = editor()
        editor.ready()
        editor.setName("Movies")
        editor.setNewFolder("Movies 2")
        editor.save()
        var state = editor.until { !it.saving && it.errors.isNotEmpty() }
        assertEquals(mapOf(EditorField.Name to "A library called Movies already exists."), state.errors)
        editor.setName("Films")
        editor.save()
        state = editor.until { !it.saving && it.formError != null }
        assertEquals("Nothing more fits: a stash can have 24 libraries.", state.formError)
        assertNull(state.finished)
    }

    @Test
    fun anEditSendsOnlyWhatChanged() = runTest {
        serve(
            F.ownerServer(F.FIVE)
                .on("GET", "/api/libraries/${F.KIDS_ID}", F.KIDS)
                .on("PATCH", "/api/libraries/${F.KIDS_ID}", F.KIDS),
        )
        val editor = editor(F.KIDS_ID)
        val state = editor.ready(libraries = 5)
        assertFalse("an empty library can change its type", state.locked)
        editor.setName("Little ones")
        editor.toggleMember("u-ines")
        editor.save()
        val request = routes.nextWrite()
        assertEquals("PATCH /api/libraries/${F.KIDS_ID}", "${request.method} ${request.path}")
        assertEquals(json("""{"name":"Little ones","member_ids":["u-tomas","u-ines"]}"""), json(request.bodyText))
        assertEquals(EditorFinished("Changes saved."), editor.until { it.finished != null }.finished)
    }

    @Test
    fun anEditThatChangesNothingJustCloses() = runTest {
        serve(F.ownerServer(F.FIVE).on("GET", "/api/libraries/${F.KIDS_ID}", F.KIDS))
        val editor = editor(F.KIDS_ID)
        editor.ready(libraries = 5)
        editor.save()
        assertEquals(EditorFinished(null), editor.until { it.finished != null }.finished)
        assertTrue(routes.drain().none { it.startsWith("PATCH") })
    }

    @Test
    fun aTypeChangeAlwaysNamesTheClaims() {
        val kids = F.libraries().first { it.id == F.KIDS_ID }
        val draft = LibraryDraft.from(kids).copy(contentType = ContentType.Photos, structure = null)
        assertEquals(
            LibraryChanges(contentType = ContentType.Photos, structure = null, structureSet = true, claims = emptyList()),
            LibraryEditorController.changesBetween(kids, draft),
        )
        assertNull(LibraryEditorController.changesBetween(kids, LibraryDraft.from(kids)))
    }

    @Test
    fun deletingTakesTheLibraryOutAndSaysTheFilesStay() = runTest {
        serve(
            F.ownerServer(F.FIVE)
                .on("GET", "/api/libraries/${F.KIDS_ID}", F.KIDS)
                .on("DELETE", "/api/libraries/${F.KIDS_ID}", code = 204),
        )
        val editor = editor(F.KIDS_ID)
        editor.ready(libraries = 5)
        editor.askDelete()
        assertTrue(editor.state.value.confirmingDelete)
        editor.cancelDelete()
        assertFalse(editor.state.value.confirmingDelete)
        editor.askDelete()
        editor.confirmDelete()
        assertEquals("DELETE /api/libraries/${F.KIDS_ID}", routes.nextWrite().let { "${it.method} ${it.path}" })
        assertEquals(
            EditorFinished("Kids is no longer in the app. Its files stay in Kids/."),
            editor.until { it.finished != null }.finished,
        )
    }

    @Test
    fun aLibraryThatIsGoneSaysSo() = runTest {
        serve(
            F.ownerServer().on(
                "GET",
                "/api/libraries/${F.KIDS_ID}",
                """{"error":{"code":"not_found","message":"That library no longer exists."}}""",
                404,
            ),
        )
        val editor = editor(F.KIDS_ID)
        val load = editor.until { it.load is EditorLoad.Failed }.load
        assertEquals(EditorLoad.Failed("This library is not here any more.", gone = true), load)
    }

    @Test
    fun aLibraryDeletedElsewhereWhileOpenStopsTheForm() = runTest {
        serve(F.ownerServer(F.FIVE).on("GET", "/api/libraries/${F.KIDS_ID}", F.KIDS))
        val editor = editor(F.KIDS_ID)
        editor.ready(libraries = 5)
        routes.only("GET", "/api/libraries", F.FOUR)
        events.emit(StashEvent.LibrariesChanged)
        val load = editor.until { it.load is EditorLoad.Failed }.load
        assertEquals(EditorLoad.Failed("This library is not here any more.", gone = true), load)
    }
}
