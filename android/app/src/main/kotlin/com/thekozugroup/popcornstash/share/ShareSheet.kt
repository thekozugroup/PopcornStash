package com.thekozugroup.popcornstash.share

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.thekozugroup.popcornstash.feature.downloads.breakableName
import com.thekozugroup.popcornstash.feature.downloads.breakableUrl
import com.thekozugroup.popcornstash.feature.downloads.describeLink
import com.thekozugroup.popcornstash.feature.downloads.displayName
import com.thekozugroup.popcornstash.feature.downloads.formatBytes
import com.thekozugroup.popcornstash.ui.components.PrimaryButton
import com.thekozugroup.popcornstash.ui.components.QuietButton
import com.thekozugroup.popcornstash.ui.components.SecondaryButton
import com.thekozugroup.popcornstash.ui.icons.LucideGlyph
import com.thekozugroup.popcornstash.ui.icons.LucideGlyphs
import com.thekozugroup.popcornstash.ui.icons.LucideIcon
import com.thekozugroup.popcornstash.ui.icons.PopcornIcons
import com.thekozugroup.popcornstash.ui.theme.Popcorn

/** What the sheet's buttons do. */
class ShareActions(
    val onAddLink: () -> Unit = {},
    val onUpload: () -> Unit = {},
    val onOpenApp: () -> Unit = {},
    val onOpenDownloads: () -> Unit = {},
    val onClose: () -> Unit = {},
)

/** The share sheet over the app that shared: a Material bottom sheet with [ShareSheetContent]. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ShareSheet(state: ShareSheetState, actions: ShareActions) {
    ModalBottomSheet(
        onDismissRequest = actions.onClose,
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
        containerColor = Popcorn.colors.surface,
        contentColor = Popcorn.colors.text,
        shape = Popcorn.shapes.sheet,
        scrimColor = Popcorn.colors.stage.copy(alpha = 0.4f),
    ) {
        ShareSheetContent(state, actions)
    }
}

/** The sheet's content for each state (android/DESIGN.md, Share sheet). */
@Composable
fun ShareSheetContent(state: ShareSheetState, actions: ShareActions, modifier: Modifier = Modifier) {
    Column(
        modifier
            .fillMaxWidth()
            .windowInsetsPadding(WindowInsets.navigationBars)
            .padding(start = Popcorn.spacing.gutter, end = Popcorn.spacing.gutter, top = Popcorn.spacing.xs, bottom = Popcorn.spacing.m)
            .testTag("share sheet"),
    ) {
        when (state) {
            ShareSheetState.Checking -> Checking()
            ShareSheetState.SignedOut -> {
                Title("Sign in to PopcornStash first.")
                Sentence("Then share again, and it goes to your stash.")
                Buttons {
                    PrimaryButton("Open PopcornStash", onClick = actions.onOpenApp, modifier = Modifier.fillMaxWidth())
                    QuietButton("Cancel", onClick = actions.onClose, modifier = Modifier.fillMaxWidth())
                }
            }
            is ShareSheetState.Link -> LinkContent(state, actions)
            is ShareSheetState.Files -> FilesContent(state, actions)
            is ShareSheetState.Unusable -> {
                Title("Nothing here PopcornStash can take")
                Sentence(
                    if (state.refused > 0) {
                        "PopcornStash takes files only when another app shares them with it. Share them from the gallery or a files app."
                    } else {
                        "Share a web link, a magnet link, or photos, videos and files."
                    },
                )
                Buttons { SecondaryButton("Close", onClick = actions.onClose, modifier = Modifier.fillMaxWidth()) }
            }
        }
    }
}

@Composable
private fun ColumnScope.LinkContent(state: ShareSheetState.Link, actions: ShareActions) {
    if (state.addedName != null) {
        Title("Added to Downloads")
        Text(
            breakableName(state.addedName),
            style = Popcorn.type.item,
            color = Popcorn.colors.text,
            modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite },
        )
        Spacer(Modifier.height(Popcorn.spacing.xxs))
        Sentence("It shows under In progress, then goes to the library that takes it.")
        Buttons {
            PrimaryButton("Done", onClick = actions.onClose, modifier = Modifier.fillMaxWidth())
            QuietButton("Open Downloads", onClick = actions.onOpenDownloads, modifier = Modifier.fillMaxWidth())
        }
        return
    }
    Title("Add to Downloads?")
    Row(verticalAlignment = Alignment.Top) {
        GlyphTile(PopcornIcons.Link)
        Spacer(Modifier.width(Popcorn.spacing.s))
        Column(Modifier.weight(1f)) {
            Text(
                breakableName(state.link.displayName()),
                style = Popcorn.type.item,
                color = Popcorn.colors.text,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                breakableUrl(state.link.url),
                style = Popcorn.type.metadata,
                color = Popcorn.colors.textMuted,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.testTag("shared link"),
            )
        }
    }
    Spacer(Modifier.height(Popcorn.spacing.s))
    Sentence(describeLink(state.link, state.viaVpn))
    if (state.error != null) {
        Spacer(Modifier.height(Popcorn.spacing.xs))
        ErrorSentence(state.error)
    }
    Buttons {
        PrimaryButton("Add to Downloads", onClick = actions.onAddLink, busy = state.busy, modifier = Modifier.fillMaxWidth())
        QuietButton("Cancel", onClick = actions.onClose, enabled = !state.busy, modifier = Modifier.fillMaxWidth())
    }
}

