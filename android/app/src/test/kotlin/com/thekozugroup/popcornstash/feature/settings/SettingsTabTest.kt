package com.thekozugroup.popcornstash.feature.settings

import com.thekozugroup.popcornstash.testing.hasPlainText
import androidx.activity.ComponentActivity
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsNotSelected
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isSelectable
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.longClick
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.thekozugroup.popcornstash.TabRequests
import com.thekozugroup.popcornstash.data.api.ApiClient
import com.thekozugroup.popcornstash.data.api.Role as PersonRole
import com.thekozugroup.popcornstash.data.api.StashEvent
import com.thekozugroup.popcornstash.data.api.User
import com.thekozugroup.popcornstash.data.api.baseHttpClient
import com.thekozugroup.popcornstash.data.repo.StashData
import com.thekozugroup.popcornstash.navigation.Tab
import com.thekozugroup.popcornstash.navigation.TabPage
import com.thekozugroup.popcornstash.navigation.ViewModelStores
import com.thekozugroup.popcornstash.testing.bodyText
import com.thekozugroup.popcornstash.testing.drawnTextColor
import com.thekozugroup.popcornstash.testing.path
import com.thekozugroup.popcornstash.ui.components.IconPicker
import com.thekozugroup.popcornstash.ui.theme.LightPopcornColors
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.serialization.json.Json
import mockwebserver3.MockWebServer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import com.thekozugroup.popcornstash.feature.settings.SettingsFixtures as F

