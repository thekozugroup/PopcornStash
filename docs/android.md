# The Android app

PopcornStash for Android shows the family's libraries, the downloads and the owner's library settings on a phone. It talks only to your own server, with the same API and the same rules as the web app: a person sees only the libraries the owner shares with them.

It is new. It has been tested on the build machine (unit tests, Compose UI tests on Robolectric, screenshots and runs against a real server), but not yet on a phone or an emulator. Read [What is not done yet](#what-is-not-done-yet) before you hand it to the family.

## What it does

Three tabs sit in a floating bar at the bottom of the screen: Downloads on the left, Home in the centre (where the app opens) and Settings on the right. The bar shows icons only; hold one to see its name. The selected tab's square is blue with a white icon, and the pages slide in from the side of the tab you tap (they fade instead when the phone's "Remove animations" setting is on). On Android 12 and later the bar is frosted glass: the pages scroll under it blurred and tinted. On Android 9 to 11, and on any phone whose colour contrast setting is medium or high, it is a plain white bar (dark grey in dark mode).

- **Sign in:** type your server's address, then sign in with your passkey or pair the phone with a code that someone approves on the web.
- **Home:** what you were watching, to carry on with one tap, then every library you can see with its icon, its name and its newest items.
- **A library:** a grid of posters or video frames for films and clips, series then seasons and episodes for shows, and the month timeline for photos with a full-screen viewer (pinch to zoom, save the original to the phone).
- **An item:** its picture, details and favourite, and Play or Resume.
- **The player:** full screen, resumes where you stopped, saves your place as you watch, offers the next episode, and keeps playing in a small window (picture-in-picture) when you leave the app. It asks the server for a version this phone can decode.
- **Downloads:** what is downloading, with live progress, the VPN line in the web app's words, pause, resume, remove and "Add a link" for a web link or a magnet; the files waiting to be imported, each with the server's suggestion, a library picker that lists only what fits, Edit (title, show, season, episode), Accept, Ignore and "Accept all"; finished downloads say which library they went into, and tapping that opens it.
- **The share sheet:** share a link from another app to add it to Downloads, or share photos, videos and other files to upload them (a notification shows the upload, which carries on in the background and can be stopped).
- **Settings:** who is signed in, this server, and sign out. Owners also manage the libraries here: make one (from a new folder or a folder already on the share), edit its name, icon, type, the kinds of new file it takes and who sees it, reorder and delete. VPN, people, devices, security and system open the web settings in the phone's browser for now. **This app** shows the app's version and build, checks for a newer build and installs it ([Updates](#updates)).

Everything the app keeps (the sign-in, pictures it has loaded, files it opened) is deleted when you sign out, and none of it goes into the phone's backups.

## Build the app

You need JDK 17 or later and the Android SDK with SDK Platform 37.2 (Android 17 QPR2). The app compiles against it for the toolbar's frosted glass on Android 17 QPR2 phones. Install it with:

```sh
sdkmanager "platforms;android-37.2"
```

Tell Gradle where the SDK is with `ANDROID_HOME`, or with `sdk.dir=/path/to/sdk` in `android/local.properties` (that file is not committed).

```sh
cd android
./gradlew assembleDebug
```

The app is in `android/app/build/outputs/apk/debug/app-debug.apk`. A debug build is signed with your computer's debug key (`~/.android/debug.keystore`, made by the Android tools the first time) and also accepts `http://localhost`, `http://127.0.0.1` and `http://10.0.2.2` for a development server; everything else must be https.

A release build (`./gradlew assembleRelease`) is signed only when the signing variables are set ([android/README.md](../android/README.md#release-signing)), and speaks https only.

## Install a debug build

With the phone connected over USB and USB debugging turned on (Settings, About phone, tap Build number seven times, then Developer options, USB debugging):

```sh
adb install -r android/app/build/outputs/apk/debug/app-debug.apk
```

Or copy the APK to the phone and open it in the Files app; Android asks you to allow installing apps from that source once. A debug build and a release build of PopcornStash cannot be installed side by side: they have the same package name and different signing keys, so remove one before installing the other.

## Sign in

Open the app and type the address you open PopcornStash at in a browser (for example `stash.example.com`). The app checks that a PopcornStash server answers there. If the server has no owner yet, finish the setup in a browser first.

### Pair with a code

This always works.

1. Choose **Pair with a code**. The app shows a code such as `KQ7M-2XWD`.
2. Choose **Open PopcornStash to approve**: the pairing page opens in the phone's browser with the code filled in. Sign in there if you are asked, check that the codes match and choose **Approve**. You can also approve from a computer: open `https://<your server>/pair`, type the code under **Code on the device** and choose **Find device**, or find the request in **Settings, Devices**.
3. The app signs in by itself within a few seconds. A code lasts ten minutes.

The phone then appears in **Settings, Devices** under its model name; removing it there signs the app out.

### Sign in with a passkey

The app can use the passkey you already sign in with on the web, once the server tells Android that the app may. Until then the app offers pairing only and says "Passkeys work in the app once the server lists it."

1. Find the SHA-256 fingerprint of the certificate your copy of the app is signed with. For a debug build from your own computer:

   ```sh
   keytool -list -v -keystore ~/.android/debug.keystore -alias androiddebugkey -storepass android | grep 'SHA256:'
   ```

   For any APK (a release build, or one someone else built), with the SDK's build tools:

   ```sh
   $ANDROID_HOME/build-tools/36.0.0/apksigner verify --print-certs app-debug.apk | grep SHA-256
   ```

   `cd android && ./gradlew :app:signingReport` prints it for every build type too. For a release key, run the `keytool` line on your `.jks` file with your alias and password. If the app is ever installed from Google Play with Play App Signing, use the "App signing key certificate" fingerprint from the Play Console instead. The fingerprint is 32 bytes in hex; colons between the bytes are fine, and so is no separator at all.

2. Put it in `STASH_ANDROID_ASSETLINKS`, exactly as below with your fingerprint (list both the debug and the release fingerprint if you use both builds). In `.env`, on one line, in single quotes:

   ```sh
   STASH_ANDROID_ASSETLINKS='[{"relation":["delegate_permission/common.get_login_creds"],"target":{"namespace":"android_app","package_name":"com.thekozugroup.popcornstash","sha256_cert_fingerprints":["14:6D:E9:83:C5:73:06:50:D8:EE:B9:95:2F:34:FC:64:16:A0:83:42:E6:1D:BE:A8:8A:04:96:B2:3F:CF:44:E5"]}}]'
   ```

   `compose.yaml` passes it to the container. On Unraid, add a variable named `STASH_ANDROID_ASSETLINKS` to the container with the same value (without the single quotes). Then recreate the container (`docker compose up -d`); a restart keeps the old environment.

3. Check that the server serves it: `https://<your server>/.well-known/assetlinks.json` must show the same statement. The server now also accepts the app's passkey origin (`android:apk-key-hash:` of that certificate) for sign-in; nothing else changes for the web.

4. In the app, go back to the address step and continue: it now offers **Sign in with a passkey**. Android shows the passkeys saved for your server (in Google Password Manager or another passkey provider on the phone); pick yours.

Android checks this file through Google's Digital Asset Links service, so passkeys in the app need an address that is reachable from the internet over https with a publicly trusted certificate (a Cloudflare Tunnel or a reverse proxy with Let's Encrypt works; an address only your home network or Tailscale can reach may not). Pairing works everywhere.

## Updates

The app updates itself from the project's GitHub releases. Each push to main that changes the app is built, signed with the project's release key and published as a release named "Android app, build N" (tag `android-N`), where N is the build number: it only ever grows. The server is not updated this way; you update the container yourself ([docs/deployment.md](deployment.md)).

**Settings, This app** shows the version and build ("0.1.0, build 412") and where updates stand:

- **Check for updates** asks GitHub now. "Up to date, checked 2 minutes ago." or "Build 415 is available."
- **Update** downloads that build (a notification shows the progress), checks it, then hands it to Android. The first time, Android asks you to let PopcornStash install apps (Settings, Apps, Special app access, Install unknown apps); allow it and come back, and the update carries on. Android then shows its own confirmation. On Android 12 and later, once a build from GitHub has been installed by the app itself, later updates can install without that confirmation. The app restarts on the new build and you stay signed in.
- **Check for updates automatically** (on by default): the app asks GitHub once a day, on any network, and posts one notification when it finds a newer build ("PopcornStash build 415 is ready to install"); tapping it opens Settings. Turn the switch off and the app never asks GitHub by itself.

Before anything is installed, the download must match the SHA-256 published next to it, be this app at the build it claims, and be signed with the same key as the app on the phone. Android itself refuses an update signed with another key. A file that fails a check is deleted at once, and so is the download once it is installed or the install fails.

**What the app sends to GitHub:** a request for the repository's public release list (`api.github.com/repos/thekozugroup/PopcornStash/releases`) and, when you update, the download of the APK and its checksum. The requests carry the app's name and version (`PopcornStash-Android/0.1.0+412`) and nothing else: no account, no token, nothing about your server, your phone or what is in your libraries. GitHub sees your network's address, as for any web request. The app keeps the last answer's ETag, so an unchanged list costs GitHub nothing; GitHub allows 60 such checks an hour from one network address, and the app says when that limit is reached.

### Switching to the builds on GitHub

A copy you built yourself (any debug build, or a release signed with your own key) is signed with a different key from the builds on GitHub, so it cannot update itself: Settings says "Build 415 is available, but this copy cannot update itself." and offers the release page. Switch once:

1. Uninstall PopcornStash from the phone (this signs it out and removes what it kept).
2. Open the release page from the app's Settings, or [github.com/thekozugroup/PopcornStash/releases](https://github.com/thekozugroup/PopcornStash/releases), download `popcornstash-android-N.apk` from the newest "Android app, build N" release and open it; allow installing from your browser when Android asks.
3. Open the app and sign in again: pair with a code, or use your passkey. For passkeys, `STASH_ANDROID_ASSETLINKS` must list the release key's fingerprint (the value in `android/release-cert.sha256`) instead of, or as well as, your debug key's ([Sign in with a passkey](#sign-in-with-a-passkey)).

From then on the app updates itself.

## What is not done yet

- **Not run on a phone or an emulator.** The build machine has no emulator, so gestures, the photo viewer's zoom, real video decoding, picture-in-picture, rotation, the share sheet's background uploads and the passkey prompt have only run under Robolectric and against a real server from the JVM.
- **The frosted toolbar on Android 17 QPR2 has not run.** From Android 17 QPR2 (SDK 37.2) the platform blurs the window behind the bar itself (`RenderNode.setBackdropRenderEffect`). That code compiles and passes lint, but there was no 37.2 phone or emulator image to run it on, and Robolectric runs Android 16, so it has never drawn. The same goes for how the bar fades in and out there: without a layer of its own (`CompositingStrategy.ModulateAlpha`), because the platform would otherwise blur that empty layer instead of the pages. The glass on Android 12 to 17.1 (the app records the pages and blurs them; most phones show this until Android 17 QPR2 reaches Pixels in December 2026) is covered by screenshots and pixel checks, and the opaque bar on Android 9 to 11 by a screenshot.
- **No lock screen or headset controls** (no media session), **no subtitles**, and no double tap to seek in the player.
- **A shared `.torrent` file is uploaded as a file**, not added as a download.
- **Admin pages on the web:** VPN, people, devices, security, system, sorting and the trash open in the phone's browser, which has its own sign-in.
- **Imports for members:** the API does not say who added a waiting file, so a member's row whose suggestion names a library offers only that library, even for a file the member added (the web does the same).
- **Not in a store.** Builds come from GitHub releases ([Updates](#updates)); a copy built on your own computer is signed with your key and cannot update itself from them.
- **The updater has not run on a phone.** The check, the download, the checksum and signature checks and the install session are tested on the build machine (Robolectric, with a real signed APK from a mock GitHub), but Android's own install, its confirmation and the update without a confirmation on Android 12 and later have not been tried on a device. Robolectric cannot read an APK's certificates through PackageManager, so the tests read them with apksig instead; on a phone the app reads them through PackageManager.
