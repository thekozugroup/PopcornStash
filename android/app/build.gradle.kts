import java.util.Locale

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.kotlin.serialization)
    alias(libs.plugins.roborazzi)
}

// Release signing comes only from the environment, so no keystore or password is
// ever written into the repository. Without these variables a release build is
// left unsigned; debug builds always use the SDK's debug key.
//   POPCORNSTASH_KEYSTORE           path to the .jks or .p12 file
//   POPCORNSTASH_KEYSTORE_PASSWORD  keystore password
//   POPCORNSTASH_KEY_ALIAS          key alias
//   POPCORNSTASH_KEY_PASSWORD       key password (defaults to the keystore password)
val releaseKeystore = providers.environmentVariable("POPCORNSTASH_KEYSTORE").orNull
val releaseStorePassword = providers.environmentVariable("POPCORNSTASH_KEYSTORE_PASSWORD").orNull
val releaseKeyAlias = providers.environmentVariable("POPCORNSTASH_KEY_ALIAS").orNull
val releaseKeyPassword = providers.environmentVariable("POPCORNSTASH_KEY_PASSWORD").orNull ?: releaseStorePassword
val signRelease = !releaseKeystore.isNullOrBlank() && !releaseStorePassword.isNullOrBlank() && !releaseKeyAlias.isNullOrBlank()

// The integration test (ServerIntegrationTest) runs a real `popcornstash serve` on a
// scripts/dev-fixture.sh stash. It runs only when the server binary is named:
//   ./gradlew :app:testDebugUnitTest --tests '*ServerIntegrationTest*' -PpopcornstashServer=../target/debug/popcornstash
// or with POPCORNSTASH_SERVER_BIN set. Without either it is skipped.
val repoRoot = rootProject.layout.projectDirectory.dir("..").asFile
val serverBinary = providers.gradleProperty("popcornstashServer")
    .orElse(providers.environmentVariable("POPCORNSTASH_SERVER_BIN"))
    .map { path -> File(path).let { if (it.isAbsolute) it else rootProject.file(path) }.absolutePath }
    .orNull

// The version: scripts/android-release.sh passes the build number (commits on main) and
// "<Cargo version>+<build>"; a local build is versionCode 1 with the Cargo workspace version.
val cargoVersion: String = repoRoot.resolve("Cargo.toml").readLines()
    .dropWhile { it.trim() != "[workspace.package]" }
    .firstNotNullOfOrNull { Regex("^version\\s*=\\s*\"([^\"]+)\"").find(it.trim())?.groupValues?.get(1) }
    ?: throw GradleException("No version in Cargo.toml [workspace.package].")
val appVersionCode: Int = providers.gradleProperty("popcornstashVersionCode").orNull?.toInt() ?: 1
val appVersionName: String = providers.gradleProperty("popcornstashVersionName").orNull ?: cargoVersion

// The release key's certificate SHA-256 (android/release-cert.sha256, public, committed once the
// owner has made the key). The app compares its own certificate with it to tell a copy signed with
// another key (a debug build), which cannot install a release over itself, before downloading one.
val releaseCertFile = rootProject.file("release-cert.sha256")
val releaseCertSha256: String = if (releaseCertFile.isFile) {
    releaseCertFile.readText().filterNot { it == ':' || it.isWhitespace() }.lowercase().also {
        if (it.isNotEmpty() && !Regex("^[0-9a-f]{64}$").matches(it)) {
            throw GradleException("android/release-cert.sha256 is not a SHA-256 fingerprint (64 hex digits, colons allowed).")
        }
    }
} else {
    ""
}

