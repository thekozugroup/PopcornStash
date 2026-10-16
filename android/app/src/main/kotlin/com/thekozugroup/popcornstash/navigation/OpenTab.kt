package com.thekozugroup.popcornstash.navigation

import android.content.Context
import android.content.Intent
import com.thekozugroup.popcornstash.MainActivity

/**
 * Opening the app on a given tab from outside it: the upload notification and the share
 * sheet open Downloads. MainActivity reads [EXTRA_OPEN_TAB] in onCreate and onNewIntent and
 * hands the tab to [AppShell]'s `openTab`.
 */
object OpenTab {
    const val EXTRA_OPEN_TAB = "com.thekozugroup.popcornstash.extra.OPEN_TAB"

    /** An intent that brings the app forward on [tab] (a running app keeps its pages). */
    fun intent(context: Context, tab: Tab): Intent =
        Intent(context, MainActivity::class.java)
            .putExtra(EXTRA_OPEN_TAB, tab.name)
            .addFlags(Intent.FLAG_ACTIVITY_NO_USER_ACTION or Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP)

    /** The tab an intent asks for, if any. */
    fun from(intent: Intent?): Tab? =
        intent?.getStringExtra(EXTRA_OPEN_TAB)?.let { name -> Tab.entries.firstOrNull { it.name == name } }
}
