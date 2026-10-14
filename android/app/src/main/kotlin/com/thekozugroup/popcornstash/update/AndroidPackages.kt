package com.thekozugroup.popcornstash.update

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageInstaller
import android.content.pm.PackageManager
import android.content.pm.Signature
import android.content.pm.SigningInfo
import android.os.Build
import android.provider.Settings
import androidx.core.content.IntentCompat
import androidx.core.net.toUri
import com.thekozugroup.popcornstash.PopcornStashApplication
import java.io.File
import java.io.IOException
import java.security.MessageDigest

/** The SHA-256 of each certificate, in hex. */
private fun digests(signatures: Array<Signature>?): Set<String> =
    signatures.orEmpty().map { MessageDigest.getInstance("SHA-256").digest(it.toByteArray()).toHex() }.toSet()

/** The certificates that sign the package now (several only for an APK with more than one signer). */
fun currentSigners(info: SigningInfo): Set<String> = digests(info.apkContentsSigners)

/** The current certificates and every older one the package proved it may follow (key rotation). */
fun signingLineage(info: SigningInfo): Set<String> =
    currentSigners(info) + if (info.hasMultipleSigners()) emptySet() else digests(info.signingCertificateHistory)

/**
 * What PackageManager says about an APK on disk and about the installed app: the package,
 * the build and the signing certificates (getPackageArchiveInfo with GET_SIGNING_CERTIFICATES).
 */
class PackageManagerApks(private val context: Context) : ApkInspector {
    override fun inspect(apk: File): ApkFacts? {
        val info = try {
            context.packageManager.getPackageArchiveInfo(apk.path, PackageManager.GET_SIGNING_CERTIFICATES)
        } catch (_: RuntimeException) {
            null
        } ?: return null
        val signing = info.signingInfo ?: return null
        return ApkFacts(info.packageName, info.longVersionCode, currentSigners(signing), signingLineage(signing))
    }

    /** The installed app's own signing certificates. */
    fun installedSigners(): Set<String> {
        val info = try {
            context.packageManager.getPackageInfo(context.packageName, PackageManager.GET_SIGNING_CERTIFICATES)
        } catch (_: PackageManager.NameNotFoundException) {
            return emptySet()
        }
        return info.signingInfo?.let(::currentSigners).orEmpty()
    }
}

/** Installing a verified APK; [PackageInstallerUpdates] on a phone. */
interface UpdateInstaller {
    /** Whether the person lets this app install apps ("Install unknown apps"). */
    fun canInstall(): Boolean

    /** The system page where the person allows that, for this app. */
    fun permissionIntent(): Intent

    /** Hands [apk] to Android; the result arrives at [UpdateInstallReceiver]. */
    @Throws(IOException::class)
    fun install(apk: File)
}

/**
 * Installs through a PackageInstaller session. On Android 12 and later it asks to install
 * without a confirmation (USER_ACTION_NOT_REQUIRED), which Android grants only once this app
 * installed itself before (it is then the installer of record); otherwise Android shows its
 * confirmation, which [UpdateInstallReceiver] opens.
 */
class PackageInstallerUpdates(private val context: Context) : UpdateInstaller {
    override fun canInstall(): Boolean = context.packageManager.canRequestPackageInstalls()

    override fun permissionIntent(): Intent =
        Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, "package:${context.packageName}".toUri())
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)

    override fun install(apk: File) {
        val installer = context.packageManager.packageInstaller
        val params = PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL).apply {
            setAppPackageName(context.packageName)
            setSize(apk.length())
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                setRequireUserAction(PackageInstaller.SessionParams.USER_ACTION_NOT_REQUIRED)
            }
        }
        val sessionId = installer.createSession(params)
        try {
            installer.openSession(sessionId).use { session ->
                apk.inputStream().use { input ->
                    session.openWrite("base.apk", 0, apk.length()).use { output ->
                        input.copyTo(output)
                        session.fsync(output)
                    }
                }
                session.commit(statusReceiver().intentSender)
            }
        } catch (e: IOException) {
            installer.abandonSession(sessionId)
            throw e
        } catch (e: RuntimeException) {
            installer.abandonSession(sessionId)
            throw IOException(e.message, e)
        }
    }

    // Mutable: PackageInstaller adds the status and the confirmation to the intent. It names
    // the receiver, so nothing else can receive it.
    private fun statusReceiver(): PendingIntent {
        val flags = PendingIntent.FLAG_UPDATE_CURRENT or
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) PendingIntent.FLAG_MUTABLE else 0
        return PendingIntent.getBroadcast(context, REQUEST_CODE, Intent(context, UpdateInstallReceiver::class.java), flags)
    }

    private companion object {
        const val REQUEST_CODE = 415
    }
}

/**
 * PackageInstaller's answer about an update: Android's confirmation to show, or how it ended.
 * It is not exported; only the PendingIntent this app gave the session reaches it.
 */
class UpdateInstallReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val status = intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE)
        val updates = (context.applicationContext as? PopcornStashApplication)?.container?.updates ?: return
        if (status == PackageInstaller.STATUS_PENDING_USER_ACTION) {
            val confirm = IntentCompat.getParcelableExtra(intent, Intent.EXTRA_INTENT, Intent::class.java)
            updates.onInstallStatus(InstallStatus.NeedsConfirmation)
            if (confirm != null) {
                try {
                    context.startActivity(confirm.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
                } catch (_: RuntimeException) {
                    updates.onInstallStatus(InstallStatus.Failed(null))
                }
            }
            return
        }
        updates.onInstallStatus(
            when (status) {
                PackageInstaller.STATUS_SUCCESS -> InstallStatus.Installed
                PackageInstaller.STATUS_FAILURE_ABORTED -> InstallStatus.Cancelled
                else -> InstallStatus.Failed(intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE))
            },
        )
    }
}

/** How an install went, as [UpdateInstallReceiver] heard it. */
sealed interface InstallStatus {
    data object NeedsConfirmation : InstallStatus

    data object Installed : InstallStatus

    data object Cancelled : InstallStatus

    data class Failed(val message: String?) : InstallStatus
}