android {
    namespace = "com.thekozugroup.popcornstash"
    // Android 17 QPR2 (SDK 37.2): the floating toolbar's glass uses the platform's backdrop
    // blur there (RenderNode.setBackdropRenderEffect). It needs SDK Platform 37.2 installed:
    // sdkmanager "platforms;android-37.2".
    compileSdk {
        version = release(37) { minorApiLevel = 2 }
    }

    defaultConfig {
        applicationId = "com.thekozugroup.popcornstash"
        // Passkeys go through Credential Manager, which needs Android 9 (API 28).
        minSdk = 28
        targetSdk = 37
        versionCode = appVersionCode
        versionName = appVersionName
        buildConfigField("String", "RELEASE_CERT_SHA256", "\"$releaseCertSha256\"")
    }

    signingConfigs {
        if (signRelease) {
            create("release") {
                storeFile = file(releaseKeystore!!)
                storePassword = releaseStorePassword
                keyAlias = releaseKeyAlias
                keyPassword = releaseKeyPassword
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            signingConfig = if (signRelease) signingConfigs.getByName("release") else null
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    testOptions {
        unitTests {
            isIncludeAndroidResources = true
            all {
                // Robolectric draws with the real Android graphics stack, so
                // screenshots show shadows and anti-aliasing as a phone would.
                it.systemProperty("robolectric.graphicsMode", "NATIVE")
                it.systemProperty("robolectric.pixelCopyRenderMode", "hardware")
                it.systemProperty("popcornstash.repoRoot", repoRoot.absolutePath)
                if (serverBinary != null) it.systemProperty("popcornstash.serverBin", serverBinary)
                it.maxHeapSize = "2g"
                // Robolectric's Android 16 sets up shared memory through JDK internals.
                it.jvmArgs("--add-opens=java.base/jdk.internal.access=ALL-UNNAMED")
            }
        }
    }

    lint {
        abortOnError = true
        warningsAsErrors = true
        checkDependencies = true
        // Version checks need the network and change from day to day; dependency
        // updates are a deliberate step, not a lint failure.
        disable += setOf("GradleDependency", "NewerVersionAvailable", "AndroidGradlePluginVersion")
    }

    packaging {
        resources.excludes += setOf("/META-INF/{AL2.0,LGPL2.1}", "/META-INF/LICENSE*.md")
    }
}

// Files outside this project that the unit tests read. Gradle only knows about a test task's
// declared inputs, so without these a changed server, icon list or fixture script would let
// the build cache answer "passed" without running a single test.
tasks.withType<Test>().configureEach {
    // LibraryIconsTest: the server's and the web's icon lists.
    inputs.file(repoRoot.resolve("crates/stash-core/src/library/defs.rs"))
        .withPropertyName("serverIconList").withPathSensitivity(PathSensitivity.NONE)
    inputs.file(repoRoot.resolve("web/src/lib/libraryIcons.ts"))
        .withPropertyName("webIconList").withPathSensitivity(PathSensitivity.NONE)
    // The real-server tests: the stash they build and the server they run.
    inputs.file(repoRoot.resolve("scripts/dev-fixture.sh"))
        .withPropertyName("devFixture").withPathSensitivity(PathSensitivity.NONE)
    if (serverBinary != null) {
        inputs.file(serverBinary).withPropertyName("serverBinary").withPathSensitivity(PathSensitivity.NONE)
        // A run against a real server is never taken from the cache or skipped as up to date:
        // the server's behaviour is more than its binary (the fixture's media, ffmpeg, the clock).
        outputs.cacheIf("runs against a real server") { false }
        outputs.upToDateWhen { false }
    }
}

roborazzi {
    // Screenshots are reviewed and committed; verifyRoborazziDebug compares against them.
    outputDir.set(file("src/test/screenshots"))
}

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.lifecycle.process)
    implementation(libs.androidx.browser)
    implementation(libs.androidx.credentials)
    implementation(libs.androidx.credentials.play.services)

    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.ui.graphics)
    implementation(libs.compose.foundation)
    implementation(libs.compose.animation)
    implementation(libs.compose.material3)
    implementation(libs.compose.ui.tooling.preview)
    debugImplementation(libs.compose.ui.tooling)
    debugImplementation(libs.compose.ui.test.manifest)

    implementation(libs.kotlinx.coroutines.android)
    implementation(libs.kotlinx.serialization.json)
    implementation(libs.okhttp)
    implementation(libs.okhttp.sse)
    // Thumbnails and photos, loaded with the session's bearer client (feature/home).
    implementation(libs.coil.compose)
    implementation(libs.coil.network.okhttp)

    // The player (feature/player): ExoPlayer, HLS, requests through the session's OkHttp
    // client (so the bearer token goes only to the server), and the Compose video surface.
    implementation(libs.media3.exoplayer)
    implementation(libs.media3.exoplayer.hls)
    implementation(libs.media3.datasource.okhttp)
    implementation(libs.media3.ui.compose)

    // Share sheet uploads (share/): a WorkManager job with a foreground notification.
    implementation(libs.androidx.work.runtime)

    testImplementation(platform(libs.compose.bom))
    testImplementation(libs.compose.ui.test.junit4)
    testImplementation(libs.junit)
    testImplementation(libs.robolectric)
    testImplementation(libs.androidx.test.core)
    testImplementation(libs.androidx.test.ext.junit)
    testImplementation(libs.roborazzi)
    testImplementation(libs.roborazzi.compose)
    testImplementation(libs.roborazzi.junit.rule)
    testImplementation(libs.kotlinx.coroutines.test)
    testImplementation(libs.okhttp.mockwebserver)
    testImplementation(libs.coil.test)
    testImplementation(libs.androidx.work.testing)
    testImplementation(libs.apksig)
}

