package com.thekozugroup.popcornstash.data.api

import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.serialization.ExperimentalSerializationApi
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonNamingStrategy
import okhttp3.Call
import okhttp3.Callback
import okhttp3.HttpUrl
import okhttp3.Interceptor
import okhttp3.OkHttpClient
import okhttp3.Response
import java.io.IOException
import java.util.concurrent.TimeUnit
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

/** JSON as the server writes it: snake_case names, unknown fields and enum words tolerated. */
@OptIn(ExperimentalSerializationApi::class)
val StashJson: Json = Json {
    namingStrategy = JsonNamingStrategy.SnakeCase
    ignoreUnknownKeys = true
    coerceInputValues = true
    explicitNulls = false
    encodeDefaults = true
}

/**
 * The one HTTP client of the app process. No logging interceptor, ever: requests carry the
 * bearer token and answers carry the family's names and pictures. No HTTP cache either,
 * so nothing a person loaded outlives their session on disk.
 */
fun baseHttpClient(userAgent: String): OkHttpClient =
    OkHttpClient.Builder()
        .connectTimeout(15, TimeUnit.SECONDS)
        .readTimeout(30, TimeUnit.SECONDS)
        .writeTimeout(60, TimeUnit.SECONDS)
        .followRedirects(false)
        .addInterceptor { chain ->
            chain.proceed(chain.request().newBuilder().header("User-Agent", userAgent).build())
        }
        .build()

/**
 * Adds `Authorization: Bearer` to requests for [server] only (scheme, host and port), so the
 * token never goes anywhere else, and reports a 401 to such a request through [onUnauthorized].
 */
class AuthInterceptor(
    private val server: HttpUrl,
    private val token: () -> String?,
    private val onUnauthorized: () -> Unit,
) : Interceptor {
    override fun intercept(chain: Interceptor.Chain): Response {
        val request = chain.request()
        val bearer = token()?.takeIf { sameOrigin(request.url, server) }
        if (bearer == null) return chain.proceed(request)
        val response = chain.proceed(request.newBuilder().header("Authorization", "Bearer $bearer").build())
        if (response.code == 401) onUnauthorized()
        return response
    }
}

fun sameOrigin(a: HttpUrl, b: HttpUrl): Boolean = a.scheme == b.scheme && a.host == b.host && a.port == b.port

/** Runs the call on OkHttp's threads and cancels it when the coroutine is cancelled. */
suspend fun Call.await(): Response = suspendCancellableCoroutine { continuation ->
    continuation.invokeOnCancellation { cancel() }
    enqueue(
        object : Callback {
            override fun onFailure(call: Call, e: IOException) {
                continuation.resumeWithException(e)
            }

            override fun onResponse(call: Call, response: Response) {
                continuation.resume(response) { _, value, _ -> value.close() }
            }
        },
    )
}
