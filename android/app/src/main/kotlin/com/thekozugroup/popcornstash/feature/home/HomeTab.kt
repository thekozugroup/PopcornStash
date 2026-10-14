package com.thekozugroup.popcornstash.feature.home

import androidx.compose.foundation.layout.wrapContentHeight
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import com.thekozugroup.popcornstash.navigation.TabPage
import com.thekozugroup.popcornstash.ui.components.EmptyState
import com.thekozugroup.popcornstash.ui.components.TabRootScaffold
import com.thekozugroup.popcornstash.ui.icons.PopcornIcons

/** Home, the start tab: continue watching, then every library the person can see. */
@Composable
fun HomeTab(page: TabPage) {
    TabRootScaffold(page = page, title = "Home") {
        item(key = "placeholder") {
            EmptyState(
                glyph = PopcornIcons.Home,
                title = "Your libraries",
                body = "What you were watching and every library shared with you will be here.",
                modifier = Modifier.fillParentMaxHeight(0.75f).wrapContentHeight(),
            )
        }
    }
}
