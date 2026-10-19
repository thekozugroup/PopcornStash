package com.thekozugroup.popcornstash.feature.downloads

import com.thekozugroup.popcornstash.data.api.TorrentFileKind
import com.thekozugroup.popcornstash.data.api.TorrentFileRules
import java.math.BigDecimal
import java.math.RoundingMode

data class TorrentRuleDraft(
    val enabled: Boolean = false,
    val include: String = "",
    val exclude: String = "",
    val minimum: String = "",
    val maximum: String = "",
    val gigabytes: Boolean = false,
) {
    fun rules(types: List<TorrentFileKind>): TorrentFileRules? {
        if (!enabled) return null
        require(types.isNotEmpty()) { "Choose at least one file type for the rules." }
        fun names(text: String): List<String> = text.split(',').map(String::trim).filter(String::isNotEmpty).also { names ->
            require(names.size <= 32 && names.all { name ->
                name.any(Char::isLetterOrDigit) && name.toByteArray(Charsets.UTF_8).size <= 128 && name.none(Char::isISOControl)
            }) { "Use up to 32 names per field, each up to 128 bytes and containing letters or numbers." }
        }
        fun size(text: String): Long? {
            val value = text.trim()
            if (value.isEmpty()) return null
            require(Regex("(?:\\d+(?:\\.\\d*)?|\\.\\d+)").matches(value) && value.length <= 32) { "Enter a valid positive file size, or leave it blank." }
            val bytes = BigDecimal(value).multiply(BigDecimal(if (gigabytes) 1_000_000_000L else 1_000_000L)).setScale(0, RoundingMode.HALF_UP)
            require(bytes <= BigDecimal(9_007_199_254_740_991L)) { "File size is too large." }
            return bytes.longValueExact()
        }
        val min = size(minimum)
        val max = size(maximum)
        require(min == null || max == null || min <= max) { "Minimum size must be no greater than maximum size." }
        return TorrentFileRules(types, names(include), names(exclude), min, max)
    }
}
