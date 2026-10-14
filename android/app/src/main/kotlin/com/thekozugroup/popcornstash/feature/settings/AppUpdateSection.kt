package com.thekozugroup.popcornstash.feature.settings

import android.content.ActivityNotFoundException
import android.content.Intent
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.core.net.toUri
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import com.thekozugroup.popcornstash.feature.downloads.ThinProgress
import com.thekozugroup.popcornstash.ui.components.ListRow
import com.thekozugroup.popcornstash.ui.components.PrimaryButton
import com.thekozugroup.popcornstash.ui.components.RowDivider
import com.thekozugroup.popcornstash.ui.components.SecondaryButton
import com.thekozugroup.popcornstash.ui.components.SectionHeading
import com.thekozugroup.popcornstash.ui.components.SwitchRow
import com.thekozugroup.popcornstash.ui.icons.LucideIcon
import com.thekozugroup.popcornstash.ui.icons.PopcornIcons
import com.thekozugroup.popcornstash.ui.theme.Popcorn
import com.thekozugroup.popcornstash.update.AppUpdateUi
import com.thekozugroup.popcornstash.update.AppUpdates

/** "This app" on the Settings page: the version, its updates and the daily check. [now] dates "checked 2 minutes ago". */
data class AppUpdateSectionState(val ui: AppUpdateUi, val now: Long)

/**
 * The updater's state for the Settings page, and what its buttons do: checks, downloads,
 * installs, the system page that lets PopcornStash install apps, and the release page in the
 * browser. Coming back to the page carries on an update that waited for that permission.
 */
@Composable
fun rememberAppUpdateSection(updates: AppUpdates): Pair<AppUpdateSectionState, AppUpdateActions> {
    val context = LocalContext.current
    val ui by updates.state.collectAsStateWithLifecycle()
    var resumed by remember { mutableIntStateOf(0) }
    LifecycleResumeEffect(updates) {
        resumed++
        updates.onResume()
        onPauseOrDispose { }
    }
    // "Checked 2 minutes ago" is dated when the state changes and whenever the page comes back.
    val now = remember(ui, resumed) { System.currentTimeMillis() }
    val actions = remember(updates, context) {
        fun start(intent: Intent) {
            try {
                context.startActivity(intent)
            } catch (_: ActivityNotFoundException) {
                // No settings page or browser for it on this phone: the page keeps its words.
            }
        }
        AppUpdateActions(
            onAction = { action ->
                when (action) {
                    UpdateAction.Check, UpdateAction.CheckAgain -> updates.checkNow()
                    UpdateAction.Update, UpdateAction.UpdateAgain -> if (!updates.update()) start(updates.permissionIntent())
                    UpdateAction.AllowInstalls -> start(updates.permissionIntent())
                    UpdateAction.Install -> if (!updates.install()) start(updates.permissionIntent())
                    UpdateAction.OpenReleasePage -> start(
                        Intent(Intent.ACTION_VIEW, updates.releasePage().toUri()).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
                    )
                }
            },
            onAutoCheck = updates::setAutoCheck,
        )
    }
    return AppUpdateSectionState(ui, now) to actions
}

/** What the buttons of "This app" do. */
class AppUpdateActions(
    val onAction: (UpdateAction) -> Unit = {},
    val onAutoCheck: (Boolean) -> Unit = {},
)

/**
 * "This app" (android/DESIGN.md, Settings): the app and its version on a row like the server's,
 * then where its updates stand in one sentence with at most one action (Update, Install or
 * Allow installs as the page's primary button; Check for updates as a secondary one), a bar
 * while it downloads and checks, and the switch for the daily check.
 */
fun LazyListScope.appUpdateSection(state: AppUpdateSectionState, actions: AppUpdateActions) {
    val ui = state.ui
    item(key = "app heading") {
        SectionHeading("This app")
        ListRow(
            title = "PopcornStash for Android",
            supporting = versionLine(ui),
            leading = { LucideIcon(PopcornIcons.Phone, contentDescription = null, tint = Popcorn.colors.textMuted) },
        )
    }
    item(key = "app update") {
        UpdateStatusBlock(updateWords(ui.status, state.now), actions.onAction)
    }
    item(key = "app auto check") {
        RowDivider(inset = false)
        SwitchRow(
            label = "Check for updates automatically",
            supporting = "Asks GitHub once a day for a newer build.",
            checked = ui.autoCheck,
            onCheckedChange = actions.onAutoCheck,
            modifier = Modifier.testTag("auto check"),
        )
    }
}

@Composable
private fun UpdateStatusBlock(words: UpdateWords, onAction: (UpdateAction) -> Unit) {
    val colors = Popcorn.colors
    Column(
        Modifier
            .fillMaxWidth()
            .padding(start = Popcorn.spacing.gutter, end = Popcorn.spacing.gutter, bottom = Popcorn.spacing.m),
    ) {
        // Read out as it changes: "Downloading build 415, 41%", then "Build 415 is ready to install."
        Column(Modifier.semantics(mergeDescendants = true) { liveRegion = LiveRegionMode.Polite }.testTag("update status")) {
            Text(words.line, style = Popcorn.type.body, color = colors.text)
            if (words.detail != null) {
                Spacer(Modifier.height(Popcorn.spacing.xxs))
                Text(words.detail, style = Popcorn.type.metadata, color = colors.textMuted)
            }
        }
        words.progress?.let { progress ->
            Spacer(Modifier.height(Popcorn.spacing.s))
            // The downloads' bar: Action Blue on `progressTrack`, filling as it goes, never looping.
            ThinProgress(
                fraction = progress.percent?.let { it / 100.0 },
                moving = true,
                label = words.line,
                modifier = Modifier.testTag("update progress"),
            )
        }
        words.action?.let { action ->
            Spacer(Modifier.height(Popcorn.spacing.s))
            val modifier = Modifier.testTag("update action")
            if (action.primary) {
                PrimaryButton(action.label, onClick = { onAction(action) }, busy = words.busy, modifier = modifier)
            } else {
                SecondaryButton(action.label, onClick = { onAction(action) }, busy = words.busy, modifier = modifier)
            }
        }
    }
}
