package com.thekozugroup.popcornstash.update

import com.thekozugroup.popcornstash.data.api.await
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.SerializationException
import okhttp3.HttpUrl
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.OkHttpClient
import okhttp3.Request
import java.io.IOException

/** Why a check found nothing. */
sealed interface CheckProblem {
    /** No connection to GitHub. */
    data object Offline : CheckProblem

    /** GitHub's limit of 60 unsigned requests an hour for this network; [resetsAt] in epoch milliseconds when GitHub said. */
    data class RateLimited(val resetsAt: Long?) : CheckProblem

    /** Any other answer than 200 or 304. */
    data class HttpError(val code: Int) : CheckProblem

    /** A 200 whose body is not a release list. */
    data object Unreadable : CheckProblem
}

sealed interface CheckResult {
    /** GitHub answered: [newest] is the highest app build there (null when there is none). */
    data class Answered(val newest: AppBuild?, val checkedAt: Long) : CheckResult

    data class Failed(val problem: CheckProblem) : CheckResult
}

/**
 * Asks GitHub for the repository's releases, without a login: the repository is public.
 * The answer's ETag is kept with the newest build it held, and sent back as If-None-Match,
 * so an unchanged list comes back as an empty 304. That saves the download, not the hourly
 * limit: GitHub counts a 304 to a request without a login like any other.
 * The request carries the app's User-Agent and nothing else about the phone or the person.
 */
class UpdateChecker(
    private val http: OkHttpClient,
    private val memory: UpdateMemory,
    private val releasesUrl: HttpUrl = RELEASES_URL.toHttpUrl(),
    private val clock: () -> Long = System::currentTimeMillis,
) {
    suspend fun check(): CheckResult = withContext(Dispatchers.IO) {
        val known = memory.newestSeen
        val etag = memory.etag
        val request = Request.Builder()
            .url(releasesUrl)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .apply { if (etag != null) header("If-None-Match", etag) }
            .build()
        val response = try {
            http.newCall(request).await()
        } catch (_: IOException) {
            return@withContext CheckResult.Failed(CheckProblem.Offline)
        }
        response.use {
            val now = clock()
            when {
                it.code == 304 && etag != null -> {
                    memory.lastChecked = now
                    CheckResult.Answered(known, now)
                }
                it.code == 200 -> {
                    val body = try {
                        it.body.string()
                    } catch (_: IOException) {
                        return@withContext CheckResult.Failed(CheckProblem.Offline)
                    }
                    val newest = try {
                        AppBuilds.newest(AppBuilds.parse(body))
                    } catch (_: SerializationException) {
                        return@withContext CheckResult.Failed(CheckProblem.Unreadable)
                    } catch (_: IllegalArgumentException) {
                        return@withContext CheckResult.Failed(CheckProblem.Unreadable)
                    }
                    memory.remember(it.header("ETag"), newest)
                    memory.lastChecked = now
                    CheckResult.Answered(newest, now)
                }
                (it.code == 403 || it.code == 429) && (it.header("x-ratelimit-remaining") == "0" || it.code == 429) -> {
                    val reset = it.header("x-ratelimit-reset")?.toLongOrNull()?.times(1000)
                    val retryAfter = it.header("Retry-After")?.toLongOrNull()?.let { seconds -> now + seconds * 1000 }
                    CheckResult.Failed(CheckProblem.RateLimited(reset ?: retryAfter))
                }
                else -> CheckResult.Failed(CheckProblem.HttpError(it.code))
            }
        }
    }

    companion object {
        /** The public repository's last 30 releases, newest first. */
        const val RELEASES_URL = "https://api.github.com/repos/thekozugroup/PopcornStash/releases?per_page=30"

        /** Where a person can see every build and install one by hand. */
        const val RELEASES_PAGE = "https://github.com/thekozugroup/PopcornStash/releases"
    }
}
