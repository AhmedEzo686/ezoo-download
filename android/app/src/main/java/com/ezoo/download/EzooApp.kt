package com.ezoo.download

import android.app.Application
import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.Context
import android.os.Build
import android.os.Environment
import com.chaquo.python.Python
import com.chaquo.python.android.AndroidPlatform
import com.ezoo.download.engine.Engine
import java.io.File

class EzooApp : Application() {
    override fun onCreate() {
        super.onCreate()
        channels()
        // Python for yt-dlp (video pages); it starts in a moment, here once
        try {
            if (!Python.isStarted()) Python.start(AndroidPlatform(this))
        } catch (_: Throwable) {
        }
        val data = File(filesDir, "data")
        engineError = try {
            Engine.init(data.absolutePath, downloadRoot().absolutePath, applicationInfo.nativeLibraryDir)
        } catch (e: Throwable) {
            e.toString()
        }
        // downloads that were running when the app was closed go on
        if (engineError == null && Engine.busy()) DownloadService.start(this)
    }

    private fun channels() {
        if (Build.VERSION.SDK_INT < 26) return
        val nm = getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(
            NotificationChannel(CH_PROGRESS, getString(R.string.ch_progress), NotificationManager.IMPORTANCE_LOW)
                .apply { setShowBadge(false) }
        )
        nm.createNotificationChannel(
            NotificationChannel(CH_DONE, getString(R.string.ch_done), NotificationManager.IMPORTANCE_DEFAULT)
        )
    }

    companion object {
        const val CH_PROGRESS = "progress"
        const val CH_DONE = "done"

        /** Null when the engine runs, else why it does not. */
        var engineError: String? = null
            private set

        /** Download/Ezoo: each kind of file in its folder (Video, Music, ...). */
        @Suppress("DEPRECATION")
        fun downloadRoot(): File =
            File(Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS), "Ezoo")

        /** The download service runs while something downloads. */
        fun kick(ctx: Context) = DownloadService.start(ctx)
    }
}
