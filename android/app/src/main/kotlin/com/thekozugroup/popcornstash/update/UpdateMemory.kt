package com.thekozugroup.popcornstash.update

import android.content.Context
import android.content.SharedPreferences
import androidx.core.content.edit
import com.thekozugroup.popcornstash.data.api.StashJson
import kotlinx.serialization.Serializable
import kotlinx.serialization.SerializationException

/** A downloaded build that passed every check and waits to be installed. */
@Serializable
data class ReadyBuild(val build: AppBuild, val sha256: String, val fileName: String)

/**
 * What the updater keeps between runs. None of it is about the person or their server: the
 * last answer from GitHub (its ETag and the newest build it listed), when the app last asked,
 * whether it asks by itself, the last build it notified about, and a verified download.
 */
interface UpdateMemory {
    val etag: String?
    val newestSeen: AppBuild?

    /** The answer to a 200: its ETag and the newest build in it, kept together. */
    fun remember(etag: String?, newest: AppBuild?)

    var lastChecked: Long?
    var autoCheck: Boolean
    var lastNotified: Int
    var ready: ReadyBuild?
}

/** [UpdateMemory] in the app's own preferences ("updates"); not backed up, like everything else. */
class PreferencesUpdateMemory(private val preferences: SharedPreferences) : UpdateMemory {
    constructor(context: Context) : this(context.getSharedPreferences("updates", Context.MODE_PRIVATE))

    override val etag: String? get() = preferences.getString(ETAG, null)

    override val newestSeen: AppBuild?
        get() = decode(preferences.getString(NEWEST, null), AppBuild.serializer())

    override fun remember(etag: String?, newest: AppBuild?) = preferences.edit {
        putString(ETAG, etag)
        putString(NEWEST, newest?.let { StashJson.encodeToString(AppBuild.serializer(), it) })
    }

    override var lastChecked: Long?
        get() = preferences.getLong(LAST_CHECKED, 0L).takeIf { it > 0 }
        set(value) = preferences.edit { if (value == null) remove(LAST_CHECKED) else putLong(LAST_CHECKED, value) }

    override var autoCheck: Boolean
        get() = preferences.getBoolean(AUTO_CHECK, true)
        set(value) = preferences.edit { putBoolean(AUTO_CHECK, value) }

    override var lastNotified: Int
        get() = preferences.getInt(LAST_NOTIFIED, 0)
        set(value) = preferences.edit { putInt(LAST_NOTIFIED, value) }

    override var ready: ReadyBuild?
        get() = decode(preferences.getString(READY, null), ReadyBuild.serializer())
        set(value) = preferences.edit { putString(READY, value?.let { StashJson.encodeToString(ReadyBuild.serializer(), it) }) }

    private fun <T> decode(text: String?, serializer: kotlinx.serialization.KSerializer<T>): T? = text?.let {
        try {
            StashJson.decodeFromString(serializer, it)
        } catch (_: SerializationException) {
            null
        } catch (_: IllegalArgumentException) {
            null
        }
    }

    private companion object {
        const val ETAG = "etag"
        const val NEWEST = "newest"
        const val LAST_CHECKED = "last_checked"
        const val AUTO_CHECK = "auto_check"
        const val LAST_NOTIFIED = "last_notified"
        const val READY = "ready"
    }
}

/** [UpdateMemory] in memory, for tests and previews. */
class InMemoryUpdateMemory(
    override var lastChecked: Long? = null,
    override var autoCheck: Boolean = true,
    override var lastNotified: Int = 0,
    override var ready: ReadyBuild? = null,
) : UpdateMemory {
    override var etag: String? = null
        private set
    override var newestSeen: AppBuild? = null
        private set

    override fun remember(etag: String?, newest: AppBuild?) {
        this.etag = etag
        newestSeen = newest
    }
}
