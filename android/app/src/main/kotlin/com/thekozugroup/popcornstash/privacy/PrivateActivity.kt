package com.thekozugroup.popcornstash.privacy

import android.app.KeyguardManager
import android.content.ActivityNotFoundException
import android.content.Intent
import android.hardware.biometrics.BiometricManager
import android.hardware.biometrics.BiometricManager.Authenticators.BIOMETRIC_STRONG
import android.hardware.biometrics.BiometricPrompt
import android.os.Build
import android.os.Bundle
import android.os.CancellationSignal
import android.provider.Settings
import android.view.WindowManager
import androidx.activity.compose.setContent
import androidx.activity.ComponentActivity
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.launch
import com.thekozugroup.popcornstash.PopcornStashApplication
import com.thekozugroup.popcornstash.ui.theme.PopcornTheme

/** Every production activity must gate its content here, including incoming shares. */
abstract class PrivateActivity : ComponentActivity() {
    protected val appLock get() = (application as PopcornStashApplication).appLock
    private val keyguard get() = getSystemService(KeyguardManager::class.java)
    private var authenticating by mutableStateOf(false)
    private var askedThisVisit = false
    private var attempt = -1L
    private var cancellation: CancellationSignal? = null
    private var secure by mutableStateOf(false)
    private var problem by mutableStateOf<String?>(null)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        addOnUserLeaveHintListener {
            // Home can return before onStop during its animation. Close immediately.
            if (!authenticating) { askedThisVisit = false; appLock.lock() }
        }
        window.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
        if (Build.VERSION.SDK_INT >= 33) setRecentsScreenshotEnabled(false)
    }

    protected fun setPrivateContent(content: @Composable () -> Unit) {
        setContent {
            val unlocked by appLock.unlocked.collectAsStateWithLifecycle()
            PrivateContent(unlocked && secure, locked = {
                PopcornTheme { AppLockScreen(secure, authenticating, problem, ::requestUnlock, ::setScreenLock) }
            }, content = content)
        }
        // This collector remains active while stopped, before Compose removes the player.
        lifecycleScope.launch {
            appLock.unlocked.collect { if (it) onPrivateUnlocked() else onPrivateLocked() }
        }
    }

    protected open fun onPrivateLocked() = Unit
    protected open fun onPrivateUnlocked() = Unit

    override fun onStart() {
        super.onStart()
        appLock.started(this)
    }

    override fun onResume() {
        super.onResume()
        secure = hasStrongBiometrics()
        if (!secure || keyguard?.isDeviceLocked == true) appLock.lock()
        if (!appLock.unlocked.value && secure && !askedThisVisit) requestUnlock()
    }

    override fun onStop() {
        super.onStop()
        cancellation?.cancel()
        attempt = -1L
        cancellation = null
        authenticating = false
        appLock.stopped(this, isChangingConfigurations)
        if (!authenticating) askedThisVisit = false
    }

    override fun onDestroy() {
        cancellation?.cancel()
        super.onDestroy()
    }

    private fun authenticationEnded(success: Boolean, completedAttempt: Long = attempt) {
        if (isDestroyed || completedAttempt != attempt) return
        attempt = -1L
        authenticating = false
        cancellation = null
        secure = hasStrongBiometrics()
        // A cancelled/missing/removed credential always leaves the app closed.
        if (success && secure && keyguard?.isDeviceLocked == false && !isDestroyed) {
            problem = null
            if (!appLock.completeAuthentication(completedAttempt, this)) appLock.lock()
        } else {
            appLock.lock()
        }
    }

    protected open fun hasStrongBiometrics(): Boolean = keyguard?.isDeviceSecure == true &&
        (Build.VERSION.SDK_INT < 30 || getSystemService(BiometricManager::class.java)?.canAuthenticate(BIOMETRIC_STRONG) == BiometricManager.BIOMETRIC_SUCCESS)

    private fun requestUnlock() {
        if (authenticating) return
        secure = hasStrongBiometrics()
        if (!secure) { appLock.lock(); return }
        askedThisVisit = true
        authenticating = true
        attempt = appLock.beginAuthentication()
        val authenticationAttempt = attempt
        val signal = CancellationSignal()
        cancellation = signal
        problem = null
        try {
            confirmBiometric(signal) { success -> authenticationEnded(success, authenticationAttempt) }
        } catch (_: java.security.GeneralSecurityException) {
            authenticationEnded(false, authenticationAttempt)
            problem = "Biometric verification is unavailable. Check your biometrics in Settings and try again."
        } catch (_: java.security.ProviderException) {
            authenticationEnded(false, authenticationAttempt)
            problem = "Biometric verification is unavailable. Try again."
        } catch (_: IllegalStateException) {
            authenticationEnded(false, authenticationAttempt)
            problem = "Biometric verification is unavailable. Try again."
        } catch (_: SecurityException) {
            authenticationEnded(false, authenticationAttempt)
            problem = "Biometric verification was blocked. Check your biometrics in Settings."
        }
    }

    protected open fun confirmBiometric(signal: CancellationSignal, onResult: (Boolean) -> Unit) {
        val cipher = BiometricUnlockKey.newCipher()
        val builder = BiometricPrompt.Builder(this)
            .setTitle("Unlock PopcornStash")
            .setDescription("Use strong biometrics to open your private library.")
            .setNegativeButton("Cancel", mainExecutor) { _, _ -> signal.cancel(); onResult(false) }
        if (Build.VERSION.SDK_INT >= 30) builder.setAllowedAuthenticators(BIOMETRIC_STRONG)
        // CryptoObject restricts Android 9/10 to Keystore-backed Class 3 biometrics too.
        builder.build().authenticate(BiometricPrompt.CryptoObject(cipher), signal, mainExecutor, object : BiometricPrompt.AuthenticationCallback() {
            override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) {
                val biometric = Build.VERSION.SDK_INT < 30 || result.authenticationType == BiometricPrompt.AUTHENTICATION_RESULT_TYPE_BIOMETRIC
                val verified = biometric && BiometricUnlockKey.verify(cipher, result.cryptoObject?.cipher)
                onResult(verified)
            }
            override fun onAuthenticationError(errorCode: Int, errString: CharSequence) {
                onResult(false)
                problem = "Biometric verification did not finish. Unlock your phone if needed, then try again."
            }
        })
    }

    private fun setScreenLock() {
        try {
            startActivity(Intent(Settings.ACTION_SECURITY_SETTINGS))
        } catch (_: ActivityNotFoundException) {
            problem = "Open your phone Settings and enroll strong biometrics, then return here."
        }
    }
}