@Composable
private fun ColumnScope.FilesContent(state: ShareSheetState.Files, actions: ShareActions) {
    val count = state.files.size
    Title(if (count == 1) "Upload this file?" else "Upload $count files?")
    state.files.take(SHOWN_FILES).forEach { file ->
        Row(
            Modifier
                .fillMaxWidth()
                .heightIn(min = 48.dp)
                .semantics(mergeDescendants = true) { },
            verticalAlignment = Alignment.CenterVertically,
        ) {
            GlyphTile(glyphFor(file.mime))
            Spacer(Modifier.width(Popcorn.spacing.s))
            Text(
                file.name,
                style = Popcorn.type.item,
                color = Popcorn.colors.text,
                maxLines = 1,
                overflow = TextOverflow.MiddleEllipsis,
                modifier = Modifier.weight(1f),
            )
            file.size?.let {
                Spacer(Modifier.width(Popcorn.spacing.s))
                Text(formatBytes(it), style = Popcorn.type.metadata, color = Popcorn.colors.textMuted)
            }
        }
    }
    if (count > SHOWN_FILES) {
        Text(
            "and ${count - SHOWN_FILES} more",
            style = Popcorn.type.metadata,
            color = Popcorn.colors.textMuted,
            modifier = Modifier.padding(start = 40.dp + Popcorn.spacing.s),
        )
    }
    Spacer(Modifier.height(Popcorn.spacing.s))
    Sentence("Uploads as ${state.userName} to ${state.server}.")
    if (state.refused > 0) {
        Spacer(Modifier.height(Popcorn.spacing.xxs))
        Text(
            if (state.refused == 1) {
                "1 file stays behind: PopcornStash takes files only when another app shares them with it."
            } else {
                "${state.refused} files stay behind: PopcornStash takes files only when another app shares them with it."
            },
            style = Popcorn.type.metadata,
            color = Popcorn.colors.textMuted,
        )
    }
    Buttons {
        PrimaryButton("Upload", onClick = actions.onUpload, busy = state.busy, modifier = Modifier.fillMaxWidth())
        QuietButton("Cancel", onClick = actions.onClose, enabled = !state.busy, modifier = Modifier.fillMaxWidth())
    }
}

private const val SHOWN_FILES = 3

private fun glyphFor(mime: String?): LucideGlyph = when {
    mime == null -> LucideGlyphs.FileText
    mime.startsWith("image/") -> LucideGlyphs.Image
    mime.startsWith("video/") -> LucideGlyphs.Film
    else -> LucideGlyphs.FileText
}

/** A 40dp square with the glyph, like a library icon in a list. */
@Composable
private fun GlyphTile(glyph: LucideGlyph) {
    Box(
        Modifier
            .size(40.dp)
            .background(Popcorn.colors.subtle, Popcorn.shapes.control)
            .clearAndSetSemantics { },
        contentAlignment = Alignment.Center,
    ) {
        LucideIcon(glyph, contentDescription = null, tint = Popcorn.colors.textMuted, size = 20.dp)
    }
}

@Composable
private fun Title(text: String) {
    Text(
        text,
        style = Popcorn.type.title,
        color = Popcorn.colors.text,
        modifier = Modifier
            .padding(bottom = Popcorn.spacing.m)
            .semantics { heading() },
    )
}

@Composable
private fun Sentence(text: String) {
    Text(text, style = Popcorn.type.body, color = Popcorn.colors.textMuted)
}

@Composable
private fun ErrorSentence(text: String) {
    Text(
        text,
        style = Popcorn.type.body,
        color = Popcorn.colors.error,
        modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite },
    )
}

@Composable
private fun Buttons(content: @Composable ColumnScope.() -> Unit) {
    Spacer(Modifier.height(Popcorn.spacing.l))
    Column(verticalArrangement = Arrangement.spacedBy(Popcorn.spacing.xxs), content = content)
}

/** While the session or the files are read: the sheet's height in quiet placeholder bars. */
@Composable
private fun Checking() {
    Column(
        Modifier
            .fillMaxWidth()
            .semantics { contentDescription = "Loading" },
    ) {
        Box(Modifier.width(180.dp).height(20.dp).background(Popcorn.colors.subtle, Popcorn.shapes.tag))
        Spacer(Modifier.height(Popcorn.spacing.l))
        Box(Modifier.fillMaxWidth(0.8f).height(14.dp).background(Popcorn.colors.subtle, Popcorn.shapes.tag))
        Spacer(Modifier.height(Popcorn.spacing.xs))
        Box(Modifier.fillMaxWidth(0.5f).height(14.dp).background(Popcorn.colors.subtle, Popcorn.shapes.tag))
        Spacer(Modifier.height(Popcorn.spacing.l))
        Box(Modifier.fillMaxWidth().height(48.dp).background(Popcorn.colors.subtle, Popcorn.shapes.control))
    }
}
