package com.thekozugroup.popcornstash.data.auth

import okhttp3.HttpUrl
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull

/**
 * The server address a person types, reduced to scheme, host and port
 * (docs/architecture.md section 10, Server address). `https://` is assumed when no scheme
 * is typed. Plain `http://` is accepted only for this phone or the emulator's host
 * (`localhost`, `127.0.0.1`, `10.0.2.2`) and only in debug builds ([allowLocalHttp]).
 */
object ServerAddress {
    sealed interface Result {
        data class Valid(val url: HttpUrl) : Result
        data class Invalid(val message: String) : Result
    }

    private val localHosts = setOf("localhost", "127.0.0.1", "10.0.2.2")

    fun parse(input: String, allowLocalHttp: Boolean): Result {
        val text = input.trim()
        if (text.isEmpty()) return Result.Invalid("Enter the address of your PopcornStash server.")
        val scheme = text.substringBefore("://", missingDelimiterValue = "").lowercase()
        if (scheme.isNotEmpty() && scheme != "https" && scheme != "http") return Result.Invalid("Use an https address.")
        val withScheme = if (scheme.isEmpty()) "https://$text" else text
        val url = withScheme.toHttpUrlOrNull()
            ?: return Result.Invalid("That is not a web address. It looks like stash.example.com.")
        if (url.host.isEmpty()) return Result.Invalid("That is not a web address. It looks like stash.example.com.")
        val localHttp = url.scheme == "http" && url.host in localHosts && allowLocalHttp
        if (url.scheme != "https" && !localHttp) return Result.Invalid("Use an https address.")
        val base = HttpUrl.Builder().scheme(url.scheme).host(url.host).port(url.port).build()
        return Result.Valid(base)
    }

    /** How the address reads to a person: no scheme, no default port, no trailing slash. */
    fun display(url: HttpUrl): String {
        val defaultPort = HttpUrl.defaultPort(url.scheme)
        val host = if (url.host.contains(':')) "[${url.host}]" else url.host
        val port = if (url.port == defaultPort) "" else ":${url.port}"
        val scheme = if (url.scheme == "https") "" else "${url.scheme}://"
        return "$scheme$host$port"
    }
}
