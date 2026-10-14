package com.thekozugroup.popcornstash.feature.settings

import androidx.compose.foundation.layout.wrapContentHeight
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import com.thekozugroup.popcornstash.navigation.TabPage
import com.thekozugroup.popcornstash.ui.components.EmptyState
import com.thekozugroup.popcornstash.ui.components.TabRootScaffold
import com.thekozugroup.popcornstash.ui.icons.PopcornIcons

/** Settings, the right tab: the signed-in person, this server, libraries for the owner, sign out. */
@Composable
fun SettingsTab(page: TabPage) {
    TabRootScaffold(page = page, title = "Settings") {
        item(key = "placeholder") {
            EmptyState(
                glyph = PopcornIcons.Settings,
                title = "Settings",
                body = "Your account, this server and the libraries you manage will be here.",
                modifier = Modifier.fillParentMaxHeight(0.75f).wrapContentHeight(),
            )
        }
    }
}
