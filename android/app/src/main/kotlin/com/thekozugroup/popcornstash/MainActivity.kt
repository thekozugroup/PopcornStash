package com.thekozugroup.popcornstash

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.Composable
import com.thekozugroup.popcornstash.feature.downloads.DownloadsTab
import com.thekozugroup.popcornstash.feature.home.HomeTab
import com.thekozugroup.popcornstash.feature.settings.SettingsTab
import com.thekozugroup.popcornstash.navigation.AppShell
import com.thekozugroup.popcornstash.navigation.Tab
import com.thekozugroup.popcornstash.navigation.TabPage
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        setContent {
            PopcornTheme {
                AppShell(page = { TabRoot(it) })
            }
        }
    }
}

/** The first page of each tab. */
@Composable
fun TabRoot(page: TabPage) {
    when (page.tab) {
        Tab.Downloads -> DownloadsTab(page)
        Tab.Home -> HomeTab(page)
        Tab.Settings -> SettingsTab(page)
    }
}
