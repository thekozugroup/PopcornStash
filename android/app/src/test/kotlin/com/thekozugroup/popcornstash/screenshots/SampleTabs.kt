package com.thekozugroup.popcornstash.screenshots

import androidx.compose.runtime.Composable
import com.thekozugroup.popcornstash.data.api.Role
import com.thekozugroup.popcornstash.data.api.User
import com.thekozugroup.popcornstash.feature.downloads.DownloadsActions
import com.thekozugroup.popcornstash.feature.downloads.DownloadsPage
import com.thekozugroup.popcornstash.feature.downloads.DownloadsSamples
import com.thekozugroup.popcornstash.feature.home.HomeTab
import com.thekozugroup.popcornstash.feature.settings.SettingsTab
import com.thekozugroup.popcornstash.navigation.Tab
import com.thekozugroup.popcornstash.navigation.TabPage

/** An owner as the screenshots show them (an invented person). */
val SampleOwner = User(id = "u-owner", displayName = "Ana Ribeiro", role = Role.Owner)

/** The tab roots as the signed-in app shows them, with sample data. */
@Composable
fun SampleTabRoot(page: TabPage, user: User = SampleOwner) {
    when (page.tab) {
        Tab.Downloads -> DownloadsPage(page, DownloadsSamples.content(), DownloadsActions(), DownloadsSamples.NOW)
        Tab.Home -> HomeTab(page)
        Tab.Settings -> SettingsTab(
            page = page,
            user = user,
            serverDisplay = "stash.example.com",
            serverVersion = "0.2.0",
            onOpenWeb = {},
            onSignOut = {},
        )
    }
}
