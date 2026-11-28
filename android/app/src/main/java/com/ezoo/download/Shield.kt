package com.ezoo.download

import android.annotation.SuppressLint
import android.content.Context
import android.content.SharedPreferences
import android.os.Handler
import android.os.Looper
import android.webkit.JavascriptInterface
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import com.ezoo.download.engine.AdBlock
import org.json.JSONObject
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.Executors

/**
 * The browser's protection: which parts are on, the sites left alone, the
 * counters, and keeping the filter lists fresh (every few days, by itself).
 */
object Shield {
    private lateinit var prefs: SharedPreferences
    private lateinit var dir: File
    private val worker = Executors.newSingleThreadExecutor()
    private val main = Handler(Looper.getMainLooper())

    /** Lists are read and blocking works. */
    var ready by mutableStateOf(false)
        private set

    /** What the protection does now (shown in its settings). */
    var status by mutableStateOf("")
        private set

    var adsBlocked by mutableLongStateOf(0L)
        private set
    var popupsBlocked by mutableLongStateOf(0L)
        private set
    var listsTime by mutableLongStateOf(0L)
        private set

    var ads by mutableStateOf(true)
        private set
    var popups by mutableStateOf(true)
        private set
    var trackers by mutableStateOf(true)
        private set
    var cookies by mutableStateOf(true)
        private set
    var annoyances by mutableStateOf(false)
        private set
    var allowed by mutableStateOf(setOf<String>())
        private set

    private const val REFRESH_SECS = 4 * 24 * 3600L

    fun start(ctx: Context) {
        if (::prefs.isInitialized) return
        prefs = ctx.getSharedPreferences("shield", Context.MODE_PRIVATE)
        dir = File(ctx.filesDir, "adblock").apply { mkdirs() }
        ads = prefs.getBoolean("ads", true)
        popups = prefs.getBoolean("popups", true)
        trackers = prefs.getBoolean("trackers", true)
        cookies = prefs.getBoolean("cookies", true)
        annoyances = prefs.getBoolean("annoyances", false)
        allowed = prefs.getStringSet("allowed", emptySet())!!.toSet()
        adsBlocked = prefs.getLong("adsBlocked", 0)
        popupsBlocked = prefs.getLong("popupsBlocked", 0)
        worker.execute {
            listsTime = AdBlock.listsTime(dir.path)
            if (listsTime > 0) load()
            val old = System.currentTimeMillis() / 1000 - listsTime > REFRESH_SECS
            if (old) update()
            if (!File(dir, "resources.json").exists() || old) main.post { Resources.build(ctx.applicationContext, dir) }
        }
    }

    private fun groups(): String = buildList {
        if (ads) add("ads")
        if (trackers) add("privacy")
        if (cookies) add("cookies")
        if (annoyances) add("annoyances")
    }.joinToString(",")

    private fun load() {
        status = "بيجهز قوائم الحماية…"
        val n = AdBlock.load(dir.path, groups())
        ready = n > 0
        status = ""
    }

    /** Fetch the lists now (also from the settings: "تحديث"). */
    fun update() {
        status = "بيحدّث قوائم الحماية…"
        val r = try {
            JSONObject(AdBlock.update(dir.path))
        } catch (_: Exception) {
            JSONObject()
        }
        listsTime = AdBlock.listsTime(dir.path)
        load()
        if (r.optInt("ok") == 0) status = "ماقدرش يحدّث القوائم (النت؟)"
    }

    fun updateNow(ctx: Context) {
        worker.execute { update() }
        main.post { Resources.build(ctx.applicationContext, dir) }
    }

    internal fun resourcesSaved() {
        worker.execute { load() }
    }

    fun set(key: String, on: Boolean) {
        prefs.edit().putBoolean(key, on).apply()
        when (key) {
            "ads" -> ads = on
            "popups" -> popups = on
            "trackers" -> trackers = on
            "cookies" -> cookies = on
            "annoyances" -> annoyances = on
        }
        if (key != "popups") worker.execute { load() }
    }

    /** The site (host) is left alone by the protection. */
    fun isAllowed(host: String?): Boolean = host != null && allowed.any { host == it || host.endsWith(".$it") }

    fun allow(host: String, on: Boolean) {
        val site = host.removePrefix("www.")
        allowed = if (on) allowed + site else allowed - site - host
        prefs.edit().putStringSet("allowed", allowed).apply()
    }

    /** Blocking works on this page (lists ready, the site not left alone). */
    fun activeFor(host: String?): Boolean = ready && (ads || trackers || cookies || annoyances) && !isAllowed(host)

