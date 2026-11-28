package com.ezoo.download.engine

import org.json.JSONArray
import org.json.JSONObject

/** One download of the list (what [Engine.list] gives). */
data class Download(
    val id: Long,
    val name: String,
    val path: String,
    val category: String,
    val kind: String,
    val status: String,
    val error: String?,
    val total: Long?,
    val done: Long,
    val speed: Long,
    val eta: Long?,
    val url: String,
    val waiting: Boolean,
    val connections: Int,
) {
    val running get() = status == "Connecting" || status == "Downloading" || status == "Assembling"
    val queued get() = status == "Queued"
    val completed get() = status == "Completed"
    val failed get() = status == "Failed"
    val paused get() = status == "Paused"
    val torrent get() = kind == "Torrent"
    val progress: Float
        get() = when {
            completed -> 1f
            total != null && total > 0 -> (done.toFloat() / total).coerceIn(0f, 1f)
            else -> 0f
        }

    companion object {
        private fun JSONObject.optLongOrNull(k: String): Long? = if (isNull(k)) null else optLong(k)
        private fun JSONObject.optTextOrNull(k: String): String? = if (isNull(k)) null else optString(k)

        fun parseList(json: String): List<Download> = try {
            val a = JSONArray(json)
            List(a.length()) { i ->
                val o = a.getJSONObject(i)
                Download(
                    id = o.getLong("id"),
                    name = o.optString("name"),
                    path = o.optString("path"),
                    category = o.optString("category", "General"),
                    kind = o.optString("kind"),
                    status = o.optString("status"),
                    error = o.optTextOrNull("error"),
                    total = o.optLongOrNull("total"),
                    done = o.optLong("done"),
                    speed = o.optLong("speed"),
                    eta = o.optLongOrNull("eta"),
                    url = o.optString("url"),
                    waiting = o.optBoolean("waiting"),
                    connections = o.optInt("connections"),
                )
            }
        } catch (e: Exception) {
            emptyList()
        }
    }
}
