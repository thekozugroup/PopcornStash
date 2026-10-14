# Update test fixtures

Two real APKs for the updater's tests (`update/UpdateEndToEndTest`), each about 8 KB. They hold
only a manifest (`fixture-manifest.xml`: package `com.thekozugroup.popcornstash`, versionCode 415,
versionName `0.1.0+415`, no code), so they cannot run.

| File | Signed with | Certificate SHA-256 |
| --- | --- | --- |
| `build-415.apk` | throwaway key "CN=Throwaway test key" | `7721ec2f9c281a3bde0e53633e947fa6e89e31ef53cb9816e4dda8ef1224fda4` |
| `build-415-other-key.apk` | throwaway key "CN=Other test key" | `f288080fcc7845f386f1ff1911a122d6b5285636f5553d2ba5327e018d0275eb` |

Both keys were made for these files and deleted; nothing here can sign anything else. To make
them again (new keys, so the fingerprints in the test change too):

```sh
BT=$ANDROID_HOME/build-tools/36.0.0
$BT/aapt2 link -o unsigned.apk -I $ANDROID_HOME/platforms/android-37.0/android.jar --manifest fixture-manifest.xml
keytool -genkeypair -keystore throwaway.jks -storepass throwaway-pass -keypass throwaway-pass -alias throwaway \
  -keyalg RSA -keysize 2048 -validity 3650 -dname "CN=Throwaway test key"
$BT/apksigner sign --ks throwaway.jks --ks-pass pass:throwaway-pass --out build-415.apk unsigned.apk
$BT/apksigner verify --print-certs build-415.apk | grep SHA-256
rm throwaway.jks unsigned.apk
```

and the same with a second key for `build-415-other-key.apk`.
