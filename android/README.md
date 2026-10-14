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
                           MediaTile, Shelf, PageStatus (load failed, not connected)
  ui/media/                StashImages: Coil over the session's client, Pictures (thumbnail and view addresses)
  data/api/                ApiClient (every endpoint the app uses), ApiModels, ApiError, EventStream (SSE)
  data/auth/               ServerAddress, PairingFlow, PasskeySignIn, Credential Manager, the sealed token store
  data/session/            SessionRepository: who is signed in; ending a session forgets everything
  data/repo/               StashData: Home, libraries, downloads, Inbox and the VPN line, kept current by events
  data/device/             PlaybackCapabilities: this phone's decoders for the playback decision
  feature/                 signin, downloads, settings, and Home with what it opens:
    home/                  HomeTab (the tab's page stack), Home, continue watching
    library/               library pages by layout, shows, Other files, MediaFormat (the web's words)
    item/                  an item's page and its favourite
    photos/                the full-screen photo viewer (zoom, save the original)
```

Home opens pages on top of itself in the tab's own stack (`PageStack`): a library (grid, shows or the photo timeline, by its `layout`), a show, an item, Other files. Each page has a view model in a store that outlives a tab switch and a rotation and is cleared when the page closes or the session ends. Live `library` and `libraries` events and a reconnected events stream load what a page shows again; a library the person stops seeing closes with its pages. Playing hands the item to the player through `HomeTab(onPlay = ...)`; until the player is in the app, `MainActivity` opens the item on the web. Owners' "Edit library" and "Make a library" open the web library settings for now.

Pictures load through Coil with the session's client, so the bearer token is added for the server and nowhere else; the memory cache and the disk cache (`cacheDir/images`) go with the session. An Other file opens in another app from a private copy in `cacheDir/opened` through a FileProvider with a read-only grant; the copy is deleted when the person comes back, at the next start and on sign-out. "Save the original" in the photo viewer uses the system's "Save to" picker and streams the file into it with the app's own client.

The session's token is sealed with AES-GCM under a non-exportable Android Keystore key and kept in `noBackupFilesDir`; the app allows no backup or device transfer. There is no HTTP logging and no HTTP cache. Signing out, a 401 on any request, or signing in as someone else ends the session: the token file and its key are deleted, the session's work is cancelled and the tabs start again empty.

## Tests

- JVM: `ApiClientTest` (every endpoint against MockWebServer: method, path, query, body, token, the error envelope), `PairingFlowTest` (the pairing state machine), `EventStreamTest`, `SessionRepositoryTest`, `SessionStoreTest`, `StashDataTest`, `ServerAddressTest`, `PlaybackCapabilitiesTest`, `SignInControllerTest`, `LibraryIconsTest` (the icon names match the server's list).
- Home and libraries (JVM, MockWebServer): `LibraryViewModelTest` (each layout's request, sort, paging, a month split across pages, events about the library, a burst of events as one reload, gone, reconnects), `ShowViewModelTest` (what plays next, seasons, gone), `ItemViewModelTest` (next episode, the favourite and its rollback, gone), `OtherFilesViewModelTest` (the list, sort, a private copy with the session client), `HomeViewModelTest` (shelf tiles, continue watching words), `SaveOriginalViewModelTest`, `MediaFormatTest`, `LiveUpdatesTest` (reconnects and the "not connected" line), `PageStackTest` (the stack, its saver, the view model stores).
- Robolectric: `AppShellTest` (the toolbar's selection, tooltips, descriptions, page changes, slide direction, reduce motion, kept state, Back), `SignInScreensTest`, `HomeTabTest` (Home against a server: opening a library and Back, tapping Home again, a tab keeping its pages, a show playing what comes next, continue watching and "Start from the beginning", the photo viewer hiding the toolbar, a library that goes away, "Edit library").
- Roborazzi: `ShellScreenshotTest`, `SignInScreenshotTest`, `HomeScreenshotTest` and `LibraryScreenshotTest` (Home, each library layout, a show, an item, the photo viewer and Other files, with their loading, empty and failed states), light and dark, in `app/src/test/screenshots/`. The pictures in them are painted by the test (`testing/SampleImages.kt`) and served by Coil's fake engine; the titles and people are invented.
- `ServerIntegrationTest` runs the app's client against a real server: it builds a `scripts/dev-fixture.sh` stash, starts `popcornstash serve`, makes the owner with a software passkey and the setup code from the log (as tests/e2e does with Chrome's virtual authenticator), then pairs the app (the owner approves the code), signs in with a passkey from the app's `android:apk-key-hash:` origin (`STASH_ANDROID_ASSETLINKS` lists it), and reads the libraries, Home and live events with the app's token; then the Home view models read the real stash (Movies as posters, Shows with what plays next, the photo timeline, an item's favourite saved and put back), a thumbnail comes with the bearer client and an Other file is fetched into a private copy. It needs ffmpeg and the server binary, so it runs only when asked:

  ```sh
  cargo build -p popcornstash
  cd android && ./gradlew :app:testDebugUnitTest --tests '*ServerIntegrationTest*' -PpopcornstashServer=../target/debug/popcornstash
  ```

  `scripts/check.sh --e2e` runs it. `POPCORNSTASH_KEEP=1` keeps its temporary stash and server log.

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
