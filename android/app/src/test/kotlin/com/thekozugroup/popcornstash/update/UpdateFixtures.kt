package com.thekozugroup.popcornstash.update

/** GitHub's release list as the API writes it, trimmed to what matters and a little of what does not. */
object UpdateFixtures {
    data class Asset(val name: String, val url: String, val size: Long = 8538, val state: String = "uploaded")

    fun release(
        tag: String,
        assets: List<Asset>,
        draft: Boolean = false,
        prerelease: Boolean = true,
        page: String = "https://github.com/thekozugroup/PopcornStash/releases/tag/$tag",
    ): String = """
        {
          "url": "https://api.github.com/repos/thekozugroup/PopcornStash/releases/1",
          "html_url": "$page",
          "id": ${tag.hashCode() and 0xffff},
          "tag_name": "$tag",
          "target_commitish": "0123456789abcdef0123456789abcdef01234567",
          "name": "${tag.replace("android-", "Android app, build ")}",
          "draft": $draft,
          "prerelease": $prerelease,
          "created_at": "2026-10-14T09:00:00Z",
          "published_at": "2026-10-14T09:05:00Z",
          "author": {"login": "thekozugroup", "id": 1},
          "assets": [${assets.joinToString(",") { asset(it) }}],
          "body": "Built from commit 0123456 on main."
        }
    """.trimIndent()

    private fun asset(a: Asset) = """
        {"url": "https://api.github.com/repos/thekozugroup/PopcornStash/releases/assets/9", "id": 9,
         "name": "${a.name}", "label": null, "content_type": "application/octet-stream",
         "state": "${a.state}", "size": ${a.size}, "download_count": 3,
         "browser_download_url": "${a.url}"}
    """.trimIndent()

    /** An app build's release with both files under [base] (no trailing slash). */
    fun appRelease(number: Int, base: String = "https://github.com/thekozugroup/PopcornStash/releases/download/android-$number", draft: Boolean = false) =
        release(
            "android-$number",
            listOf(
                Asset("popcornstash-android-$number.apk", "$base/popcornstash-android-$number.apk"),
                Asset("popcornstash-android-$number.apk.sha256", "$base/popcornstash-android-$number.apk.sha256", size = 92),
            ),
            draft = draft,
        )

    /** A server release, as scripts/release.sh makes them: not for the app. */
    fun serverRelease(version: String) = release(
        "v$version",
        listOf(Asset("popcornstash-extension.zip", "https://github.com/thekozugroup/PopcornStash/releases/download/v$version/popcornstash-extension.zip")),
        prerelease = false,
    )

    fun list(vararg releases: String) = releases.joinToString(",", prefix = "[", postfix = "]")

    fun build(number: Int, base: String = "https://example.test/android-$number") = AppBuild(
        number = number,
        apkUrl = "$base/popcornstash-android-$number.apk",
        sha256Url = "$base/popcornstash-android-$number.apk.sha256",
        pageUrl = "https://github.com/thekozugroup/PopcornStash/releases/tag/android-$number",
        size = 8538,
    )
}
