package com.ezoo.download

import android.content.Context
import java.util.Locale

object Format {
    fun size(n: Long): String {
        if (n < 1024) return "$n B"
        val u = arrayOf("KB", "MB", "GB", "TB")
        var v = n / 1024.0
        var i = 0
        while (v >= 1024 && i < u.size - 1) {
            v /= 1024
            i++
        }
        return String.format(Locale.US, if (v < 10) "%.1f %s" else "%.0f %s", v, u[i])
    }

    fun time(secs: Long): String {
        val h = secs / 3600
        val m = secs % 3600 / 60
        val s = secs % 60
        return if (h > 0) String.format(Locale.US, "%d:%02d:%02d", h, m, s)
        else String.format(Locale.US, "%d:%02d", m, s)
    }

    /** "412 MB / 860 MB · 6.4 MB/s · 1:10" */
    fun progressLine(ctx: Context, done: Long, total: Long?, speed: Long, eta: Long?): String {
        val parts = mutableListOf<String>()
        parts += if (total != null) "${size(done)} / ${size(total)}" else size(done)
        if (speed > 0) parts += "${size(speed)}/s"
        if (eta != null && eta > 0) parts += time(eta)
        return parts.joinToString(" · ")
    }
}
