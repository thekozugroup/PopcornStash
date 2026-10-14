package com.thekozugroup.popcornstash.feature.library

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.core.content.FileProvider
import com.thekozugroup.popcornstash.data.api.Item
import com.thekozugroup.popcornstash.data.api.MediaKind
import com.thekozugroup.popcornstash.data.repo.Loadable
import com.thekozugroup.popcornstash.ui.components.EmptyState
import com.thekozugroup.popcornstash.ui.components.ListRow
import com.thekozugroup.popcornstash.ui.components.LoadFailed
import com.thekozugroup.popcornstash.ui.components.PageTopBar
import com.thekozugroup.popcornstash.ui.components.PlaceholderLine
import com.thekozugroup.popcornstash.ui.icons.LucideGlyph
import com.thekozugroup.popcornstash.ui.icons.LucideIcon
import com.thekozugroup.popcornstash.ui.icons.PopcornIcons
import com.thekozugroup.popcornstash.ui.theme.Popcorn
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.filter

/** What Other files does when tapped. */
class OtherFilesActions(
    val back: () -> Unit = {},
    val retry: () -> Unit = {},
    val loadMore: () -> Unit = {},
    val sort: (ItemSort) -> Unit = {},
    val open: (Item) -> Unit = {},
)

/**
 * Other files (android/DESIGN.md, A library): music, documents and anything else, as rows
 * under the web's sentence about who sees them. Tapping a row opens the file in another app.
 */
@Composable
fun OtherFilesScreen(
    ui: OtherFilesViewModel.Ui,
    isOwner: Boolean,
    bottomClearance: Dp,
    actions: OtherFilesActions,
    modifier: Modifier = Modifier,
    format: MediaFormat = MediaFormat(),
    listState: LazyListState = rememberLazyListState(),
) {
    val files = ui.files
    Column(modifier.fillMaxSize()) {
        PageTopBar(
            title = "Other files",
            onBack = actions.back,
            scrolled = listState.canScrollBackward,
            actions = { SortAction(ui.sort, ItemSort.otherFiles, actions.sort) },
        )
        val page = files.valueOrNull
        when {
            page == null && files is Loadable.Failed -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                LoadFailed(files.message, actions.retry)
            }
            page == null -> RowsPlaceholder()
            page.items.isEmpty() -> EmptyState(
                glyph = PopcornIcons.OtherFiles,
                title = "Nothing in Other files yet",
                body = "Music, documents and other files you share to PopcornStash are kept here.",
                modifier = Modifier.fillMaxSize().padding(bottom = 96.dp),
            )
            else -> {
                val loadMore by rememberUpdatedState(actions.loadMore)
                LaunchedEffect(listState) {
                    snapshotFlow {
                        val info = listState.layoutInfo
                        (info.visibleItemsInfo.lastOrNull()?.index ?: 0) >= info.totalItemsCount - 8
                    }.distinctUntilChanged().filter { it }.collect { loadMore() }
                }
                LazyColumn(
                    state = listState,
                    contentPadding = PaddingValues(bottom = bottomClearance),
                    modifier = Modifier.fillMaxSize().testTag("other files"),
                ) {
                    item(key = "who") {
                        Text(
                            if (isOwner) {
                                "Music, documents and anything else. Each file is seen by the person who added it and the owners."
                            } else {
                                "Music, documents and anything else. Only you and the owners see these."
                            },
                            style = Popcorn.type.body,
                            color = Popcorn.colors.textMuted,
                            modifier = Modifier.padding(start = Popcorn.spacing.gutter, end = Popcorn.spacing.gutter, top = Popcorn.spacing.xxs, bottom = Popcorn.spacing.m),
                        )
                    }
                    itemsIndexed(page.items, key = { _, it -> it.id }) { index, item ->
                        if (index > 0) {
                            HorizontalDivider(
                                thickness = 1.dp,
                                color = Popcorn.colors.divider,
                                modifier = Modifier.padding(start = Popcorn.spacing.gutter + Popcorn.media.rowGlyph + Popcorn.spacing.m),
                            )
                        }
                        FileRow(item, opening = ui.opening == item.id, format = format) { actions.open(item) }
                    }
                    page.moreFailed?.let { message -> item(key = "more failed") { LoadFailed(message, actions.loadMore) } }
                }
            }
        }
    }
}

