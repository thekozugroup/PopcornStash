package com.thekozugroup.popcornstash.feature.downloads

import androidx.compose.foundation.layout.wrapContentHeight
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import com.thekozugroup.popcornstash.navigation.TabPage
import com.thekozugroup.popcornstash.ui.components.EmptyState
import com.thekozugroup.popcornstash.ui.components.TabRootScaffold
import com.thekozugroup.popcornstash.ui.icons.PopcornIcons

/** Downloads, the left tab: what is downloading, and finished files to put into a library. */
@Composable
fun DownloadsTab(page: TabPage) {
    TabRootScaffold(page = page, title = "Downloads") {
        item(key = "empty") {
            EmptyState(
                glyph = PopcornIcons.Downloads,
                title = "Nothing downloading",
                body = "Links and magnets you add, and files you share to PopcornStash, show up here.",
                modifier = Modifier.fillParentMaxHeight(0.75f).wrapContentHeight(),
            )
        }
    }
}