// ---------------------------------------------------------------- Lucide glyphs ----
// The app draws the same Lucide glyphs as the web app, from the same release: the
// web's lucide-react (web/package.json). `generateLucideGlyphs` turns the SVG nodes of
// the icons the app uses into Kotlin path data (ui/icons/LucideGlyphs.kt, committed);
// `checkLucideGlyphs` fails when that file no longer matches the web's lucide-react.
// Both need `npm --prefix web ci` first; without web/node_modules the check is skipped.

/** Every Lucide icon the app draws: the library icon list (docs/architecture.md section 8) and the fixed glyphs. */
val lucideIconNames = listOf(
    // Library icons, in picker order (stash-core LIBRARY_ICONS).
    "film", "clapperboard", "tv", "popcorn", "projector", "video", "drama",
    "image", "images", "camera", "aperture",
    "users", "baby", "heart", "cake", "party-popper", "graduation-cap", "gift",
    "plane", "tree-palm", "mountain-snow", "tent", "sun", "snowflake",
    "gamepad-2", "music", "book-open", "trophy", "paw-print", "bike",
    "star",
    // Fixed places and actions (android/DESIGN.md, Touch and icons); `folder` also draws a
    // library icon the app does not know.
    "folder", "house", "arrow-down-to-line", "settings", "file-text", "inbox",
    "plus", "pause", "play", "ellipsis-vertical", "arrow-left", "external-link", "x",
    "server", "log-out", "chevron-right", "chevron-down", "check", "key-round", "link",
    "smartphone", "rotate-ccw",
    // Home and libraries: save the original, details, sort, file types.
    "download", "info", "arrow-up-down", "file-archive",
)

abstract class LucideGlyphsTask : DefaultTask() {
    @get:Internal
    abstract val iconsDir: DirectoryProperty

    @get:Input
    abstract val names: ListProperty<String>

    @get:Internal
    abstract val packageJson: RegularFileProperty

    @get:Internal
    abstract val target: RegularFileProperty

    @get:Input
    abstract val verifyOnly: Property<Boolean>

    @TaskAction
    fun run() {
        val dir = iconsDir.get().asFile
        val out = target.get().asFile
        if (!dir.isDirectory) {
            if (verifyOnly.get()) {
                logger.lifecycle("Skipping the Lucide glyph check: run `npm --prefix web ci` to compare with lucide-react.")
                return
            }
            throw GradleException("No lucide-react in web/node_modules. Run `npm --prefix web ci` first.")
        }
        val version = Regex("\"version\"\\s*:\\s*\"([^\"]+)\"").find(packageJson.get().asFile.readText())?.groupValues?.get(1)
            ?: throw GradleException("No version in lucide-react's package.json.")
        val generated = LucideKotlin.render(version, names.get().map { it to File(dir, "$it.mjs") })
        if (verifyOnly.get()) {
            if (!out.isFile || out.readText() != generated) {
                throw GradleException(
                    "${out.name} does not match lucide-react $version. Run ./gradlew :app:generateLucideGlyphs.",
                )
            }
            logger.lifecycle("LucideGlyphs.kt matches lucide-react $version.")
        } else {
            out.writeText(generated)
            logger.lifecycle("Wrote ${out.name} from lucide-react $version (${names.get().size} glyphs).")
        }
    }
}