/** The Settings tab against a server: the owner's list, the editor on top of it, and back. */
@RunWith(AndroidJUnit4::class)
@Config(qualifiers = "w411dp-h891dp-xxhdpi")
class SettingsTabTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val server = MockWebServer()
    private val events = MutableSharedFlow<StashEvent>(extraBufferCapacity = 16)
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val reselections = mutableIntStateOf(0)
    private lateinit var routes: Routes

    private val owner = User("u-owner", "Ana Ribeiro", PersonRole.Owner)
    private val member = User("u-tomas", "Tomas", PersonRole.Member)

    @Before
    fun setUp() = server.start()

    @After
    fun tearDown() {
        scope.cancel()
        server.close()
    }

    private fun show(routes: Routes, user: User = owner, requests: TabRequests? = null, uploadsRunning: Int = 0) {
        this.routes = routes
        server.dispatcher = routes
        val api = ApiClient(server.url("/"), baseHttpClient("t"))
        val services = SettingsServices(api, StashData(api, events, scope, settleMillis = 0), ViewModelStores())
        compose.setContent {
            PopcornTheme(darkTheme = false, reduceMotion = true) {
                SettingsTab(
                    page = TabPage(Tab.Settings, 120.dp, reselections.intValue),
                    user = user,
                    serverDisplay = "stash.example.com",
                    serverVersion = "0.2.0",
                    onOpenWeb = {},
                    onSignOut = {},
                    services = services,
                    open = requests?.settings,
                    onOpened = { requests?.settingsOpened() },
                    uploadsRunning = uploadsRunning,
                )
            }
        }
    }

    private fun waitFor(tag: String) = compose.waitUntil(5_000) { compose.onAllTagged(tag) > 0 }

    private fun waitForText(text: String) =
        compose.waitUntil(5_000) { compose.onAllNodes(hasText(text)).fetchSemanticsNodes().isNotEmpty() }

    private fun waitGone(tag: String) = compose.waitUntil(5_000) { compose.onAllTagged(tag) == 0 }

    private fun androidx.compose.ui.test.junit4.ComposeTestRule.onAllTagged(tag: String) =
        onAllNodes(hasTestTag(tag)).fetchSemanticsNodes().size

    private fun back() = compose.runOnUiThread { compose.activity.onBackPressedDispatcher.onBackPressed() }

    @Test
    fun anOwnerSeesTheLibrariesInOrderAndOpensOneToEdit() {
        show(F.ownerServer().on("GET", "/api/libraries/${F.MOVIES_ID}", F.MOVIES))
        waitFor("library row Photos")
        compose.onNodeWithText("Takes camera videos").assertIsDisplayed()
        compose.onNodeWithTag("library row Movies").performClick()
        waitFor("save library")
        compose.onNodeWithText("Save changes").assertExists()
        compose.onNodeWithText("The type can change only while the library is empty.").assertExists()
        // A library with files says what it holds as a fact in the text colour, not as greyed choices.
        compose.onNodeWithTag("holds fact").assertExists()
        compose.onNodeWithText("Videos, arranged as one-offs", useUnmergedTree = true).assertExists()
        assertEquals(LightPopcornColors.text, compose.onNodeWithText("Videos, arranged as one-offs", useUnmergedTree = true).drawnTextColor())
        back()
        waitFor("library row Movies")
        waitGone("library editor")
    }

    @Test
    fun makingALibraryShowsItInTheListWithANote() {
        show(F.ownerServer().on("POST", "/api/libraries", F.KIDS, code = 201))
        waitFor("make a library")
        compose.onNodeWithTag("make a library").performClick()
        waitFor("library name")
        compose.onNodeWithText("New library").assertIsDisplayed()
        compose.onNodeWithTag("library name").performTextInput("Kids")
        compose.onNodeWithTag("icon baby").performClick()
        compose.onNode(hasText("Series") and isSelectable()).performScrollTo().performClick()
        routes.only("GET", "/api/libraries", F.FIVE)
        compose.onNodeWithTag("save library").performScrollTo().performClick()
        val request = routes.nextWrite()
        assertEquals("POST /api/libraries", "${request.method} ${request.path}")
        val body = Json.parseToJsonElement(request.bodyText).toString()
        assertTrue(body, body.contains("\"icon\":\"baby\"") && body.contains("\"structure\":\"episodes\"") && body.contains("\"name\":\"Kids\""))
        waitForText("Kids is ready.")
        waitFor("library row Kids")
    }

    @Test
    fun aNameTheServerRefusesSaysWhyUnderTheName() {
        show(
            F.ownerServer().on(
                "POST",
                "/api/libraries",
                """{"error":{"code":"conflict","message":"A library called Movies already exists.","field":"name"}}""",
                409,
            ),
        )
        waitFor("make a library")
        compose.onNodeWithTag("make a library").performClick()
        waitFor("library name")
        compose.onNodeWithTag("library name").performTextInput("Movies")
        compose.onNodeWithTag("save library").performScrollTo().performClick()
        waitForText("A library called Movies already exists.")
        compose.onNodeWithText("A library called Movies already exists.").assertIsDisplayed()
    }

    @Test
    fun moveUpFromTheMenuSavesTheNewOrder() {
        show(F.ownerServer().on("PUT", "/api/libraries/order", F.list(F.SHOWS, F.MOVIES, F.HOME_VIDEOS, F.PHOTOS)))
        waitFor("library row Shows")
        compose.onNodeWithTag("more Shows").performClick()
        compose.onNodeWithText("Move up").performClick()
        val request = routes.nextWrite()
        assertEquals("PUT /api/libraries/order", "${request.method} ${request.path}")
        assertTrue(request.bodyText.startsWith("""{"ids":["${F.SHOWS_ID}","${F.MOVIES_ID}""""))
    }

    @Test
    fun rowsOfferMoveAndDeleteToTalkBack() {
        show(F.ownerServer())
        waitFor("library row Shows")
        val labels = { tag: String ->
            compose.onNodeWithTag(tag).fetchSemanticsNode().config.getOrNull(SemanticsActions.CustomActions).orEmpty().map { it.label }
        }
        assertEquals(listOf("Move down", "Delete"), labels("library row Movies"))
        assertEquals(listOf("Move up", "Move down", "Delete"), labels("library row Shows"))
        assertEquals(listOf("Move up", "Delete"), labels("library row Photos"))
        compose.onNodeWithContentDescription("More for Shows").assertExists()
    }

    @Test
    fun deletingAsksFirstAndSaysTheFilesStay() {
        show(F.ownerServer(F.FIVE).on("DELETE", "/api/libraries/${F.KIDS_ID}", code = 204))
        waitFor("library row Kids")
        compose.onNodeWithTag("more Kids").performClick()
        compose.onNodeWithText("Delete").performClick()
        compose.onNodeWithText("Delete Kids?").assertIsDisplayed()
        compose.onNode(hasPlainText("Takes Kids out of the app. Its files stay in Kids/ and only owners will see them.")).assertIsDisplayed()
        routes.only("GET", "/api/libraries", F.FOUR)
        compose.onNodeWithText("Delete library").performClick()
        assertEquals("DELETE /api/libraries/${F.KIDS_ID}", routes.nextWrite().let { "${it.method} ${it.path}" })
        waitForText("Kids is no longer in the app. Its files stay in Kids/.")
        waitGone("library row Kids")
    }

    @Test
    fun aFolderBecomesALibraryShownToNobodyUntilChosen() {
        show(F.ownerServer())
        waitFor("folder row Family 2019")
        compose.onNodeWithTag("folder row Family 2019").performScrollTo().performClick()
        waitFor("folder picker")
        compose.onNode(hasText("Only some people") and isSelectable()).assertIsSelected()
        compose.onNode(hasText("Everyone") and isSelectable()).assertIsNotSelected()
        compose.onNode(hasText("Photos") and isSelectable()).assertIsSelected()
        compose.onNodeWithText("Only owners will see its 12 videos and 20 photos.").assertExists()
    }

    @Test
    fun tappingSettingsAgainClosesTheEditor() {
        show(F.ownerServer())
        waitFor("make a library")
        compose.onNodeWithTag("make a library").performClick()
        waitFor("library editor")
        reselections.intValue++
        waitGone("library editor")
        waitFor("make a library")
    }

    @Test
    fun anEditorHomeAsksForOpensOverTheFirstPageOnce() {
        val requests = TabRequests()
        show(F.ownerServer().on("GET", "/api/libraries/${F.MOVIES_ID}", F.MOVIES), requests = requests)
        waitFor("library row Movies")
        compose.runOnUiThread { requests.openSettings(SettingsRoute.EditLibrary(F.MOVIES_ID)) }
        waitFor("save library")
        compose.onNodeWithText("Save changes").assertExists()
        compose.runOnIdle { assertEquals(null, requests.settings) }
        back()
        waitGone("library editor")
        waitFor("library row Movies")
    }

    @Test
    fun aNewLibraryHomeAsksForReplacesAnEditorAlreadyOpen() {
        val requests = TabRequests()
        show(F.ownerServer().on("GET", "/api/libraries/${F.MOVIES_ID}", F.MOVIES), requests = requests)
        waitFor("library row Movies")
        compose.onNodeWithTag("library row Movies").performClick()
        waitFor("save library")
        compose.runOnUiThread { requests.openSettings(SettingsRoute.NewLibrary()) }
        waitForText("New library")
        compose.onNodeWithText("Save changes").assertDoesNotExist()
        back()
        waitGone("library editor")
        waitFor("library row Movies")
    }

    @Test
    fun aMemberAskedForAnEditorStaysOnTheFirstPage() {
        val requests = TabRequests()
        show(F.ownerServer(), user = member, requests = requests)
        compose.runOnUiThread { requests.openSettings(SettingsRoute.NewLibrary()) }
        compose.runOnIdle { assertEquals(null, requests.settings) }
        assertEquals(0, compose.onAllTagged("library editor"))
        compose.onNodeWithText("Devices").assertIsDisplayed()
    }

    @Test
    fun signingOutSaysHowManyUploadsStop() {
        show(F.ownerServer(), user = member, uploadsRunning = 2)
        compose.onNodeWithText("Sign out").performScrollTo().performClick()
        compose.onNodeWithText("Sign out on this phone?").assertIsDisplayed()
        compose.onNodeWithText("PopcornStash forgets your sign-in and everything it kept on this phone. 2 uploads stop.").assertIsDisplayed()
    }

    @Test
    fun membersManageNoLibraries() {
        show(F.ownerServer(), user = member)
        compose.waitForIdle()
        compose.onNodeWithText("Devices").assertIsDisplayed()
        compose.onNodeWithText("Security").assertIsDisplayed()
        assertEquals(0, compose.onAllNodes(hasText("Libraries")).fetchSemanticsNodes().size)
        assertEquals(0, compose.onAllNodes(hasText("VPN")).fetchSemanticsNodes().size)
        assertTrue(routes.drain().none { it.contains("/api/libraries") || it.contains("/api/users") })
    }

    @Test
    fun theIconPickerIsOneGroupOfNamedRadioButtons() {
        var chosen = "film"
        compose.setContent {
            PopcornTheme(darkTheme = false, reduceMotion = true) {
                IconPicker(icons = listOf("film", "tv", "baby", "something-newer"), selected = chosen, onSelect = { chosen = it })
            }
        }
        compose.onNodeWithTag("icon film")
            .assertIsSelected()
            .assert(SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.RadioButton))
            .assert(SemanticsMatcher.expectValue(SemanticsProperties.ContentDescription, listOf("Film")))
        compose.onNodeWithTag("icon tv").assertIsNotSelected().performClick()
        assertEquals("tv", chosen)
        // A name this app does not know is still offered, drawn and named as a folder.
        compose.onNodeWithTag("icon something-newer").assert(SemanticsMatcher.expectValue(SemanticsProperties.ContentDescription, listOf("Folder")))
        assertTrue(compose.onNodeWithTag("icon picker").fetchSemanticsNode().config.contains(SemanticsProperties.SelectableGroup))
        compose.onNodeWithTag("icon baby").performTouchInput { longClick() }
        compose.waitForIdle()
        compose.onNodeWithText("Baby").assertIsDisplayed()
    }
}
