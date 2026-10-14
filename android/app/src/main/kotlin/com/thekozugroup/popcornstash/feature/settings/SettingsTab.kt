package com.thekozugroup.popcornstash.feature.settings

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.unit.dp
import com.thekozugroup.popcornstash.data.api.Role
import com.thekozugroup.popcornstash.data.api.User
import com.thekozugroup.popcornstash.navigation.TabPage
import com.thekozugroup.popcornstash.ui.components.ListRow
import com.thekozugroup.popcornstash.ui.components.QuietButton
import com.thekozugroup.popcornstash.ui.components.RowDivider
import com.thekozugroup.popcornstash.ui.components.SectionHeading
import com.thekozugroup.popcornstash.ui.components.TabRootScaffold
import com.thekozugroup.popcornstash.ui.icons.LucideIcon
import com.thekozugroup.popcornstash.ui.icons.PopcornIcons
import com.thekozugroup.popcornstash.ui.theme.Popcorn

/** A web settings page the app opens in a Custom Tab for now (docs/architecture.md section 10, Settings). */
data class WebSettingsPage(val label: String, val path: String)

/** The web settings pages a person can open from the app: every area for owners, their own devices and passkeys for members. */
fun webSettingsPages(role: Role): List<WebSettingsPage> = if (role == Role.Owner) {
    listOf(
        WebSettingsPage("Libraries", "settings/libraries"),
        WebSettingsPage("VPN", "settings/vpn"),
        WebSettingsPage("People", "settings/people"),
        WebSettingsPage("Devices", "settings/devices"),
        WebSettingsPage("Security", "settings/security"),
        WebSettingsPage("System", "settings/system"),
    )
} else {
    listOf(WebSettingsPage("Devices", "settings/devices"), WebSettingsPage("Security", "settings/security"))
}

/**
 * Settings, the right tab (android/DESIGN.md, Settings): the signed-in person, this server,
 * the web settings pages, and sign out.
 */
@Composable
fun SettingsTab(
    page: TabPage,
    user: User,
    serverDisplay: String,
    serverVersion: String?,
    onOpenWeb: (WebSettingsPage) -> Unit,
    onSignOut: () -> Unit,
) {
    var confirmSignOut by rememberSaveable { mutableStateOf(false) }
    TabRootScaffold(page = page, title = "Settings") {
        item(key = "you") {
            SectionHeading("You")
            ListRow(
                title = user.displayName.ifBlank { "Signed in" },
                supporting = if (user.role == Role.Owner) "Owner" else "Member",
                leading = { Avatar(user.displayName) },
            )
        }
        item(key = "server") {
            SectionHeading("This server")
            ListRow(
                title = serverDisplay,
                supporting = serverVersion?.let { "Version $it" },
                leading = { LucideIcon(PopcornIcons.Server, contentDescription = null, tint = Popcorn.colors.textMuted) },
            )
        }
        item(key = "web heading") { SectionHeading("On the web") }
        val pages = webSettingsPages(user.role)
        pages.forEachIndexed { index, web ->
            item(key = "web ${web.path}") {
                ListRow(
                    title = web.label,
                    onClick = { onOpenWeb(web) },
                    onClickLabel = "Open ${web.label} settings on the web",
                    trailing = { LucideIcon(PopcornIcons.OpenOnWeb, contentDescription = null, tint = Popcorn.colors.textMuted, size = 20.dp) },
                )
                if (index < pages.lastIndex) RowDivider(inset = false)
            }
        }
        item(key = "sign out") {
            Spacer(Modifier.height(Popcorn.spacing.xl))
            RowDivider(inset = false)
            ListRow(
                title = "Sign out",
                titleColor = Popcorn.colors.textMuted,
                onClick = { confirmSignOut = true },
                leading = { LucideIcon(PopcornIcons.SignOut, contentDescription = null, tint = Popcorn.colors.textMuted) },
            )
        }
    }
    if (confirmSignOut) {
        AlertDialog(
            onDismissRequest = { confirmSignOut = false },
            title = { Text("Sign out on this phone?") },
            text = { Text("PopcornStash forgets your sign-in and everything it kept on this phone.") },
            confirmButton = {
                QuietButton("Sign out", onClick = {
                    confirmSignOut = false
                    onSignOut()
                })
            },
            dismissButton = { QuietButton("Cancel", onClick = { confirmSignOut = false }) },
            containerColor = Popcorn.colors.surface,
            titleContentColor = Popcorn.colors.text,
            textContentColor = Popcorn.colors.textMuted,
        )
    }
}

/** The person's initials on a circle, 48dp. */
@Composable
private fun Avatar(name: String) {
    val initials = name.split(' ').filter { it.isNotBlank() }.take(2).joinToString("") { it.first().uppercase() }
    Box(
        Modifier
            .size(48.dp)
            .background(Popcorn.colors.subtle, Popcorn.shapes.avatar)
            .clearAndSetSemantics { },
        contentAlignment = Alignment.Center,
    ) {
        Text(initials.ifEmpty { "?" }, style = Popcorn.type.section, color = Popcorn.colors.text)
    }
}
