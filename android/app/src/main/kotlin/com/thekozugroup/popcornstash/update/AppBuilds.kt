package com.thekozugroup.popcornstash.update

import com.thekozugroup.popcornstash.data.api.StashJson
import kotlinx.serialization.Serializable
import kotlinx.serialization.builtins.ListSerializer

/**
 * One release in GitHub's list (`GET /repos/{owner}/{repo}/releases`), only the fields the
 * app reads. scripts/android-release.sh makes the app's: tag `android-N`, a pre-release with
 * `popcornstash-android-N.apk` and `popcornstash-android-N.apk.sha256`. The server's releases
 * (`v0.2.0`) are in the same list and are ignored.
 */
@Serializable
data class GitHubRelease(
    val tagName: String = "",
    val draft: Boolean = false,
    val htmlUrl: String = "",
    val assets: List<GitHubAsset> = emptyList(),
)

@Serializable
data class GitHubAsset(
    val name: String = "",
    val browserDownloadUrl: String = "",
    val size: Long = 0,
    /** "uploaded" once the file is complete; "open" while it is still being sent. */
    val state: String = "uploaded",
)

/** A build of the app on GitHub: its number (the APK's versionCode) and where its files are. */
@Serializable
data class AppBuild(
    val number: Int,
    val apkUrl: String,
    val sha256Url: String,
    val pageUrl: String,
    val size: Long,
)

/** Reading GitHub's release list and choosing the build to offer. */
object AppBuilds {
    private val tagPattern = Regex("^android-([1-9][0-9]{0,8})$")

    /** GitHub's JSON as releases; throws [kotlinx.serialization.SerializationException] when it is not a list of them. */
    fun parse(json: String): List<GitHubRelease> = StashJson.decodeFromString(ListSerializer(GitHubRelease.serializer()), json)

    /**
     * [release] as a build, or null when it is not one the app can install: not an `android-N`
     * tag, a draft, without both files complete, or larger than [UpdateFetcher.MAX_APK_BYTES].
     */
    fun buildOf(release: GitHubRelease): AppBuild? {
        val number = tagPattern.matchEntire(release.tagName)?.groupValues?.get(1)?.toIntOrNull() ?: return null
        if (release.draft) return null
        val apkName = "popcornstash-android-$number.apk"
        val apk = release.assets.firstOrNull { it.name == apkName && it.state == "uploaded" } ?: return null
        val sha = release.assets.firstOrNull { it.name == "$apkName.sha256" && it.state == "uploaded" } ?: return null
        // Release builds refuse plain http anyway (no cleartext exception outside debug builds).
        if (!isWebAddress(apk.browserDownloadUrl) || !isWebAddress(sha.browserDownloadUrl)) return null
        // A build of the app is about 6 MB; a list that says more is not offered.
        if (apk.size > UpdateFetcher.MAX_APK_BYTES) return null
        return AppBuild(number, apk.browserDownloadUrl, sha.browserDownloadUrl, release.htmlUrl, apk.size)
    }

    private fun isWebAddress(url: String) = url.startsWith("https://") || url.startsWith("http://")

    /** The highest build in [releases], whatever is installed. */
    fun newest(releases: List<GitHubRelease>): AppBuild? = releases.mapNotNull(::buildOf).maxByOrNull { it.number }

    /** The build to offer to an app that is build [installed]: the newest, when it is higher. */
    fun offer(newest: AppBuild?, installed: Int): AppBuild? = newest?.takeIf { it.number > installed }

    /** The hex SHA-256 from a `.sha256` file as `sha256sum` writes it ("<hex>  <name>"), or null. */
    fun parseSha256(text: String): String? =
        text.trim().split(Regex("\\s+")).firstOrNull()?.lowercase()?.takeIf { Regex("^[0-9a-f]{64}$").matches(it) }
}
