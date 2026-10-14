package com.thekozugroup.popcornstash.ui.components

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import com.thekozugroup.popcornstash.navigation.TabPage

/**
 * A tab's first page: the top bar with the tab's title over a list that scrolls under
 * it and ends clear of the floating toolbar. Tapping the tab again scrolls to the top.
 * The list's scroll position survives switching tabs.
 */
@Composable
fun TabRootScaffold(
    page: TabPage,
    title: String,
    modifier: Modifier = Modifier,
    listState: LazyListState = rememberLazyListState(),
    actions: @Composable RowScope.() -> Unit = {},
    content: LazyListScope.() -> Unit,
) {
    var handledReselections by rememberSaveable { mutableIntStateOf(page.reselections) }
    LaunchedEffect(page.reselections) {
        if (page.reselections != handledReselections) {
            handledReselections = page.reselections
            listState.animateScrollToItem(0)
        }
    }
    Column(modifier.fillMaxSize()) {
        PageTopBar(title = title, scrolled = listState.canScrollBackward, actions = actions)
        LazyColumn(
            state = listState,
            contentPadding = PaddingValues(bottom = page.bottomClearance),
            modifier = Modifier.fillMaxSize().testTag("list ${page.tab.name}"),
            content = content,
        )
    }
}
