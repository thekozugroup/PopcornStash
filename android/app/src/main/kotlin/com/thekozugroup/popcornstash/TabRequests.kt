package com.thekozugroup.popcornstash

import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import com.thekozugroup.popcornstash.feature.home.HomeRoute
import com.thekozugroup.popcornstash.feature.settings.SettingsRoute
import com.thekozugroup.popcornstash.navigation.Tab

/**
 * One tab asking for a page of another: a finished download opens its library in Home, and
 * Home's "Edit library" and "Make a library" open the library editor in Settings. The shell
 * selects [tab] and calls [tabShown]; the tab opens its page once and calls [homeOpened] or
 * [settingsOpened]. Notifications and the share sheet ask for a tab alone ([showTab]).
 */
@Stable
class TabRequests {
    var tab by mutableStateOf<Tab?>(null)
        private set
    var home by mutableStateOf<HomeRoute?>(null)
        private set
    var settings by mutableStateOf<SettingsRoute?>(null)
        private set

    fun showTab(tab: Tab) {
        this.tab = tab
    }

    fun openHome(route: HomeRoute) {
        home = route
        tab = Tab.Home
    }

    fun openSettings(route: SettingsRoute) {
        settings = route
        tab = Tab.Settings
    }

    fun tabShown() {
        tab = null
    }

    fun homeOpened() {
        home = null
    }

    fun settingsOpened() {
        settings = null
    }
}