@Composable
private fun FileRow(item: Item, opening: Boolean, format: MediaFormat, onOpen: () -> Unit) {
    val name = MediaFormat.fileName(item.relPath).ifBlank { item.title }
    ListRow(
        title = name,
        supporting = format.fileLine(item),
        onClick = onOpen,
        onClickLabel = "Open in another app",
        leading = { GlyphSquare(fileGlyph(item)) },
        trailing = {
            Box(Modifier.size(24.dp), contentAlignment = Alignment.Center) {
                if (opening) {
                    CircularProgressIndicator(
                        color = Popcorn.colors.textMuted,
                        strokeWidth = 2.dp,
                        modifier = Modifier.size(20.dp).clearAndSetSemantics { contentDescription = "Opening" },
                    )
                }
            }
        },
    )
}

/** A glyph on a 40dp `subtle` square with 8dp corners, the leading picture of a row. */
@Composable
fun GlyphSquare(glyph: LucideGlyph) {
    Box(
        Modifier
            .size(Popcorn.media.rowGlyph)
            .background(Popcorn.colors.subtle, Popcorn.shapes.control),
        contentAlignment = Alignment.Center,
    ) {
        LucideIcon(glyph, contentDescription = null, tint = Popcorn.colors.textMuted, size = 22.dp)
    }
}

/** Music, an archive, or a document (the web's choice of glyph). */
fun fileGlyph(item: Item): LucideGlyph = when {
    item.kind == MediaKind.Audio -> PopcornIcons.Audio
    MediaFormat.fileType(item.relPath) in ARCHIVES -> PopcornIcons.Archive
    else -> PopcornIcons.OtherFiles
}

private val ARCHIVES = setOf("ZIP", "RAR", "7Z", "TAR", "GZ", "ISO")

@Composable
private fun RowsPlaceholder() {
    Column(Modifier.padding(top = Popcorn.spacing.xs).clearAndSetSemantics { contentDescription = "Loading" }) {
        PlaceholderLine(Modifier.padding(horizontal = Popcorn.spacing.gutter).width(280.dp), height = 14.dp)
        Spacer(Modifier.height(Popcorn.spacing.l))
        repeat(6) {
            androidx.compose.foundation.layout.Row(
                Modifier.padding(horizontal = Popcorn.spacing.gutter, vertical = Popcorn.spacing.s),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Box(Modifier.size(Popcorn.media.rowGlyph).background(Popcorn.colors.subtle, Popcorn.shapes.control))
                Spacer(Modifier.width(Popcorn.spacing.m))
                Column {
                    PlaceholderLine(Modifier.width(180.dp))
                    Spacer(Modifier.height(8.dp))
                    PlaceholderLine(Modifier.width(90.dp))
                }
            }
        }
    }
}

/**
 * Hands a private copy to another app (docs/architecture.md section 10, Opening an Other
 * file): a FileProvider `content://` address with a one-time read grant, no write grant.
 * False when no app on the phone opens the type.
 */
fun openInAnotherApp(context: Context, opened: OpenedFile): Boolean {
    val uri = FileProvider.getUriForFile(context, "${context.packageName}.files", opened.file, opened.name)
    val intent = Intent(Intent.ACTION_VIEW)
        .setDataAndType(uri, opened.mime ?: context.contentResolver.getType(uri) ?: "application/octet-stream")
        .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    return try {
        context.startActivity(intent)
        true
    } catch (_: ActivityNotFoundException) {
        false
    }
}
