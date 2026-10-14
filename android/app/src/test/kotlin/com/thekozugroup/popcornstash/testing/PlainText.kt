package com.thekozugroup.popcornstash.testing

import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsMatcher
import java.util.TimeZone

/**
 * Text as a person reads it: the no-break spaces that keep a date or a name on one line
 * (and the narrow one the phone's clock format puts before "PM") read as spaces, and the
 * zero-width spaces that let an address wrap after its dots are not there.
 */
fun plain(text: String): String = text.replace(' ', ' ').replace(' ', ' ').replace("​", "")

/** A node whose text reads [text] once [plain]: what a person sees, whatever keeps it whole or lets it wrap. */
fun hasPlainText(text: String): SemanticsMatcher = SemanticsMatcher("reads \"$text\"") { node ->
    node.config.getOrNull(SemanticsProperties.Text).orEmpty().any { plain(it.text) == text }
}

/**
 * Zones far apart on both sides of UTC, including the first and the last to reach a date,
 * so a time moved to the phone's zone would land on another hour, day or month.
 */
val ZonesAroundTheWorld = listOf(
    "America/Los_Angeles",
    "America/New_York",
    "UTC",
    "Europe/Lisbon",
    "Asia/Tokyo",
    "Pacific/Kiritimati",
    "Pacific/Pago_Pago",
)

/** Runs [block] once with each of [zones] as the phone's time zone, then puts the zone back. */
fun inEachZone(zones: List<String> = ZonesAroundTheWorld, block: (zone: String) -> Unit) {
    val before = TimeZone.getDefault()
    try {
        zones.forEach { zone ->
            TimeZone.setDefault(TimeZone.getTimeZone(zone))
            block(zone)
        }
    } finally {
        TimeZone.setDefault(before)
    }
}
