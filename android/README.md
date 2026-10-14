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
