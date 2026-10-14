package com.thekozugroup.popcornstash.share

import android.content.ClipData
import android.content.ContentProvider
import android.content.ContentValues
import android.content.Intent
import android.database.Cursor
import android.database.MatrixCursor
import android.net.Uri
import android.os.ParcelFileDescriptor
import android.provider.OpenableColumns
import androidx.test.core.app.ApplicationProvider
import com.thekozugroup.popcornstash.feature.downloads.LinkKind
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import java.io.File

/** What another app shares, and which files PopcornStash may read. */
@RunWith(RobolectricTestRunner::class)
class ShareRequestTest {
    private val context = ApplicationProvider.getApplicationContext<android.content.Context>()
    private val anyContent: (Uri) -> Boolean = { it.scheme == "content" }

    @Test
    fun aSharedPageGivesItsLink() {
        val intent = Intent(Intent.ACTION_SEND).setType("text/plain")
            .putExtra(Intent.EXTRA_TEXT, "Harbor Lights trailer https://media.example.org/harbor-lights-trailer.mp4")
        val request = parseShare(intent, anyContent) as ShareRequest.Link
        assertEquals(LinkKind.Http, request.link.kind)
        assertEquals("https://media.example.org/harbor-lights-trailer.mp4", request.link.url)
    }

    @Test
    fun aTextShareIgnoresThePreviewPictureInClipData() {
        val intent = Intent(Intent.ACTION_SEND).setType("text/plain")
            .putExtra(Intent.EXTRA_TEXT, "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567")
        intent.clipData = ClipData.newRawUri("preview", Uri.parse("content://browser/preview.png"))
        assertTrue(parseShare(intent, anyContent) is ShareRequest.Link)
    }

    @Test
    fun textWithoutALinkIsNothingToAdd() {
        val intent = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, "See you at six")
        assertEquals(ShareRequest.Unusable(0), parseShare(intent, anyContent))
    }

    @Test
    fun severalFilesAreTakenOnlyAsContentUris() {
        val photos = arrayListOf(
            Uri.parse("content://media/external/images/1"),
            Uri.parse("file:///data/data/com.thekozugroup.popcornstash/no_backup/session"),
            Uri.parse("content://media/external/images/2"),
        )
        val intent = Intent(Intent.ACTION_SEND_MULTIPLE).setType("image/*").putParcelableArrayListExtra(Intent.EXTRA_STREAM, photos)
        val request = parseShare(intent, anyContent) as ShareRequest.Files
        assertEquals(listOf(photos[0], photos[2]), request.uris)
        assertEquals(1, request.refused)
    }

    @Test
    fun onlyRefusedFilesAreNothingToUpload() {
        val intent = Intent(Intent.ACTION_SEND).setType("video/mp4").putExtra(Intent.EXTRA_STREAM, Uri.parse("file:///sdcard/a.mp4"))
        assertEquals(ShareRequest.Unusable(1), parseShare(intent, anyContent))
    }

    @Test
    fun theAppNeverUploadsItsOwnFiles() {
        assertFalse(isShareableUri(context, Uri.parse("file:///sdcard/DCIM/a.jpg")))
        assertFalse(isShareableUri(context, Uri.parse("content://com.thekozugroup.popcornstash.fileprovider/opened/token")))
        assertFalse(isShareableUri(context, Uri.parse("content://com.thekozugroup.popcornstash/x")))
        assertTrue(isShareableUri(context, Uri.parse("content://media/external/images/1")))
    }

    @Test
    fun aSharedFileIsDescribedAndReadThroughItsProvider() {
        Robolectric.setupContentProvider(GalleryProvider::class.java, "test.gallery")
        val uri = Uri.parse("content://test.gallery/photo/1")
        val file = describeSharedFile(context.contentResolver, uri, "Shared file 1")
        assertEquals("PXL_20261012_181733.jpg", file.name)
        assertEquals(GalleryProvider.BYTES.size.toLong(), file.size)
        assertEquals(1_760_000_000L, file.lastModified)
        val source = ContentChunkSource(context.contentResolver, uri)
        assertArrayEquals(GalleryProvider.BYTES.copyOfRange(4, 9), source.read(4, 5))
        // Going back re-reads from the start.
        assertArrayEquals(GalleryProvider.BYTES.copyOfRange(0, 3), source.read(0, 3))
        assertEquals(GalleryProvider.BYTES.size.toLong(), source.measure())
        source.close()
    }

    @Test
    fun namesLoseFolderPartsAndControlCharacters() {
        assertEquals("a.jpg", cleanFileName("../../a.jpg", "x"))
        assertEquals("ab.jpg", cleanFileName("a\u0000b.jpg", "x"))
        assertEquals("Shared file 1", cleanFileName("  ", "Shared file 1"))
    }

    /** A gallery app's provider: one photo with a name, a size and a date in milliseconds. */
    class GalleryProvider : ContentProvider() {
        companion object {
            val BYTES = "jpeg bytes of a photo".toByteArray()
        }

        override fun onCreate() = true

        override fun query(uri: Uri, projection: Array<out String>?, selection: String?, args: Array<out String>?, sort: String?): Cursor =
            MatrixCursor(arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE, "last_modified")).apply {
                addRow(arrayOf<Any>("PXL_20261012_181733.jpg", BYTES.size.toLong(), 1_760_000_000_000L))
            }

        override fun openFile(uri: Uri, mode: String): ParcelFileDescriptor {
            val file = File.createTempFile("photo", ".jpg").apply { writeBytes(BYTES) }
            return ParcelFileDescriptor.open(file, ParcelFileDescriptor.MODE_READ_ONLY)
        }

        override fun getType(uri: Uri) = "image/jpeg"

        override fun insert(uri: Uri, values: ContentValues?): Uri? = null

        override fun delete(uri: Uri, selection: String?, args: Array<out String>?) = 0

        override fun update(uri: Uri, values: ContentValues?, selection: String?, args: Array<out String>?) = 0
    }
}
