plugins {
    id("com.android.application") version "8.10.1" apply false
    id("org.jetbrains.kotlin.android") version "2.1.21" apply false
    id("org.jetbrains.kotlin.plugin.compose") version "2.1.21" apply false
    // Python inside the app, for yt-dlp (video pages)
    id("com.chaquo.python") version "16.1.0" apply false
}
