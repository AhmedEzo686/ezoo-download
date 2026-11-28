plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

// the version of the whole program (the workspace's Cargo.toml)
val ezooVersion: String = Regex("""(?m)^version\s*=\s*"([^"]+)"""")
    .find(rootProject.file("../Cargo.toml").readText())?.groupValues?.get(1) ?: "0.0.0"
val ezooCode: Int = ezooVersion.split(".").map { it.toIntOrNull() ?: 0 }
    .let { (it.getOrElse(0) { 0 } * 10000) + (it.getOrElse(1) { 0 } * 100) + it.getOrElse(2) { 0 } }

android {
    namespace = "com.ezoo.download"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.ezoo.download"
        minSdk = 24
        targetSdk = 35
        versionCode = ezooCode
        versionName = ezooVersion
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }
    buildFeatures {
        compose = true
    }
    packaging {
        // the engine (libezoo.so) is loaded straight from the APK
        jniLibs.useLegacyPackaging = false
    }
}

dependencies {
    val bom = platform("androidx.compose:compose-bom:2025.05.01")
    implementation(bom)
    implementation("androidx.core:core-ktx:1.16.0")
    implementation("androidx.activity:activity-compose:1.10.1")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.9.0")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-core")
}
