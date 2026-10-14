# kotlinx.serialization, OkHttp and AndroidX ship their own keep rules.

# Credential Manager loads the Play services provider (passkeys on Android 13 and
# earlier) by name.
-if class androidx.credentials.CredentialManager
-keep class androidx.credentials.playservices.** {
  *;
}
