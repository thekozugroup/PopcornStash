# PopcornStash for Android

The family's libraries, downloads and settings on an Android phone: Kotlin, Jetpack Compose and Material 3 themed with the Popcorn tokens. The design system is [DESIGN.md](DESIGN.md); the API it uses and how it signs in are in [docs/architecture.md](../docs/architecture.md) (section 10, Android app).

- Package `com.thekozugroup.popcornstash`, minSdk 28 (Credential Manager passkeys), compile and target SDK 37.
- Three tabs in a floating toolbar: Downloads (left), Home (centre, the start tab), Settings (right).

## Building

Needs JDK 17 or later and the Android SDK (platform 37), named by `ANDROID_HOME` or `sdk.dir` in `android/local.properties` (git-ignored).

```sh
cd android
./gradlew assembleDebug            # app/build/outputs/apk/debug/app-debug.apk
./gradlew test                     # JVM, Robolectric UI and screenshot tests
./gradlew lint                     # Android lint, warnings are errors
./gradlew verifyRoborazziDebug     # compares the screens with app/src/test/screenshots
./gradlew recordRoborazziDebug     # records them again after a design change: look at each
./gradlew checkLucideGlyphs        # the icons still match the web's lucide-react
```

`scripts/check.sh` runs `./gradlew test lint assembleDebug verifyRoborazziDebug checkLucideGlyphs` when it finds an SDK and skips the app with a note otherwise. There is no emulator on the build machine: UI tests run on Robolectric and screenshots are drawn by Robolectric's native graphics (Roborazzi).

