package com.ezoo.download

import android.content.Context
import com.chaquo.python.PyException
import com.chaquo.python.Python
import com.ezoo.download.engine.Engine
import org.json.JSONObject
import java.io.File

/** One quality of a video page. */
data class Quality(val key: String, val label: String, val sub: String, val ext: String, val size: Long, val audio: Boolean)

data class VideoPage(val page: String, val title: String, val site: String, val qualities: List<Quality>)

/** Video pages: yt-dlp (Python, inside the app) finds the video, the engine its qualities. */
object Video {
    /** Sites that are always video pages (no need to ask the server first). */
    private val SITES = listOf(
        "youtube.com", "youtu.be", "youtube-nocookie.com", "facebook.com", "fb.watch", "instagram.com",
        "tiktok.com", "twitter.com", "x.com", "vimeo.com", "dailymotion.com", "dai.ly", "twitch.tv",
        "reddit.com", "soundcloud.com", "pinterest.com", "pin.it", "threads.net", "snapchat.com",
        "linkedin.com", "bilibili.com", "rumble.com", "ok.ru", "vk.com", "streamable.com",
    )

    fun isVideoSite(url: String): Boolean {
        val host = try {
            java.net.URI(url.trim()).host?.lowercase() ?: return false
        } catch (_: Exception) {
            return false
        }
        return SITES.any { host == it || host.endsWith(".$it") }
    }

    /** The qualities of a page. Slow (seconds): never on the main thread. */
    fun look(ctx: Context, page: String): Result<VideoPage> = try {
        val cache = File(ctx.cacheDir, "yt-dlp").apply { mkdirs() }
        val json = Python.getInstance().getModule("ezoo_video").callAttr("info", page, cache.absolutePath).toString()
        val o = JSONObject(Engine.videoOptions(page, json))
        if (o.has("error")) {
            Result.failure(Exception(o.getString("error")))
        } else {
            val a = o.getJSONArray("options")
            val list = List(a.length()) { i ->
                val q = a.getJSONObject(i)
                Quality(
                    q.getString("key"), q.optString("label"), q.optString("sub"), q.optString("ext"),
                    q.optLong("size"), q.optBoolean("audio"),
                )
            }
            Result.success(VideoPage(page, o.optString("title"), o.optString("site"), list))
        }
    } catch (e: PyException) {
        // yt-dlp's own words, without its "ERROR: [youtube] id:" start
        val msg = (e.message ?: "").substringAfterLast("ERROR: ").substringAfter("]: ").substringAfter(": ").trim()
        Result.failure(Exception(msg.ifEmpty { "the video was not found on this page" }))
    } catch (e: Exception) {
        Result.failure(e)
    }

    fun ytdlpVersion(): String = try {
        Python.getInstance().getModule("ezoo_video").callAttr("version").toString()
    } catch (_: Exception) {
        "?"
    }
}
