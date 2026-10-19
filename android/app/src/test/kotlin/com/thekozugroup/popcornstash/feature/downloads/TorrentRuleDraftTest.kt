package com.thekozugroup.popcornstash.feature.downloads

import com.thekozugroup.popcornstash.data.api.TorrentFileKind
import org.junit.Assert.*
import org.junit.Test

class TorrentRuleDraftTest {
    private val video = listOf(TorrentFileKind.Video)
    @Test fun disabledRulesDoNotChangeOldRequests() { assertNull(TorrentRuleDraft(include = "Jane").rules(emptyList())) }
    @Test fun namesAndDecimalSizesBecomeOnePolicy() {
        val rules = TorrentRuleDraft(true, " Jane Doe, Alex Smith ", "sample", ".5", "2.25", true).rules(video)!!
        assertEquals(listOf("Jane Doe", "Alex Smith"), rules.nameContains)
        assertEquals(listOf("sample"), rules.nameExcludes)
        assertEquals(500_000_000L, rules.minSize)
        assertEquals(2_250_000_000L, rules.maxSize)
        assertEquals(video, rules.fileTypes)
    }
    @Test fun invalidPoliciesAreRejectedBeforeSending() {
        listOf(TorrentRuleDraft(true, minimum = "2", maximum = "1"), TorrentRuleDraft(true, minimum = "-1"),
            TorrentRuleDraft(true, minimum = "NaN"), TorrentRuleDraft(true, include = "..."),
            TorrentRuleDraft(true, maximum = "99999999999999999999")).forEach { draft ->
            assertThrows(IllegalArgumentException::class.java) { draft.rules(video) }
        }
        assertThrows(IllegalArgumentException::class.java) { TorrentRuleDraft(true).rules(emptyList()) }
    }
}
