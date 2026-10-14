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
  feature/                 signin, home, downloads, settings, player
```

The player (`feature/player/`) is its own activity, opened with `context.openPlayer(itemId, fromStart)`. `PlayerController` holds its rules apart from Android (the playback request from this phone's decoders, progress every 10 seconds and on pause, leaving and the end, the server's HLS session ended once it is no longer played, a forgotten session asked for again, the next episode); `ExoPlayerEngine` is Media3 ExoPlayer with HLS, every request through the signed-in session's OkHttp client so the token goes to the server only. Nothing it plays is cached on disk.

The session's token is sealed with AES-GCM under a non-exportable Android Keystore key and kept in `noBackupFilesDir`; the app allows no backup or device transfer. There is no HTTP logging and no HTTP cache. Signing out, a 401 on any request, or signing in as someone else ends the session: the token file and its key are deleted, the session's work is cancelled and the tabs start again empty.

## Tests

- JVM: `ApiClientTest` (every endpoint against MockWebServer: method, path, query, body, token, the error envelope), `PairingFlowTest` (the pairing state machine), `EventStreamTest`, `SessionRepositoryTest`, `SessionStoreTest`, `StashDataTest`, `ServerAddressTest`, `PlaybackCapabilitiesTest`, `SignInControllerTest`, `LibraryIconsTest` (the icon names match the server's list).
- Robolectric: `AppShellTest` (the toolbar's selection, tooltips, descriptions, page changes, slide direction, reduce motion, kept state, Back), `SignInScreensTest`.
- Player: `PlaybackRequestBuilderTest` (the playback request and its wire shape, quality and audio choices), `ProgressSaverTest`, `StreamSessionsTest`, `PlayerControllerTest` (open and resume, direct and HLS, progress every 10 s, pause, background, leaving and the end, quality and audio switches, late answers ended, a forgotten session asked for again, gone videos, retry, the safe-codec retry, the next episode), `PlayerFormatTest`, `ExoPlayerEngineTest` (Robolectric: the token and Range on the session's client, no token to another host, ExoPlayer's errors, HLS and file loads, the picture-in-picture shape), `PlayerScreenTest` (Robolectric: what each control says and does, the menu, the end, failures, hiding the chrome, picture-in-picture) and `PlayerLauncherTest`.
- Roborazzi: `ShellScreenshotTest`, `SignInScreenshotTest` and `PlayerScreenshotTest`, in `app/src/test/screenshots/` (light and dark; the player is always dark).
- `ServerIntegrationTest` runs the app's client against a real server: it builds a `scripts/dev-fixture.sh` stash, starts `popcornstash serve`, makes the owner with a software passkey and the setup code from the log (as tests/e2e does with Chrome's virtual authenticator), then pairs the app (the owner approves the code), signs in with a passkey from the app's `android:apk-key-hash:` origin (`STASH_ANDROID_ASSETLINKS` lists it), and reads the libraries, Home and live events with the app's token. It needs ffmpeg and the server binary, so it runs only when asked:

  ```sh
  cargo build -p popcornstash
  cd android && ./gradlew :app:testDebugUnitTest --tests '*ServerIntegrationTest*' -PpopcornstashServer=../target/debug/popcornstash
  ```

  `scripts/check.sh --e2e` runs it. `POPCORNSTASH_KEEP=1` keeps its temporary stash and server log. `PlayerServerIntegrationTest` uses the same server: the player gets the fixture's H.264 MP4 directly (a Range request answers 206) and its HEVC MKV as a conversion (the playlist and the first segment answer the app's token), the place where it paused is saved, and leaving ends the conversion (the playlist then answers 404). The command above (and `check.sh --e2e`) runs both.

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
