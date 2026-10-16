package com.thekozugroup.popcornstash.privacy

import android.annotation.SuppressLint
import android.app.KeyguardManager
import android.content.ActivityNotFoundException
import android.content.Intent
import android.hardware.biometrics.BiometricManager.Authenticators.DEVICE_CREDENTIAL
import android.hardware.biometrics.BiometricPrompt
import android.os.Build
import android.os.Bundle
import android.os.CancellationSignal
import android.provider.Settings
import android.view.WindowManager
import androidx.activity.compose.setContent
import androidx.activity.ComponentActivity
import androidx.activity.result.contract.ActivityResultContracts
import androidx.annotation.RequiresApi
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
    private var legacyInFlight = false
    private var attempt = -1L
    private var cancellation: CancellationSignal? = null
    private var secure by mutableStateOf(false)
    private var problem by mutableStateOf<String?>(null)

    @SuppressLint("InvalidFragmentVersionForActivityResult")
    private val confirmLegacy = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
        authenticationEnded(result.resultCode == RESULT_OK)
    }

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
        secure = keyguard?.isDeviceSecure == true
        if (!secure || keyguard?.isDeviceLocked == true) appLock.lock()
        if (!appLock.unlocked.value && secure && !askedThisVisit) requestUnlock()
    }

    override fun onStop() {
        super.onStop()
        appLock.stopped(this, isChangingConfigurations, legacyInFlight)
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
        legacyInFlight = false
        cancellation = null
        secure = keyguard?.isDeviceSecure == true
        // A cancelled/missing/removed credential always leaves the app closed.
        if (success && secure && keyguard?.isDeviceLocked == false && !isDestroyed) {
            problem = null
            if (!appLock.completeAuthentication(completedAttempt, this)) appLock.lock()
        } else {
            appLock.lock()
        }
    }

    @Suppress("DEPRECATION")
    private fun requestUnlock() {
        if (authenticating) return
        secure = keyguard?.isDeviceSecure == true
        if (!secure) { appLock.lock(); return }
        askedThisVisit = true
        authenticating = true
        attempt = appLock.beginAuthentication()
        problem = null
        try {
            if (Build.VERSION.SDK_INT >= 30) confirmCredential() else {
                // Credential-only BiometricPrompt is unavailable on Android 9 and 10.
                val intent = keyguard?.createConfirmDeviceCredentialIntent("Unlock PopcornStash", "Use your device screen lock to open your private library.")
                if (intent == null) authenticationEnded(false) else { legacyInFlight = true; confirmLegacy.launch(intent) }
            }
        } catch (_: ActivityNotFoundException) {
            authenticationEnded(false)
            problem = "Device verification is unavailable. Check your screen lock in Settings."
        } catch (_: SecurityException) {
            authenticationEnded(false)
            problem = "Device verification was blocked. Check your screen lock in Settings."
        }
    }

    @RequiresApi(30)
    private fun confirmCredential() {
        val authenticationAttempt = attempt
        val signal = CancellationSignal()
        cancellation = signal
        BiometricPrompt.Builder(this)
            .setTitle("Unlock PopcornStash")
            .setDescription("Use your device screen lock to open your private library.")
            .setAllowedAuthenticators(DEVICE_CREDENTIAL)
            .build()
            .authenticate(signal, mainExecutor, object : BiometricPrompt.AuthenticationCallback() {
                override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) =
                    authenticationEnded(result.authenticationType == BiometricPrompt.AUTHENTICATION_RESULT_TYPE_DEVICE_CREDENTIAL, authenticationAttempt)
                override fun onAuthenticationError(errorCode: Int, errString: CharSequence) = authenticationEnded(false, authenticationAttempt)
            })
    }

    private fun setScreenLock() {
        try {
            startActivity(Intent(Settings.ACTION_SECURITY_SETTINGS))
        } catch (_: ActivityNotFoundException) {
            problem = "Open your phone Settings and add a screen lock, then return here."
        }
    }
}
