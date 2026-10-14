package com.thekozugroup.popcornstash.screenshots

import androidx.compose.runtime.Composable
import com.thekozugroup.popcornstash.data.api.Role
import com.thekozugroup.popcornstash.data.api.User
import com.thekozugroup.popcornstash.data.repo.Loadable
import com.thekozugroup.popcornstash.feature.downloads.DownloadsTab
import com.thekozugroup.popcornstash.feature.home.HomeActions
import com.thekozugroup.popcornstash.feature.home.HomeScreen
import com.thekozugroup.popcornstash.feature.library.MediaFormat
import com.thekozugroup.popcornstash.feature.settings.SettingsTab
import com.thekozugroup.popcornstash.navigation.Tab
import com.thekozugroup.popcornstash.navigation.TabPage
import com.thekozugroup.popcornstash.testing.SampleMedia
import com.thekozugroup.popcornstash.testing.WithSampleImages
import java.time.ZoneId
import java.util.Locale

/** An owner as the screenshots show them (an invented person). */
val SampleOwner = User(id = "u-owner", displayName = "Ana Ribeiro", role = Role.Owner)

/** Dates and numbers as the screenshots show them, whatever the machine's settings. */
val SampleFormat = MediaFormat(Locale.US, ZoneId.of("America/New_York"))

/** The tab roots as the signed-in app shows them, with sample data. */
@Composable
fun SampleTabRoot(page: TabPage, user: User = SampleOwner) {
    when (page.tab) {
        Tab.Downloads -> DownloadsTab(page)
        Tab.Home -> WithSampleImages {
            HomeScreen(page, Loadable.Ready(SampleMedia.home), isOwner = user.role == Role.Owner, actions = HomeActions(), format = SampleFormat)
        }
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
