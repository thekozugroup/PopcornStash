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
  AppContainer.kt          the process's services: the HTTP client, the session, sign-in
  navigation/              Tab, AppShell (pages, toolbar, Back), PageTransitions
  ui/theme/                the Popcorn tokens: Color, Type, Shape, Tokens, Motion, Theme
  ui/icons/                LucideGlyphs (generated), PopcornIcons, LibraryIcons
  ui/components/           FloatingNavToolbar, PageTopBar, TabRootScaffold, rows, buttons, EmptyState
  data/api/                ApiClient (every endpoint the app uses), ApiModels, ApiError, EventStream (SSE)
  data/auth/               ServerAddress, PairingFlow, PasskeySignIn, Credential Manager, the sealed token store
  data/session/            SessionRepository: who is signed in; ending a session forgets everything
  data/repo/               StashData: Home, libraries, downloads, Inbox and the VPN line, kept current by events
  data/device/             PlaybackCapabilities: this phone's decoders for the playback decision
  feature/                 signin, home, downloads, settings
  share/                   ShareActivity (the share sheet), the upload job (UploadWorker, UploadRunner, Uploader), UploadQueue
```

**Downloads** (`feature/downloads/`): `DownloadsController` builds the tab from `StashData` (downloads, the Inbox, libraries, the VPN status, all kept current by events), the person's role, the events connection and the share sheet's uploads, and sends every action: pause, resume, remove, try again, add a link, accept (always the place and library the row shows), ignore, edit and accept all. The words come from the web app: `DownloadsText.kt` (facts, where a finished download went), `VpnText.kt` (the VPN line), `LinkDetect.kt` (what a pasted or shared link is), `ImportTargets.kt` (which libraries fit a file, why it waits, the edit form and its live preview). The controller lives per session outside composition (`DownloadsHolders`), so the tab keeps its state while the person switches tabs and nothing of it outlives the session.

**Share sheet and uploads** (`share/`): `ShareActivity` takes `ACTION_SEND` and `ACTION_SEND_MULTIPLE`. Text is searched for its first http(s) or magnet link, which goes to Downloads after a confirm. Files are taken only as `content://` URIs from another app's provider (never `file://`, never one of this app's own providers, so a share can never make the app upload its own files). After the signed-in person confirms, the batch (who confirmed it, the server, each file's name, size, type and last change) is kept in `noBackupFilesDir/uploads/`, the app keeps its own read grant for each file, and a WorkManager job (`UploadWorker`, a `dataSync` foreground service with a progress notification) uploads the files one by one through the resumable upload API, keeping each file's upload id so a job that is stopped and started again continues where the server is. The job runs only while the person who confirmed it is signed in on the same server. Signing out stops every job, deletes what the server holds of them (`DELETE /api/uploads/:id`, before the logout) and forgets the batches, their grants and notifications; a 401 or someone else signing in does the same without the server step.

The session's token is sealed with AES-GCM under a non-exportable Android Keystore key and kept in `noBackupFilesDir`; the app allows no backup or device transfer. There is no HTTP logging and no HTTP cache. Signing out, a 401 on any request, or signing in as someone else ends the session: the token file and its key are deleted, the session's work is cancelled and the tabs start again empty.

## Tests

- JVM: `ApiClientTest` (every endpoint against MockWebServer: method, path, query, body, token, the error envelope), `PairingFlowTest` (the pairing state machine), `EventStreamTest`, `SessionRepositoryTest`, `SessionStoreTest`, `StashDataTest`, `ServerAddressTest`, `PlaybackCapabilitiesTest`, `SignInControllerTest`, `LibraryIconsTest` (the icon names match the server's list).
- Downloads: `DownloadsControllerTest` (against a MockWebServer that answers like the server: what each action sends, accept with the row's library, accept all in order with its count of failures, the edit form's checks and the server's field errors, remove deleting files only while in progress, retry, the clipboard link, a member's single library, a lost connection reloading), `DownloadsTextTest`, `VpnTextTest`, `LinkDetectTest`, `ImportTargetsTest`; Robolectric `DownloadsScreenTest` (every control and its screen reader name).
- Share and uploads: `UploaderTest` (chunks at the server's size, `last_modified`, resuming from the server's count, 409 and retries, a refused file, stopping), `UploadRunnerTest` (nobody signed in sends nothing, done files skipped after a restart, a lost read grant, a batch removed while running), `UploadBatchStoreTest`, `ShareControllerTest`, `ShareRequestTest` (links in text, content URIs only, never the app's own, a real provider read through `ContentChunkSource`), `ShareSheetTest`.
- Robolectric: `AppShellTest` (the toolbar's selection, tooltips, descriptions, page changes, slide direction, reduce motion, kept state, Back), `SignInScreensTest`.
- Roborazzi: `ShellScreenshotTest`, `SignInScreenshotTest`, `DownloadsScreenshotTest` and `ShareScreenshotTest`, light and dark, in `app/src/test/screenshots/` (sheets and dialogs with `captureScreenRoboImage`).
- `ServerIntegrationTest` runs the app's client against a real server: it builds a `scripts/dev-fixture.sh` stash, starts `popcornstash serve`, makes the owner with a software passkey and the setup code from the log (as tests/e2e does with Chrome's virtual authenticator), then pairs the app (the owner approves the code), signs in with a passkey from the app's `android:apk-key-hash:` origin (`STASH_ANDROID_ASSETLINKS` lists it), and reads the libraries, Home and live events with the app's token. It needs ffmpeg and the server binary, so it runs only when asked:

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
