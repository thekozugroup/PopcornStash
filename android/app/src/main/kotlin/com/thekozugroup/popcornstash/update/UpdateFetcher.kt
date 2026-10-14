package com.thekozugroup.popcornstash.update

import com.thekozugroup.popcornstash.data.api.await
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.withContext
import okhttp3.OkHttpClient
import okhttp3.Request
import java.io.File
import java.io.IOException
import java.security.MessageDigest
import kotlin.coroutines.coroutineContext

/** What an APK says about itself: its package, its build and the certificates it is signed with (SHA-256, hex). */
data class ApkFacts(
    val packageName: String,
    val versionCode: Long,
    /** The certificates that sign it now. */
    val signers: Set<String>,
    /** Every certificate in its signing lineage, older keys included (key rotation). */
    val lineage: Set<String> = signers,
)

/** Reads [ApkFacts] from an APK on disk ([PackageManagerApks] on a phone); null when it is not a readable APK. */
fun interface ApkInspector {
    fun inspect(apk: File): ApkFacts?
}

/** Why a download did not end as a verified APK. */
sealed interface FetchProblem {
    /** The connection broke, or GitHub answered with an error. */
    data object DownloadStopped : FetchProblem

    /** The file's SHA-256 is not the one in its `.sha256` file, or that file is unusable. */
    data object ChecksumMismatch : FetchProblem

    /** The file is not an APK of this app, or not the build it claims to be. */
    data object NotThisApp : FetchProblem

    /** The APK is signed with a key other than the installed app's, so Android would refuse it. */
    data object OtherSigner : FetchProblem
}

sealed interface FetchOutcome {
    data class Verified(val file: File, val sha256: String) : FetchOutcome

    data class Failed(val problem: FetchProblem) : FetchOutcome
}

/**
 * Downloads a build into [dir] (the app's cache) and checks it before anything installs it:
 * its SHA-256 against the build's `.sha256` file, then with [inspector] that it is this app
 * ([packageName]), the build it says it is, and signed by the installed app's certificate
 * ([installedSigners]). Whatever fails a check is deleted at once; so is a download that is
 * stopped. Only one build is kept at a time.
 */
class UpdateFetcher(
    private val http: OkHttpClient,
    val dir: File,
    private val packageName: String,
    private val inspector: ApkInspector,
    private val installedSigners: () -> Set<String>,
) {
    /**
     * Fetches [build]. [onProgress] gets the bytes so far and the total when known;
     * [onVerifying] is called once the file is all there and the checks begin.
     */
    suspend fun fetch(
        build: AppBuild,
        onProgress: suspend (done: Long, total: Long?) -> Unit = { _, _ -> },
        onVerifying: suspend () -> Unit = {},
    ): FetchOutcome = withContext(Dispatchers.IO) {
        clear()
        dir.mkdirs()
        val name = fileName(build)
        val part = File(dir, "$name.part")
        val target = File(dir, name)
        var keep = false
        try {
            val expected = when (val sha = fetchText(build.sha256Url)) {
                null -> return@withContext FetchOutcome.Failed(FetchProblem.DownloadStopped)
                else -> AppBuilds.parseSha256(sha) ?: return@withContext FetchOutcome.Failed(FetchProblem.ChecksumMismatch)
            }
            val actual = download(build, part, onProgress) ?: return@withContext FetchOutcome.Failed(FetchProblem.DownloadStopped)
            onVerifying()
            if (actual != expected) return@withContext FetchOutcome.Failed(FetchProblem.ChecksumMismatch)
            if (!part.renameTo(target)) return@withContext FetchOutcome.Failed(FetchProblem.DownloadStopped)
            val facts = inspector.inspect(target)
            if (facts == null || facts.packageName != packageName || facts.versionCode != build.number.toLong()) {
                return@withContext FetchOutcome.Failed(FetchProblem.NotThisApp)
            }
            if (!sameSigner(installedSigners(), facts)) return@withContext FetchOutcome.Failed(FetchProblem.OtherSigner)
            keep = true
            FetchOutcome.Verified(target, actual)
        } finally {
            part.delete()
            if (!keep) target.delete()
        }
    }

    /** Deletes every downloaded file. */
    fun clear() {
        dir.listFiles()?.forEach { it.deleteRecursively() }
    }

    private suspend fun fetchText(url: String): String? = try {
        http.newCall(Request.Builder().url(url).build()).await().use { response ->
            if (!response.isSuccessful) return null
            // A .sha256 file is one line; anything larger than a few KB is not one (and reads as no checksum).
            val source = response.body.source()
            if (source.request(MAX_SHA_FILE + 1)) return ""
            source.buffer.readUtf8()
        }
    } catch (_: IOException) {
        null
    } catch (_: IllegalArgumentException) {
        null
    }

    /** Writes the APK to [part] and returns its SHA-256, or null when the download broke. */
    private suspend fun download(build: AppBuild, part: File, onProgress: suspend (Long, Long?) -> Unit): String? = try {
        http.newCall(Request.Builder().url(build.apkUrl).build()).await().use { response ->
            if (!response.isSuccessful) return null
            val body = response.body
            val total = body.contentLength().takeIf { it > 0 } ?: build.size.takeIf { it > 0 }
            val digest = MessageDigest.getInstance("SHA-256")
            var done = 0L
            var reported = -1L
            body.byteStream().use { input ->
                part.outputStream().use { output ->
                    val buffer = ByteArray(64 * 1024)
                    while (true) {
                        coroutineContext.ensureActive()
                        val read = input.read(buffer)
                        if (read < 0) break
                        output.write(buffer, 0, read)
                        digest.update(buffer, 0, read)
                        done += read
                        // About every 1% (or 256 KB without a total), not on every buffer.
                        val step = total?.let { maxOf(it / 100, 1L) } ?: (256L * 1024)
                        if (done - reported >= step) {
                            reported = done
                            onProgress(done, total)
                        }
                    }
                }
            }
            if (total != null && done != total) return null
            onProgress(done, total)
            digest.digest().toHex()
        }
    } catch (_: IOException) {
        null
    } catch (_: IllegalArgumentException) {
        null
    }

    companion object {
        private const val MAX_SHA_FILE = 4096L

        fun fileName(build: AppBuild) = "popcornstash-android-${build.number}.apk"

        /**
         * Android installs an update only when it is signed by the installed app's key, or by a
         * key whose lineage includes it. An app with no known signer never updates itself.
         */
        fun sameSigner(installed: Set<String>, apk: ApkFacts): Boolean =
            installed.isNotEmpty() && (apk.signers == installed || apk.lineage.containsAll(installed))
    }
}

fun ByteArray.toHex(): String = joinToString("") { "%02x".format(it) }

/** The SHA-256 of [file] in hex. */
fun sha256Of(file: File): String {
    val digest = MessageDigest.getInstance("SHA-256")
    file.inputStream().use { input ->
        val buffer = ByteArray(64 * 1024)
        while (true) {
            val read = input.read(buffer)
            if (read < 0) break
            digest.update(buffer, 0, read)
        }
    }
    return digest.digest().toHex()
}
