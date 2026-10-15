# The Android app

PopcornStash for Android shows the family's libraries, the downloads and the owner's library settings on a phone. It talks only to your own server, with the same API and the same rules as the web app: a person sees only the libraries the owner shares with them.

It is new. It has been tested on the build machine (unit tests, Compose UI tests on Robolectric, screenshots and runs against a real server), with additional native layout checks on Android emulators. Physical phones have not been tested. Read [What is not done yet](#what-is-not-done-yet) before you hand it to the family.

## What it does

Three tabs sit in a floating bar: Downloads, Home (where the app opens), and Settings. On phones, including landscape and narrow split windows, the bar sits at the bottom. On tablets and unfolded windows at least 600dp wide and 480dp tall, it sits on the physical right edge with a separate content gutter. Targets are equally spaced. Hold an icon to see its name; the tooltip opens above the phone bar or to the left of the rail. The selected square is blue with a white icon. Pages move along the bar's axis, or fade when Remove animations is on. Content and messages keep clear of the controls. Android 12 and later uses the existing frosted treatment; Android 9–11 and higher contrast settings use an opaque surface.

Page and section headings use bundled Newsreader; controls and body text use the system sans font. The blue popcorn icon is flattened from Apple Icon Composer. Its gradient and glass treatment stay inside the icon.

On a foldable in tabletop mode (a horizontal, half-open hinge), playback shows the picture above the hinge and persistent media controls below. Both panes avoid the hinge bounds. Opening flat returns to the normal player; picture-in-picture keeps only the picture. The device must report its fold through Android's WindowManager API.

- **Sign in:** type your server's address, then sign in with your passkey or pair the phone with a code that someone approves on the web.
- **Home:** what you were watching, to carry on with one tap, then every library you can see with its icon, its name and its newest items.
- **A library:** a grid of posters or video frames for films and clips, series then seasons and episodes for shows, and the month timeline for photos with a full-screen viewer (pinch to zoom, save the original to the phone).
- **An item:** its picture, details and favourite, and Play or Resume.
- **The player:** full screen, resumes where you stopped, saves your place as you watch, offers the next episode, and keeps playing in a small window (picture-in-picture) when you leave the app. It asks the server for a version this phone can decode.
- **Downloads:** what is downloading, with live progress, the VPN line in the web app's words, pause, resume, remove and "Add a link" for a web link or a magnet; the files waiting to be imported, each with the server's suggestion, a library picker that lists only what fits, Edit (title, show, season, episode), Accept, Ignore and "Accept all"; finished downloads say which library they went into, and tapping that opens it.
- **The share sheet:** share a link from another app to add it to Downloads, or share photos, videos and other files to upload them (a notification shows the upload, which carries on in the background and can be stopped).
- **Settings:** who is signed in, this server, and sign out. Owners also manage the libraries here: make one (from a new folder or a folder already on the share), edit its name, icon, type, the kinds of new file it takes and who sees it, reorder and delete. Sorting and Trash (including owner-only restore), VPN, people, devices, security and system open the web settings in the phone's browser for now. **This app** shows the app's version and build, checks for a newer build and installs it ([Updates](#updates)).

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

The app can use the passkey you already sign in with on the web, once the server tells Android that the app may. The app checks the passkey host, both app-link permissions and its installed signing certificate before offering passkeys. When a check fails, it explains the problem and offers pairing.

1. Find the SHA-256 fingerprint of the certificate your copy of the app is signed with. For a debug build from your own computer:

   ```sh
   keytool -list -v -keystore ~/.android/debug.keystore -alias androiddebugkey -storepass android | grep 'SHA256:'
   ```

   For any APK (a release build, or one someone else built), with the SDK's build tools:

   ```sh
   $ANDROID_HOME/build-tools/36.0.0/apksigner verify --print-certs app-debug.apk | grep SHA-256
   ```

   `cd android && ./gradlew :app:signingReport` prints it for every build type too. For a release key, run the `keytool` line on your `.jks` file with your alias and password. If the app is ever installed from Google Play with Play App Signing, use the "App signing key certificate" fingerprint from the Play Console instead. Use 32 bytes in uppercase hex, with a colon between every byte (the format printed by `keytool`).

2. Put it in `STASH_ANDROID_ASSETLINKS`, exactly as below with your fingerprint (list both the debug and the release fingerprint if you use both builds). In `.env`, on one line, in single quotes:

   ```sh
   STASH_ANDROID_ASSETLINKS='[{"relation":["delegate_permission/common.handle_all_urls","delegate_permission/common.get_login_creds"],"target":{"namespace":"android_app","package_name":"com.thekozugroup.popcornstash","sha256_cert_fingerprints":["14:6D:E9:83:C5:73:06:50:D8:EE:B9:95:2F:34:FC:64:16:A0:83:42:E6:1D:BE:A8:8A:04:96:B2:3F:CF:44:E5"]}}]'
   ```

   `compose.yaml` passes it to the container. On Unraid, add a variable named `STASH_ANDROID_ASSETLINKS` to the container with the same value (without the single quotes). Then recreate the container (`docker compose up -d`); a restart keeps the old environment.

3. Check `https://<rp_id>/.well-known/assetlinks.json` on HTTPS port 443, even when the app itself uses another port. The server also accepts the listed certificate's `android:apk-key-hash:` WebAuthn origin; the web's checks stay the same. The app address's host must match `rp_id` from `/api/auth/state`.
4. Check Google's view, replacing the host and fingerprint (URL-encode the fingerprint):

   ```text
   https://digitalassetlinks.googleapis.com/v1/assetlinks:check?source.web.site=https://<rp_id>&relation=delegate_permission/common.handle_all_urls&target.android_app.package_name=com.thekozugroup.popcornstash&target.android_app.certificate.sha256_fingerprint=<FINGERPRINT>
   ```

   Repeat with `delegate_permission/common.get_login_creds`. Both must return `"linked": true`. Google caches fetch failures, so allow about ten minutes after fixing a route.

A private Tailscale address without a public route, home-only DNS, or a certificate trusted only inside your network cannot be verified by Google. Use **Pair with a code** for a private server. The app's local pre-check cannot prove Google's reachability. A provider verification failure explains this and directs the person to pairing. No automatic Google diagnostic request is sent by the app.

Two public alternatives require the owner's approval:

- Expose only the verification file on HTTPS port 443, with Tailscale Funnel or a public proxy, and keep the app private on another HTTPS port. The RP host stays the same, so its existing passkeys remain usable. Bookmarks and paired clients must use the new app port if it changes. A Funnel/file-only arrangement has not been validated here; review its routing before enabling it.
- Use an owned domain and public HTTPS route. A changed RP host requires a new passkey through `popcornstash admin reset-owner`; choose the domain before migrating clients.

Startup logs report malformed certificates, missing permissions, noncanonical fingerprint formatting and probably private RP hosts. Android logs under `PopcornStash.Passkey` contain exception and DOM class names only; provider messages, account data, assertions and challenges are excluded. The app distinguishes cancellation, no credential, verification failure, screen-lock refusal, interruption and an unavailable passkey service. Native sign-in starts and expired unfinished requests are logged by the server; native is inferred from missing Origin/Referer headers, and expiry is reported on the next flow sweep.

## Updates

The app updates itself from the project's GitHub releases. Each push to main that changes the app is built, signed with the project's release key and published as a release named "Android app, build N" (tag `android-N`), where N is the build number: it only ever grows. The server is not updated this way; you update the container yourself ([docs/deployment.md](deployment.md)).

**Settings, This app** shows the version and build ("Version 0.1.0, build 412") and where updates stand:

- **Check for updates** asks GitHub now: "Up to date, checked 2 minutes ago." or "Build 415 is available."
- **Update** downloads that build (a notification shows the progress), checks it, then hands it to Android. The first time, Android asks you to let PopcornStash install apps (Settings, Apps, Special app access, Install unknown apps); allow it and come back, and the update carries on. Android may then show its own confirmation; if you leave it, **Install** in Settings shows it again, and if you close it the download is kept for **Install** later. On Android 12 and later the app asks Android to skip that confirmation for its own update, which Android allows under its own rules (not yet tried on a phone). **PopcornStash closes while it updates: open it again afterwards.** You stay signed in.
- **Check for updates automatically** (on by default): the app asks GitHub once a day, on any network, and posts one notification when it finds a newer build it can install ("PopcornStash build 415 is ready to install"); tapping it opens Settings. On Android 13 and later the app asks for permission to post notifications when you turn the switch on, and the first time you check or update; while notifications are off the switch says so, and a new build shows only in Settings. Turn the switch off and the app never asks GitHub by itself.

Before anything is installed, the download must match the SHA-256 published next to it, be this app at the build it claims, and be signed with the same key as the app on the phone. Android itself refuses an update signed with another key. A file that fails a check is deleted at once. On a copy installed from GitHub, a download signed with any other key is refused with "Build 415 was not signed by PopcornStash.": do not install it by hand; tell the owner. The download is deleted once it is installed or the install fails; a download larger than the release list said (or than 200 MB) is stopped.

**What the app sends to GitHub:** once a day (unless turned off) and when you tap Check for updates, a request for the repository's public release list (`api.github.com/repos/thekozugroup/PopcornStash/releases`); when you tap Update, the download of the APK and its checksum. The requests carry the app's name and version (`PopcornStash-Android/0.1.0+412`) and nothing else: no account, no token, nothing about your server, your phone or what is in your libraries. GitHub sees your network's address, as for any web request. The app keeps the last answer's ETag, so an unchanged list comes back empty and saves the download; it still counts towards GitHub's limit of 60 such requests an hour from one network address, and the app says when that limit is reached.

### Switching to the builds on GitHub

**Owner, before anyone switches:** add the release key's fingerprint (the value in `android/release-cert.sha256`) to `STASH_ANDROID_ASSETLINKS`, next to the debug key's, and recreate the container ([Sign in with a passkey](#sign-in-with-a-passkey)). Until then the switched app signs in by pairing only.

A copy that was not installed from GitHub (any debug build, or a release signed with your own key) cannot update itself from the builds there: Settings says "Build 415 needs a one-time reinstall." and offers **Open the release page**, and it gets no daily notification. Switch once, in this order:

1. In the app, choose **Open the release page** (or open [github.com/thekozugroup/PopcornStash/releases](https://github.com/thekozugroup/PopcornStash/releases) and find the newest "Android app, build N"). Under **Assets**, download `popcornstash-android-N.apk`. The browser may warn that the file might be harmful; keep it.
2. Uninstall PopcornStash (press and hold its icon, then Uninstall). This signs it out and removes what it kept on the phone. Android will not install the new copy over the old one, because they are signed with different keys.
3. Open the downloaded file from the browser's downloads or the Files app. Android asks you to allow installing apps from that app ("Allow from this source"); allow it and install. Play Protect may ask too; choose to install anyway.
4. Open PopcornStash and sign in again: with your passkey, or pair with a code that you approve on the web ([Pair with a code](#pair-with-a-code)).

Then remove the old copy of this phone from **Settings, Devices** on the web. From now on the app updates itself.

## What is not done yet

- **Not run on a phone or an emulator.** The build machine has no emulator, so gestures, the photo viewer's zoom, real video decoding, picture-in-picture, rotation, the share sheet's background uploads and the passkey prompt have only run under Robolectric and against a real server from the JVM.
- **The frosted toolbar on Android 17 QPR2 has not run.** From Android 17 QPR2 (SDK 37.2) the platform blurs the window behind the bar itself (`RenderNode.setBackdropRenderEffect`). That code compiles and passes lint, but there was no 37.2 phone or emulator image to run it on, and Robolectric runs Android 16, so it has never drawn. The same goes for how the bar fades in and out there: without a layer of its own (`CompositingStrategy.ModulateAlpha`), because the platform would otherwise blur that empty layer instead of the pages. The glass on Android 12 to 17.1 (the app records the pages and blurs them; most phones show this until Android 17 QPR2 reaches Pixels in December 2026) is covered by screenshots and pixel checks, and the opaque bar on Android 9 to 11 by a screenshot.
- **No lock screen or headset controls** (no media session), **no subtitles**, and no double tap to seek in the player.
- **A shared `.torrent` file is uploaded as a file**, not added as a download.
- **Admin pages on the web:** VPN, people, devices, security, system, sorting and the trash open in the phone's browser, which has its own sign-in.
- **Imports for members:** the API does not say who added a waiting file, so a member's row whose suggestion names a library offers only that library, even for a file the member added (the web does the same).
- **Not in a store.** Builds come from GitHub releases ([Updates](#updates)); a copy built on your own computer is signed with your key and cannot update itself from them.
- **The updater has not run on a phone.** The check, the download, the checksum and signature checks and the install session are tested on the build machine (Robolectric, with a real signed APK from a mock GitHub), but Android's own install, its confirmation, the update without a confirmation on Android 12 and later and the notification permission have not been tried on a device. Robolectric cannot read an APK's certificates through PackageManager, so the tests read them with apksig instead; on a phone the app reads them through PackageManager, with GET_SIGNATURES as well on Android 9 and 10, which has not run either. Try one real update on an Android 9 phone or emulator before relying on it there.

## Choose where a download goes

“Where it goes” defaults to “Sorted automatically”. Choose any library you can see when adding a link or sharing files. Compatible files go there; other file kinds follow the server’s normal sorting rules. Uncertain files wait in Inbox with your choice suggested. A choice never changes where future automatic downloads go. If the library is hidden or deleted before sorting, normal sorting applies.
