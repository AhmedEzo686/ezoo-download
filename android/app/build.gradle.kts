plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
    id("com.chaquo.python")
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
        // nearly every phone (arm64) and the older ones (32-bit arm)
        ndk {
            abiFilters += listOf("arm64-v8a", "armeabi-v7a")
        }
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
        // the libraries are put in the app's folder on the phone: ffmpeg
        // (libffmpeg.so) is a program, run from there
        jniLibs.useLegacyPackaging = true
    }
}

// yt-dlp (it finds the video and its qualities on video pages), with
// Python 3.12; the newest yt-dlp of the day of the build
chaquopy {
    defaultConfig {
        version = "3.12"
        pip {
            install("yt-dlp")
            install("yt-dlp-ejs")
            install("certifi")
            install("requests")
            install("urllib3")
            install("websockets")
            install("mutagen")
        }
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
