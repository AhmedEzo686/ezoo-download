package com.ezoo.download

import android.app.Notification
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.media.MediaScannerConnection
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.PowerManager
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import com.ezoo.download.engine.Download
import com.ezoo.download.engine.Engine

/**
 * Keeps the downloads going while the app is in the background: a
 * notification with their progress, one more when each finishes. It stops
 * by itself when nothing downloads.
 */
class DownloadService : Service() {
    private val handler = Handler(Looper.getMainLooper())
    private var idleTicks = 0
    private var wake: PowerManager.WakeLock? = null
    private val failedSeen = HashSet<Long>()
    private var first = true

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        running = true
        val type = if (Build.VERSION.SDK_INT >= 29) ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC else 0
        ServiceCompat.startForeground(this, ID_PROGRESS, progressNotification(emptyList()), type)
        wake = getSystemService(PowerManager::class.java)
            .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "ezoo:downloads")
            .apply { setReferenceCounted(false) }
        handler.post(tick)
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_PAUSE_ALL -> Engine.pauseAll()
        }
        idleTicks = 0
        return START_STICKY
    }

    override fun onDestroy() {
        handler.removeCallbacks(tick)
        wake?.let { if (it.isHeld) it.release() }
        running = false
        super.onDestroy()
    }

    private val tick = object : Runnable {
        override fun run() {
            val all = Download.parseList(Engine.list())
            val active = all.filter { it.running || it.queued }
            finished(all)
            if (active.isEmpty()) {
                if (++idleTicks >= 3) {
                    ServiceCompat.stopForeground(this@DownloadService, ServiceCompat.STOP_FOREGROUND_REMOVE)
                    stopSelf()
                    return
                }
                wake?.let { if (it.isHeld) it.release() }
            } else {
                idleTicks = 0
                // held a while at a time: let go of it if the app is killed
                wake?.acquire(10 * 60 * 1000L)
                notify(ID_PROGRESS, progressNotification(active))
            }
            handler.postDelayed(this, 1000)
        }
    }

    /** One notification for each download that finished or failed. */
    private fun finished(all: List<Download>) {
        val done = Engine.takeCompleted().toHashSet()
        for (d in all) {
            if (d.id in done && d.completed) {
                MediaScannerConnection.scanFile(this, arrayOf(d.path), null, null)
                notify(ID_DONE_BASE + d.id.toInt(), doneNotification(d, true))
            }
            if (d.failed) {
                // failed before this run: not news
                if (failedSeen.add(d.id) && !first) notify(ID_DONE_BASE + d.id.toInt(), doneNotification(d, false))
            } else {
                failedSeen.remove(d.id)
            }
        }
        first = false
    }

    private fun notify(id: Int, n: Notification) {
        try {
            NotificationManagerCompat.from(this).notify(id, n)
        } catch (_: SecurityException) {
            // notifications not allowed: the downloads go on all the same
        }
    }

    private fun openApp(): PendingIntent = PendingIntent.getActivity(
        this, 0, Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

    private fun progressNotification(active: List<Download>): Notification {
        val b = NotificationCompat.Builder(this, EzooApp.CH_PROGRESS)
            .setSmallIcon(R.drawable.ic_notify)
            .setContentIntent(openApp())
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setSilent(true)
            .setCategory(NotificationCompat.CATEGORY_PROGRESS)
            .addAction(
                0, "إيقاف الكل",
                PendingIntent.getService(
                    this, 1, Intent(this, DownloadService::class.java).setAction(ACTION_PAUSE_ALL),
                    PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
                ),
            )
        if (active.isEmpty()) return b.setContentTitle(getString(R.string.app_name)).build()
        val total = active.sumOf { it.total ?: 0L }
        val done = active.sumOf { if (it.total != null) it.done else 0L }
        val speed = active.sumOf { it.speed }
        val title = if (active.size == 1) active[0].name else "${active.size} تحميلات شغالة"
        b.setContentTitle(title)
            .setContentText(Format.progressLine(done, total.takeIf { it > 0 }, speed, null))
        if (total > 0) b.setProgress(1000, (done * 1000 / total).toInt(), false) else b.setProgress(0, 0, true)
        return b.build()
    }

    private fun doneNotification(d: Download, ok: Boolean): Notification {
        val tap = if (ok) Files.openIntent(this, d.path)?.let {
            PendingIntent.getActivity(this, d.id.toInt(), it, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        } else null
        return NotificationCompat.Builder(this, EzooApp.CH_DONE)
            .setSmallIcon(R.drawable.ic_notify)
            .setContentTitle(if (ok) "✓ التحميل خلص" else "التحميل فشل")
            .setContentText(if (ok) d.name else listOfNotNull(d.name, d.error).joinToString(" - "))
            .setContentIntent(tap ?: openApp())
            .setAutoCancel(true)
            .build()
    }

    companion object {
        private const val ID_PROGRESS = 1
        private const val ID_DONE_BASE = 1000
        private const val ACTION_PAUSE_ALL = "com.ezoo.download.PAUSE_ALL"

        @Volatile
        var running = false
            private set

        fun start(ctx: Context) {
            try {
                ContextCompat.startForegroundService(ctx, Intent(ctx, DownloadService::class.java))
            } catch (_: Exception) {
                // the system refused (app in the background): the downloads
                // still run while the process lives
            }
        }
    }
}
