package com.thekozugroup.popcornstash.feature.settings

import android.Manifest
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import androidx.core.net.toUri
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
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

/**
 * "This app" on the Settings page: the version, its updates and the daily check. [now] dates
 * "checked 2 minutes ago"; [notificationsOff] when this app may not post notifications, so the
 * daily check's notification would not reach the person.
 */
data class AppUpdateSectionState(val ui: AppUpdateUi, val now: Long, val notificationsOff: Boolean = false)

/** Whether this app may post notifications (the permission on Android 13 and later, and the app's own switch). */
private fun notificationsAllowed(context: Context): Boolean = NotificationManagerCompat.from(context).areNotificationsEnabled()

/**
 * The updater's state for the Settings page, and what its buttons do: checks, downloads,
 * installs, the system page that lets PopcornStash install apps, Android's confirmation again,
 * and the release page in the browser. Coming back to the page carries on an update that waited
 * for that permission. On Android 13 and later it asks for permission to post notifications
 * when the person turns automatic checks on, and the first time they check or update.
 */
@Composable
fun rememberAppUpdateSection(updates: AppUpdates): Pair<AppUpdateSectionState, AppUpdateActions> {
    val context = LocalContext.current
    val ui by updates.state.collectAsStateWithLifecycle()
    var resumed by remember { mutableIntStateOf(0) }
    var notificationsOff by remember { mutableStateOf(!notificationsAllowed(context)) }
    val askNotifications = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {
        notificationsOff = !notificationsAllowed(context)
    }
    LifecycleResumeEffect(updates) {
        resumed++
        notificationsOff = !notificationsAllowed(context)
        updates.onResume()
        onPauseOrDispose { }
    }
    // "Checked 2 minutes ago" is dated when the state changes and whenever the page comes back.
    val now = remember(ui, resumed) { System.currentTimeMillis() }
    val actions = remember(updates, context, askNotifications) {
        fun start(intent: Intent) {
            try {
                context.startActivity(intent)
            } catch (_: ActivityNotFoundException) {
                // No settings page or browser for it on this phone: the page keeps its words.
            }
        }

        fun askForNotifications(turningOn: Boolean) {
            if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) return
            if (ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED) return
            if (updates.askForNotifications(turningOn)) askNotifications.launch(Manifest.permission.POST_NOTIFICATIONS)
        }
        AppUpdateActions(
            onAction = { action ->
                when (action) {
                    UpdateAction.Check, UpdateAction.CheckAgain -> {
                        askForNotifications(turningOn = false)
                        updates.checkNow()
                    }
                    // Android's page for installing apps comes first when it is needed; the question about notifications waits.
                    UpdateAction.Update, UpdateAction.UpdateAgain ->
                        if (updates.update()) askForNotifications(turningOn = false) else start(updates.permissionIntent())
                    UpdateAction.AllowInstalls -> start(updates.permissionIntent())
                    UpdateAction.Install -> if (!updates.install()) start(updates.permissionIntent())
                    UpdateAction.Confirm -> updates.confirmation()?.let(::start)
                    UpdateAction.OpenReleasePage -> start(
                        Intent(Intent.ACTION_VIEW, updates.releasePage().toUri()).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
                    )
                }
            },
            onAutoCheck = { enabled ->
                updates.setAutoCheck(enabled)
                if (enabled) askForNotifications(turningOn = true)
            },
        )
    }
    return AppUpdateSectionState(ui, now, notificationsOff) to actions
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
 * while it downloads (filling) and checks the download (moving), and the switch for the daily
 * check, whose line says when notifications are off.
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
            supporting = autoCheckLine(ui.autoCheck, state.notificationsOff),
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
            val modifier = Modifier.testTag("update progress")
            if (progress.working) {
                WorkingBar(words.line, modifier)
            } else {
                // The downloads' bar: Action Blue on `progressTrack`, filling as it goes, never looping.
                ThinProgress(fraction = progress.percent?.let { it / 100.0 }, moving = true, label = words.line, modifier = modifier)
            }
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

/**
 * A step with no measure, after the download's bar has filled (checking the download): the same
 * 4dp bar with Action Blue moving along it, so it never reads as finished. It lasts a moment.
 * Under reduce motion it stands still, full, in the waiting colour, and the sentence says what runs.
 */
@Composable
private fun WorkingBar(label: String, modifier: Modifier) {
    if (Popcorn.reduceMotion) {
        ThinProgress(fraction = 1.0, moving = false, label = label, modifier = modifier)
        return
    }
    val colors = Popcorn.colors
    LinearProgressIndicator(
        modifier = modifier
            .fillMaxWidth()
            .height(Popcorn.media.progressHeight)
            .clip(Popcorn.shapes.tag)
            .semantics { contentDescription = label },
        color = colors.accent,
        trackColor = colors.progressTrack,
        strokeCap = StrokeCap.Butt,
        gapSize = 0.dp,
    )
}
