package com.ezoo.download

import java.util.Locale

/** Sizes, speeds and times as the app shows them ("412 من 860 ميجا"). */
object Format {
    private val UNITS = arrayOf("بايت", "ك.ب", "ميجا", "جيجا", "تيرا")

    private fun split(n: Long): Pair<String, Int> {
        var v = n.toDouble()
        var i = 0
        while (v >= 1024 && i < UNITS.size - 1) {
            v /= 1024
            i++
        }
        val num = when {
            i == 0 -> v.toLong().toString()
            v < 10 -> String.format(Locale.US, "%.1f", v)
            else -> String.format(Locale.US, "%.0f", v)
        }
        return num to i
    }

    fun size(n: Long): String = split(n).let { (v, u) -> "$v ${UNITS[u]}" }

    /** "412 من 860 ميجا" (one unit when both have it) */
    fun of(done: Long, total: Long): String {
        val (d, du) = split(done)
        val (t, tu) = split(total)
        return if (du == tu) "$d من $t ${UNITS[tu]}" else "$d ${UNITS[du]} من $t ${UNITS[tu]}"
    }

    fun speed(n: Long): String = "${size(n)}/ث"

    fun time(secs: Long): String {
        val h = secs / 3600
        val m = secs % 3600 / 60
        val s = secs % 60
        return if (h > 0) String.format(Locale.US, "%d:%02d:%02d", h, m, s)
        else String.format(Locale.US, "%d:%02d", m, s)
    }

    /** "412 من 860 ميجا · 6.4 ميجا/ث · فاضل 1:10" */
    fun progressLine(done: Long, total: Long?, speed: Long, eta: Long?): String {
        val parts = mutableListOf<String>()
        parts += if (total != null && total > 0) of(done, total) else size(done)
        if (speed > 0) parts += speed(speed)
        if (eta != null && eta > 0) parts += "فاضل ${time(eta)}"
        return parts.joinToString(" · ")
    }
}