Debug builds accept `http://` for `localhost`, `127.0.0.1` and `10.0.2.2` (the emulator's view of the computer), so a development server (`popcornstash serve` with `STASH_PUBLIC_URL=http://localhost:8788`) works; release builds speak https only.

## How it is built

```
app/src/main/kotlin/com/thekozugroup/popcornstash/
  MainActivity.kt          sign-in while nobody is signed in, the three tabs once someone is
  AppContainer.kt          the process's services: the HTTP client, the session, sign-in, and per
                           session the pages' view model stores and the image loader (SessionUi)
  navigation/              Tab, AppShell (pages, toolbar, Back), PageTransitions, PageStack (a tab's
                           own pages), ToolbarRequests (the viewer and player hide the toolbar)
  ui/theme/                the Popcorn tokens: Color, Type, Shape, Tokens, Motion, Theme
  ui/icons/                LucideGlyphs (generated), PopcornIcons, LibraryIcons
  ui/components/           FloatingNavToolbar, PageTopBar, TabRootScaffold, rows, buttons, EmptyState,
                           MediaTile, Shelf, PageStatus (load failed, not connected), IconPicker
  ui/media/                StashImages: Coil over the session's client, Pictures (thumbnail and view addresses)
  data/api/                ApiClient (every endpoint the app uses), ApiModels, ApiError, EventStream (SSE)
  data/auth/               ServerAddress, PairingFlow, PasskeySignIn, Credential Manager, the sealed token store
  data/session/            SessionRepository: who is signed in; ending a session forgets everything
  data/repo/               StashData: Home, libraries, downloads, Inbox and the VPN line, kept current by events
  data/device/             PlaybackCapabilities: this phone's decoders for the playback decision
  feature/                 signin, downloads, settings, player, and Home with what it opens:
    home/                  HomeTab (the tab's page stack), Home, continue watching
    library/               library pages by layout, shows, Other files, MediaFormat (the web's words)
    item/                  an item's page and its favourite
    photos/                the full-screen photo viewer (zoom, save the original)
  share/                   ShareActivity (the share sheet), the upload job (UploadWorker, UploadRunner, Uploader), UploadQueue
```

Home opens pages on top of itself in the tab's own stack (`PageStack`): a library (grid, shows or the photo timeline, by its `layout`), a show, an item, Other files. Each page has a view model in a store that outlives a tab switch and a rotation and is cleared when the page closes or the session ends. Live `library` and `libraries` events and a reconnected events stream load what a page shows again; a library the person stops seeing closes with its pages. Playing opens the player (`openPlayer`), from Continue watching, a show, an item page or a phone clip in the photo viewer.

One tab can ask for a page of another (`TabRequests`, wired in `MainActivity`): the shell selects that tab and the tab opens the page once. Owners' "Edit library" and "Make a library" in Home open the library editor in Settings; a finished download's "In Kids" or "In Other files" opens that page in Home, on top of what Home showed. Settings and Home keep their pages' view models in the same per-session store (`AppContainer.sessionUi`), and the Downloads thumbnails come through Home's image loader. The sign-out confirmation says how many share sheet uploads it stops.

Pictures load through Coil with the session's client, so the bearer token is added for the server and nowhere else; the memory cache and the disk cache (`cacheDir/images`) go with the session. An Other file opens in another app from a private copy in `cacheDir/opened` through a FileProvider with a read-only grant; the copy is deleted when the person comes back, at the next start and on sign-out. "Save the original" in the photo viewer uses the system's "Save to" picker and streams the file into it with the app's own client.

The player (`feature/player/`) is its own activity, opened with `context.openPlayer(itemId, fromStart)`. `PlayerController` holds its rules apart from Android (the playback request from this phone's decoders, progress every 10 seconds and on pause, leaving and the end, the server's HLS session ended once it is no longer played, a forgotten session asked for again, the next episode); `ExoPlayerEngine` is Media3 ExoPlayer with HLS, every request through the signed-in session's OkHttp client so the token goes to the server only. Nothing it plays is cached on disk.

**Downloads** (`feature/downloads/`): `DownloadsController` builds the tab from `StashData` (downloads, the Inbox, libraries, the VPN status, all kept current by events), the person's role, the events connection and the share sheet's uploads, and sends every action: pause, resume, remove, try again, add a link, accept (always the place and library the row shows), ignore, edit and accept all. The words come from the web app: `DownloadsText.kt` (facts, where a finished download went), `VpnText.kt` (the VPN line), `LinkDetect.kt` (what a pasted or shared link is), `ImportTargets.kt` (which libraries fit a file, why it waits, the edit form and its live preview). The controller lives per session outside composition (`DownloadsHolders`), so the tab keeps its state while the person switches tabs and nothing of it outlives the session.

**Share sheet and uploads** (`share/`): `ShareActivity` takes `ACTION_SEND` and `ACTION_SEND_MULTIPLE`. Text is searched for its first http(s) or magnet link, which goes to Downloads after a confirm. Files are taken only as `content://` URIs from another app's provider (never `file://`, never one of this app's own providers, so a share can never make the app upload its own files). After the signed-in person confirms, the batch (who confirmed it, the server, each file's name, size, type and last change) is kept in `noBackupFilesDir/uploads/`, the app keeps its own read grant for each file, and a WorkManager job (`UploadWorker`, a `dataSync` foreground service with a progress notification) uploads the files one by one through the resumable upload API, keeping each file's upload id so a job that is stopped and started again continues where the server is. The job runs only while the person who confirmed it is signed in on the same server. Signing out stops every job, deletes what the server holds of them (`DELETE /api/uploads/:id`, before the logout) and forgets the batches, their grants and notifications; a 401 or someone else signing in does the same without the server step.

Settings (`feature/settings/`) is a stack of two pages: the first page (`SettingsScreen.kt`: the person, this server, the owner's libraries and the folders that are not a library, the web settings pages, sign out) and the library editor pushed on top of it (`LibraryEditorScreen.kt`). `LibrarySettingsController` keeps the owner's list (order, delete, people, folders) and `LibraryEditorController` one editor's form; both talk only to `ApiClient` and the session's `StashData`, whose `libraries` events keep them current. `LibraryWords.kt` holds the owner's words for libraries, the same as the web's.

The session's token is sealed with AES-GCM under a non-exportable Android Keystore key and kept in `noBackupFilesDir`; the app allows no backup or device transfer. There is no HTTP logging and no HTTP cache. Signing out, a 401 on any request, or signing in as someone else ends the session: the token file and its key are deleted, the session's work is cancelled and the tabs start again empty.

## Tests

- JVM: `ApiClientTest` (every endpoint against MockWebServer: method, path, query, body, token, the error envelope), `PairingFlowTest` (the pairing state machine), `EventStreamTest`, `SessionRepositoryTest`, `SessionStoreTest`, `StashDataTest`, `ServerAddressTest`, `PlaybackCapabilitiesTest`, `SignInControllerTest`, `LibraryIconsTest` (the icon names match the server's list, and their plain names the web's).
- Home and libraries (JVM, MockWebServer): `LibraryViewModelTest` (each layout's request, sort, paging, a month split across pages, events about the library, a burst of events as one reload, gone, reconnects), `ShowViewModelTest` (what plays next, seasons, gone), `ItemViewModelTest` (next episode, the favourite and its rollback, gone), `OtherFilesViewModelTest` (the list, sort, a private copy with the session client), `HomeViewModelTest` (shelf tiles: a tile for every show of a series library from `shows`, and the older grouping of `recent`; continue watching words), `SaveOriginalViewModelTest`, `MediaFormatTest` (the web's words; dates, times and months that never break inside), `CaptureTimeTest` (Robolectric with the phone's default zone set from Los Angeles to Kiritimati: a capture time reads as the camera's clock, a photo month as the server's, a waiting photo's folder month from its clock, an older server's `taken_at` in UTC, and an added date as a moment in the phone's zone), `LiveUpdatesTest` (reconnects and the "not connected" line), `PageStackTest` (the stack, its saver, the view model stores).
- Player: `PlaybackRequestBuilderTest` (the playback request and its wire shape, quality and audio choices), `ProgressSaverTest`, `StreamSessionsTest`, `PlayerControllerTest` (open and resume, direct and HLS, progress every 10 s, pause, background, leaving and the end, quality and audio switches, late answers ended, a forgotten session asked for again, gone videos, retry, the safe-codec retry, the next episode), `PlayerFormatTest`, `ExoPlayerEngineTest` (Robolectric: the token and Range on the session's client, no token to another host, ExoPlayer's errors, HLS and file loads, the picture-in-picture shape), `PlayerScreenTest` (Robolectric: what each control says and does, the menu, the end, failures, hiding the chrome, picture-in-picture) and `PlayerLauncherTest`.
- Downloads: `DownloadsControllerTest` (against a MockWebServer that answers like the server: what each action sends, accept with the row's library, accept all in order with its count of failures, the edit form's checks and the server's field errors, remove deleting files only while in progress, retry, the clipboard link, a member's single library, a lost connection reloading), `DownloadsTextTest`, `VpnTextTest`, `LinkDetectTest`, `ImportTargetsTest`; Robolectric `DownloadsScreenTest` (every control and its screen reader name).
- Share and uploads: `UploaderTest` (chunks at the server's size, `last_modified`, resuming from the server's count, 409 and retries, a refused file, stopping), `UploadRunnerTest` (nobody signed in sends nothing, done files skipped after a restart, a lost read grant, a batch removed while running), `UploadBatchStoreTest`, `ShareControllerTest`, `ShareRequestTest` (links in text, content URIs only, never the app's own, a real provider read through `ContentChunkSource`), `ShareSheetTest`.
- JVM, Settings: `LibraryWordsTest` (the owner's words), `LibrarySettingsControllerTest` (list, people, folders, a move shown at once and put back when it fails, delete, a library made elsewhere), `LibraryEditorControllerTest` (new, from a folder, type and claims rules, the lock on a library that holds files, the confirmation before taking a kind, validation, the server's field errors, edits that send only what changed, delete, a library gone while open), against a MockWebServer that answers by route (`Routes`).
- Between tabs: `TabRequestsTest` (Robolectric: a page asked for from another tab selects its tab and is handed over once; a tab asked for alone), `HomeTabTest` and `SettingsTabTest` (the page or editor another tab asks for, a member asked for an editor, the upload count in the sign-out confirmation).
- Robolectric: `AppShellTest` (the toolbar's selection, tooltips, descriptions, page changes, slide direction, the selection square caught mid-slide and jumping under reduce motion, a right-to-left layout mirroring the bar and the slides, the toolbar leaving while a page asks, kept state, Back), `PageStackHostTest` (a pushed page comes in from the end side), `PhotoViewerTest` (the viewer's controls on the player's disc and 16dp edge; the date on the plate is the camera's clock on a phone set to UTC+14), `SignInScreensTest`, `HomeTabTest` (Home against a server: opening a library and Back, tapping Home again, a tab keeping its pages, a show playing what comes next, continue watching and "Start from the beginning", the photo viewer hiding the toolbar, a library that goes away, "Edit library"), `SettingsTabTest` (the owner's list in order, the editor and Back, making a library end to end, a refused name under its field, move from the menu, TalkBack actions, delete with its confirmation, a folder made a library, tapping Settings again, members, the icon picker's semantics and tooltip).
- Roborazzi: `ShellScreenshotTest`, `SignInScreenshotTest`, `HomeScreenshotTest`, `LibraryScreenshotTest` (Home, each library layout, a show, an item, the photo viewer and Other files, with their loading, empty and failed states), `PlayerScreenshotTest`, `DownloadsScreenshotTest`, `ShareScreenshotTest`, `SettingsScreenshotTest` (Settings and the library editor loading, failed, empty and with content, menus and dialogs; the whole form on a tall phone) and `LargeTextScreenshotTest` (a 360dp phone at the largest font: a long server address, a folder named in a sentence, a header with its action, a photo month with its count, a link), light and dark (the player is always dark; sheets and dialogs with `captureScreenRoboImage`), in `app/src/test/screenshots/`. The pictures in them are painted by the test (`testing/SampleImages.kt`) and served by Coil's fake engine; the titles and people are invented.
- `ServerIntegrationTest` runs the app's client against a real server: it builds a `scripts/dev-fixture.sh` stash, starts `popcornstash serve`, makes the owner with a software passkey and the setup code from the log (as tests/e2e does with Chrome's virtual authenticator), then pairs the app (the owner approves the code), signs in with a passkey from the app's `android:apk-key-hash:` origin (`STASH_ANDROID_ASSETLINKS` lists it), and reads the libraries, Home and live events with the app's token; then the Home view models read the real stash (Movies as posters, Shows with what plays next, the photo timeline, an item's favourite saved and put back), a thumbnail comes with the bearer client and an Other file is fetched into a private copy. It needs ffmpeg and the server binary, so it runs only when asked:

  ```sh
  cargo build -p popcornstash
  cd android && ./gradlew :app:testDebugUnitTest --tests '*ServerIntegrationTest*' -PpopcornstashServer=../target/debug/popcornstash
  ```

  `scripts/check.sh --e2e` runs it (with `--rerun`). `POPCORNSTASH_KEEP=1` keeps its temporary stash and server log. The test task declares what it reads outside `android/` as inputs (the server's and the web's icon lists, `scripts/dev-fixture.sh`, the server binary), and a run with `-PpopcornstashServer` is never taken from Gradle's build cache or skipped as up to date, so a changed server is always tested. `PlayerServerIntegrationTest` uses the same server: the player gets the fixture's H.264 MP4 directly (a Range request answers 206) and its HEVC MKV as a conversion (the playlist and the first segment answer the app's token), the place where it paused is saved, and leaving ends the conversion (the playlist then answers 404). The command above (and `check.sh --e2e`) runs both.

  `DownloadsServerIntegrationTest` starts its own server on a fresh fixture stash for the Downloads tab: the VPN line as the server reports it, a fixture clip accepted into Movies through the library picker, and a share sheet upload split over two runs that resumes, is accepted and shows "In Other files".

  `SettingsServerIntegrationTest` does the same for library management, through the controllers Settings uses and an owner's `full` token: it makes a library from the fixture's `Family 2019/` folder (taking camera videos from Home Videos after the confirmation), renames it and shares it with everyone, moves it, gets a refused name under the name field, makes one with a new folder, and deletes both, with every file still on the disk. The same `--tests '*ServerIntegrationTest*'` pattern runs both, and `scripts/check.sh --e2e` runs them. `POPCORNSTASH_KEEP=1` keeps their temporary stash and server log.

## Icons

The app draws the web app's Lucide glyphs, from the same `lucide-react` release (`web/package.json`). `./gradlew :app:generateLucideGlyphs` writes `app/src/main/kotlin/.../ui/icons/LucideGlyphs.kt` from `web/node_modules/lucide-react` (run `npm --prefix web ci` first); the names it takes are `lucideIconNames` in `app/build.gradle.kts`, the library icon list (docs/architecture.md section 8) followed by the fixed glyphs.

## Release signing

Release builds are signed only from the environment; no keystore or password is ever kept in the repository (`*.jks` and `*.keystore` are git-ignored):

| Variable | |
| --- | --- |
| `POPCORNSTASH_KEYSTORE` | path to the `.jks` or `.p12` file |
| `POPCORNSTASH_KEYSTORE_PASSWORD` | keystore password |
| `POPCORNSTASH_KEY_ALIAS` | key alias |
| `POPCORNSTASH_KEY_PASSWORD` | key password (defaults to the keystore password) |

Without them `./gradlew assembleRelease` builds an unsigned APK.
