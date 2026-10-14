package com.thekozugroup.popcornstash.update

import android.annotation.SuppressLint
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
import android.util.Log
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
    // Android 9 and 10 read an archive's certificates only when GET_SIGNATURES is asked for too;
    // without it signingInfo comes back null. The comparison uses signingInfo (every signer and
    // the lineage), never the old signatures array, which is what the lint check warns about.
    @SuppressLint("PackageManagerGetSignatures")
    override fun inspect(apk: File): ApkFacts? {
        val flags = if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) {
            @Suppress("DEPRECATION")
            PackageManager.GET_SIGNING_CERTIFICATES or PackageManager.GET_SIGNATURES
        } else {
            PackageManager.GET_SIGNING_CERTIFICATES
        }
        val info = try {
            context.packageManager.getPackageArchiveInfo(apk.path, flags)
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

    /** Android's confirmation for the install waiting on the person, kept so the page can open it again. */
    fun confirmation(): Intent?

    /** Whether an install session of this app's is still with Android (being written, or waiting for the person). */
    fun hasSession(): Boolean
}

/**
 * Installs through a PackageInstaller session. On Android 12 and later it asks to install
 * without a confirmation (USER_ACTION_NOT_REQUIRED, with the UPDATE_PACKAGES_WITHOUT_USER_ACTION
 * permission in the manifest), which Android grants to an app updating itself; when Android
 * still wants the person to confirm, [UpdateInstallReceiver] opens its confirmation and keeps it
 * in [pendingConfirmation] so the page can open it again.
 */
class PackageInstallerUpdates(private val context: Context) : UpdateInstaller {
    /** Set by [UpdateInstallReceiver] while Android waits for the person; null otherwise. */
    @Volatile
    var pendingConfirmation: Intent? = null

    override fun canInstall(): Boolean = context.packageManager.canRequestPackageInstalls()

    override fun permissionIntent(): Intent =
        Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, "package:${context.packageName}".toUri())
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)

    override fun confirmation(): Intent? = pendingConfirmation?.let(::Intent)

    override fun hasSession(): Boolean = try {
        context.packageManager.packageInstaller.mySessions.isNotEmpty()
    } catch (_: RuntimeException) {
        false
    }

    override fun install(apk: File) {
        pendingConfirmation = null
        val installer = context.packageManager.packageInstaller
        var sessionId: Int? = null
        try {
            val params = PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL).apply {
                setAppPackageName(context.packageName)
                setSize(apk.length())
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                    setRequireUserAction(PackageInstaller.SessionParams.USER_ACTION_NOT_REQUIRED)
                }
            }
            // Inside the try: Android refuses a new session with an exception (too many open, a policy).
            val id = installer.createSession(params).also { sessionId = it }
            installer.openSession(id).use { session ->
                apk.inputStream().use { input ->
                    session.openWrite("base.apk", 0, apk.length()).use { output ->
                        input.copyTo(output)
                        session.fsync(output)
                    }
                }
                session.commit(statusReceiver().intentSender)
            }
        } catch (e: IOException) {
            abandon(installer, sessionId)
            throw e
        } catch (e: RuntimeException) {
            abandon(installer, sessionId)
            throw IOException(e.message, e)
        }
    }

    private fun abandon(installer: PackageInstaller, sessionId: Int?) {
        if (sessionId == null) return
        try {
            installer.abandonSession(sessionId)
        } catch (_: RuntimeException) {
            // Already gone.
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
        val container = (context.applicationContext as? PopcornStashApplication)?.container ?: return
        val updates = container.updates
        if (status == PackageInstaller.STATUS_PENDING_USER_ACTION) {
            val confirm = IntentCompat.getParcelableExtra(intent, Intent.EXTRA_INTENT, Intent::class.java)
            container.updateInstaller.pendingConfirmation = confirm
            updates.onInstallStatus(InstallStatus.NeedsConfirmation(canReopen = confirm != null))
            if (confirm != null) {
                try {
                    context.startActivity(Intent(confirm).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
                } catch (_: RuntimeException) {
                    // Android may not let it open from here; the page offers "Install", which opens it from the app.
                }
            }
            return
        }
        container.updateInstaller.pendingConfirmation = null
        if (status != PackageInstaller.STATUS_SUCCESS) {
            // Android's own words are for the log; the page says it plainly (InstallFailure).
            Log.w(TAG, "Install status $status: ${intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE)}")
        }
        updates.onInstallStatus(installStatusOf(status))
    }

    private companion object {
        const val TAG = "PopcornStashUpdate"
    }
}

/** PackageInstaller's final status as the updater's outcome. */
fun installStatusOf(status: Int): InstallStatus = when (status) {
    PackageInstaller.STATUS_SUCCESS -> InstallStatus.Installed
    PackageInstaller.STATUS_FAILURE_ABORTED -> InstallStatus.Cancelled
    PackageInstaller.STATUS_FAILURE_STORAGE -> InstallStatus.Failed(InstallFailure.Storage)
    PackageInstaller.STATUS_FAILURE_INCOMPATIBLE -> InstallStatus.Failed(InstallFailure.Incompatible)
    PackageInstaller.STATUS_FAILURE_CONFLICT -> InstallStatus.Failed(InstallFailure.Conflict)
    PackageInstaller.STATUS_FAILURE_BLOCKED -> InstallStatus.Failed(InstallFailure.Blocked)
    PackageInstaller.STATUS_FAILURE_INVALID -> InstallStatus.Failed(InstallFailure.Invalid)
    else -> InstallStatus.Failed(InstallFailure.Other)
}

/** How an install went, as [UpdateInstallReceiver] heard it. */
sealed interface InstallStatus {
    /** Android wants the person to confirm; [canReopen] when it gave a confirmation the page can open again. */
    data class NeedsConfirmation(val canReopen: Boolean) : InstallStatus

    data object Installed : InstallStatus

    data object Cancelled : InstallStatus

    data class Failed(val reason: InstallFailure) : InstallStatus
}
