plugins { id("com.android.application"); id("org.jetbrains.kotlin.android"); id("org.jetbrains.kotlin.plugin.compose") }
android {
 namespace = "com.personal.commandcenter"
 compileSdk = 35
 defaultConfig { applicationId = "com.personal.commandcenter.mobile"; minSdk = 31; targetSdk = 35; versionCode = 5; versionName = "0.5.0"; ndk { abiFilters += "arm64-v8a" } }
 signingConfigs {
  create("ownerRelease") {
   val path = System.getenv("PCC_ANDROID_KEYSTORE")
   if (!path.isNullOrBlank()) {
    storeFile = file(path)
    storePassword = System.getenv("PCC_ANDROID_STORE_PASSWORD") ?: error("Missing release store password")
    keyAlias = System.getenv("PCC_ANDROID_KEY_ALIAS") ?: error("Missing release key alias")
    keyPassword = System.getenv("PCC_ANDROID_KEY_PASSWORD") ?: error("Missing release key password")
   }
  }
 }
 buildTypes { getByName("release") { if (!System.getenv("PCC_ANDROID_KEYSTORE").isNullOrBlank()) signingConfig = signingConfigs.getByName("ownerRelease") } }
 buildFeatures { compose = true }
 compileOptions { sourceCompatibility = JavaVersion.VERSION_17; targetCompatibility = JavaVersion.VERSION_17 }
 kotlinOptions { jvmTarget = "17" }
}
dependencies {
 implementation("com.google.firebase:firebase-messaging:26.0.2")
 implementation(platform("androidx.compose:compose-bom:2025.03.01"))
 implementation("androidx.activity:activity-compose:1.10.1")
 implementation("androidx.compose.material3:material3")
 implementation("androidx.compose.ui:ui")
 implementation("androidx.compose.foundation:foundation")
}
