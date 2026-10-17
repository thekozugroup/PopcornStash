package com.thekozugroup.popcornstash.screenshots

import androidx.compose.runtime.Composable
import com.thekozugroup.popcornstash.data.api.Role
import com.thekozugroup.popcornstash.data.api.User
import com.thekozugroup.popcornstash.data.repo.Loadable
import com.thekozugroup.popcornstash.feature.downloads.DownloadsActions
import com.thekozugroup.popcornstash.feature.downloads.DownloadsPage
import com.thekozugroup.popcornstash.feature.downloads.DownloadsSamples
import com.thekozugroup.popcornstash.feature.home.HomeActions
import com.thekozugroup.popcornstash.feature.home.HomeScreen
import com.thekozugroup.popcornstash.feature.library.MediaFormat
import com.thekozugroup.popcornstash.feature.settings.LibrarySettingsUi
import com.thekozugroup.popcornstash.feature.settings.SettingsFixtures
import com.thekozugroup.popcornstash.feature.settings.SettingsRootPage
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

/** The owner's libraries, people and folders as Settings lists them. */
val SampleLibrarySettings = LibrarySettingsUi(
    libraries = Loadable.Ready(SettingsFixtures.libraries()),
    people = SettingsFixtures.people(),
    folders = SettingsFixtures.folders(),
)

/** The tab roots as the signed-in app shows them, with sample data. */
@Composable
fun SampleTabRoot(page: TabPage, user: User = SampleOwner) {
    when (page.tab) {
        // As the app wires it: a finished row's library opens (MainActivity).
        Tab.Downloads -> DownloadsPage(page, DownloadsSamples.content(), DownloadsActions(onOpenPlace = {}, onManageInbox = {}), DownloadsSamples.NOW)
        Tab.Search -> androidx.compose.material3.Text("Search")
        Tab.Home -> WithSampleImages {
            HomeScreen(page, Loadable.Ready(SampleMedia.home), isOwner = user.role == Role.Owner, actions = HomeActions(), format = SampleFormat)
        }
        // As the app shows it: an owner manages the libraries here, a member does not.
        Tab.Settings -> SettingsRootPage(
            page = page,
            user = user,
            serverDisplay = "stash.example.com",
            serverVersion = "0.2.0",
            libraries = if (user.role == Role.Owner) SampleLibrarySettings else null,
            onOpenWeb = {},
            onSignOut = {},
        )
    }
}