    private var unsaved = 0
    fun countAd() {
        adsBlocked++
        save()
    }

    fun countPopup() {
        popupsBlocked++
        save()
    }

    private fun save() {
        if (++unsaved < 20) return
        unsaved = 0
        prefs.edit().putLong("adsBlocked", adsBlocked).putLong("popupsBlocked", popupsBlocked).apply()
    }

    fun flush() {
        if (::prefs.isInitialized) prefs.edit().putLong("adsBlocked", adsBlocked).putLong("popupsBlocked", popupsBlocked).apply()
    }
}

/**
 * The stand-ins and page fixes the filter lists name ("redirect=",
 * "+js(...)"): uBlock Origin's own, fetched from its newest release by a
 * hidden page and turned into the form the blocker reads (resources.json).
 */
private object Resources {
    private var running = false

    @SuppressLint("SetJavaScriptEnabled")
    fun build(ctx: Context, dir: File) {
        if (running) return
        running = true
        Thread {
            val tag = latestTag() ?: "master"
            Handler(Looper.getMainLooper()).post { run(ctx, dir, tag) }
        }.start()
    }

    private fun latestTag(): String? = try {
        val c = URL("https://api.github.com/repos/gorhill/uBlock/releases/latest").openConnection() as HttpURLConnection
        c.connectTimeout = 15000
        c.readTimeout = 15000
        c.setRequestProperty("Accept", "application/vnd.github+json")
        val t = c.inputStream.bufferedReader().use { it.readText() }
        JSONObject(t).optString("tag_name").ifEmpty { null }
    } catch (_: Exception) {
        null
    }

    @SuppressLint("SetJavaScriptEnabled")
    private fun run(ctx: Context, dir: File, tag: String) {
        val web = WebView(ctx)
        val main = Handler(Looper.getMainLooper())
        var finished = false
        fun end() {
            if (finished) return
            finished = true
            running = false
            main.post { web.destroy() }
        }
        web.settings.javaScriptEnabled = true
        web.webViewClient = WebViewClient()
        web.addJavascriptInterface(object {
            @JavascriptInterface
            fun done(json: String) {
                try {
                    val tmp = File(dir, "resources.part")
                    tmp.writeText(json)
                    tmp.renameTo(File(dir, "resources.json"))
                    Shield.resourcesSaved()
                } catch (_: Exception) {
                }
                end()
            }

            @JavascriptInterface
            fun fail(@Suppress("UNUSED_PARAMETER") why: String) {
                end()
            }
        }, "EzooRes")
        val base = "https://cdn.jsdelivr.net/gh/gorhill/uBlock@$tag/src/"
        web.loadDataWithBaseURL(base, PAGE, "text/html", "utf-8", null)
        // never more than two minutes
        main.postDelayed({ end() }, 120_000)
    }

    private const val PAGE = """<!doctype html><meta charset="utf-8"><script type="module">
const b64 = (bytes) => { let s = ''; for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000)); return btoa(s); };
const MIMES = { js: 'application/javascript', gif: 'image/gif', png: 'image/png', html: 'text/html', txt: 'text/plain', mp4: 'video/mp4', mp3: 'audio/mp3', json: 'application/json', xml: 'text/xml', css: 'text/css', svg: 'image/svg+xml' };
(async () => {
  try {
    const out = [];
    const { builtinScriptlets } = await import('./js/resources/scriptlets.js');
    for (const d of builtinScriptlets) {
      if (typeof d.fn !== 'function') continue;
      out.push({ name: d.name, aliases: d.aliases || [], kind: { mime: d.name.endsWith('.fn') ? 'fn/javascript' : 'application/javascript' },
        content: b64(new TextEncoder().encode(d.fn.toString())),
        dependencies: d.dependencies || [], permission: d.requiresTrust ? 1 : 0 });
    }
    const map = (await import('./js/redirect-resources.js')).default;
    for (const [name, det] of map) {
      const mime = MIMES[name.split('.').pop()];
      if (!mime) continue;
      try {
        const r = await fetch('./web_accessible_resources/' + name);
        if (!r.ok) continue;
        const bytes = new Uint8Array(await r.arrayBuffer());
        const aliases = det && det.alias ? (Array.isArray(det.alias) ? det.alias : [det.alias]) : [];
        out.push({ name, aliases, kind: { mime }, content: b64(bytes) });
      } catch (e) {}
    }
    EzooRes.done(JSON.stringify(out));
  } catch (e) {
    EzooRes.fail(String(e));
  }
})();
</script>"""
}