/** Turns lucide-react icon nodes (path, circle, ellipse, rect, line, polyline, polygon) into SVG path data. */
object LucideKotlin {
    private val nodePattern = Regex("""\[\s*"(\w+)",\s*\{([^}]*)\}\s*\]""")
    private val attributePattern = Regex("""(\w+):\s*"([^"]*)"""")

    fun render(version: String, icons: List<Pair<String, File>>): String {
        val body = StringBuilder()
        for ((name, file) in icons) {
            if (!file.isFile) throw GradleException("lucide-react $version has no icon named $name.")
            val nodes = nodePattern.findAll(file.readText()).map { match ->
                val attributes = attributePattern.findAll(match.groupValues[2]).associate { it.groupValues[1] to it.groupValues[2] }
                toPath(name, match.groupValues[1], attributes)
            }.toList()
            if (nodes.isEmpty()) throw GradleException("Could not read the shapes of $name.")
            body.append("    val ${constantName(name)} = LucideGlyph(\n")
            body.append("        \"$name\",\n")
            body.append("        listOf(\n")
            nodes.forEach { body.append("            \"$it\",\n") }
            body.append("        ),\n")
            body.append("    )\n\n")
        }
        return buildString {
            append("// Generated by ./gradlew :app:generateLucideGlyphs from lucide-react $version (web/node_modules).\n")
            append("// Do not edit; add names to lucideIconNames in app/build.gradle.kts instead.\n")
            append("// Lucide is ISC licensed: https://lucide.dev/license\n")
            append("package com.thekozugroup.popcornstash.ui.icons\n\n")
            append("/** Lucide $version glyphs as SVG path data on the 24 x 24 grid. */\n")
            append("object LucideGlyphs {\n")
            append("    const val LUCIDE_VERSION = \"$version\"\n\n")
            append(body)
            append("    val all: List<LucideGlyph> = listOf(\n")
            icons.forEach { (name, _) -> append("        ${constantName(name)},\n") }
            append("    )\n")
            append("}\n")
        }
    }

    private fun constantName(name: String): String =
        name.split('-').joinToString("") { part -> part.replaceFirstChar { it.titlecase(Locale.ROOT) } }

    private fun num(value: Double): String {
        val rounded = Math.round(value * 1000.0) / 1000.0
        return if (rounded == Math.floor(rounded)) rounded.toLong().toString() else rounded.toString()
    }

    private fun toPath(icon: String, tag: String, a: Map<String, String>): String {
        fun d(key: String, default: Double? = null): Double =
            a[key]?.toDouble() ?: default ?: throw GradleException("$icon: $tag without $key")
        return when (tag) {
            "path" -> a["d"] ?: throw GradleException("$icon: path without d")
            "circle" -> ellipse(d("cx"), d("cy"), d("r"), d("r"))
            "ellipse" -> ellipse(d("cx"), d("cy"), d("rx"), d("ry"))
            "line" -> "M${num(d("x1"))} ${num(d("y1"))}L${num(d("x2"))} ${num(d("y2"))}"
            "polyline", "polygon" -> {
                val points = (a["points"] ?: throw GradleException("$icon: $tag without points"))
                    .trim().split(Regex("[\\s,]+")).map { it.toDouble() }.chunked(2)
                val head = "M${num(points[0][0])} ${num(points[0][1])}"
                val rest = points.drop(1).joinToString("") { "L${num(it[0])} ${num(it[1])}" }
                head + rest + if (tag == "polygon") "Z" else ""
            }
            "rect" -> {
                val x = d("x", 0.0)
                val y = d("y", 0.0)
                val w = d("width")
                val h = d("height")
                var rx = a["rx"]?.toDouble() ?: a["ry"]?.toDouble() ?: 0.0
                var ry = a["ry"]?.toDouble() ?: rx
                rx = minOf(rx, w / 2)
                ry = minOf(ry, h / 2)
                if (rx == 0.0 || ry == 0.0) {
                    "M${num(x)} ${num(y)}h${num(w)}v${num(h)}h${num(-w)}Z"
                } else {
                    "M${num(x + rx)} ${num(y)}h${num(w - 2 * rx)}" +
                        "a${num(rx)} ${num(ry)} 0 0 1 ${num(rx)} ${num(ry)}v${num(h - 2 * ry)}" +
                        "a${num(rx)} ${num(ry)} 0 0 1 ${num(-rx)} ${num(ry)}h${num(-(w - 2 * rx))}" +
                        "a${num(rx)} ${num(ry)} 0 0 1 ${num(-rx)} ${num(-ry)}v${num(-(h - 2 * ry))}" +
                        "a${num(rx)} ${num(ry)} 0 0 1 ${num(rx)} ${num(-ry)}Z"
                }
            }
            else -> throw GradleException("$icon: unsupported SVG element $tag")
        }
    }

    private fun ellipse(cx: Double, cy: Double, rx: Double, ry: Double): String =
        "M${num(cx - rx)} ${num(cy)}a${num(rx)} ${num(ry)} 0 1 0 ${num(2 * rx)} 0" +
            "a${num(rx)} ${num(ry)} 0 1 0 ${num(-2 * rx)} 0Z"
}

val lucideDir = repoRoot.resolve("web/node_modules/lucide-react")
val glyphsFile = layout.projectDirectory.file("src/main/kotlin/com/thekozugroup/popcornstash/ui/icons/LucideGlyphs.kt")

tasks.register<LucideGlyphsTask>("generateLucideGlyphs") {
    description = "Writes ui/icons/LucideGlyphs.kt from the web's lucide-react."
    iconsDir.set(lucideDir.resolve("dist/esm/icons"))
    packageJson.set(lucideDir.resolve("package.json"))
    names.set(lucideIconNames)
    target.set(glyphsFile)
    verifyOnly.set(false)
}

val checkLucideGlyphs = tasks.register<LucideGlyphsTask>("checkLucideGlyphs") {
    description = "Checks that ui/icons/LucideGlyphs.kt matches the web's lucide-react."
    group = "verification"
    iconsDir.set(lucideDir.resolve("dist/esm/icons"))
    packageJson.set(lucideDir.resolve("package.json"))
    names.set(lucideIconNames)
    target.set(glyphsFile)
    verifyOnly.set(true)
}

tasks.named("check") { dependsOn(checkLucideGlyphs) }
