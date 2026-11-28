package com.ezoo.download

import android.annotation.SuppressLint
import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Intent
import android.graphics.Bitmap
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.os.Message
import android.util.Base64
import android.view.View
import android.webkit.CookieManager
import android.webkit.JavascriptInterface
import android.webkit.URLUtil
import android.webkit.WebChromeClient
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebSettings
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.webkit.ScriptHandler
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import com.ezoo.download.engine.AdBlock
import org.json.JSONObject
import java.io.ByteArrayInputStream
import java.net.URLDecoder
import java.util.Locale

/** A file or stream seen on the page (a video it plays, a sound...). */
data class Media(val url: String, val kind: String, val referer: String)

/** Something the browser stopped, with a way to open it all the same. */
data class Notice(val text: String, val url: String?)

/**
 * The browser inside the app: one page at a time, the protection on every
 * request, and every download goes to the app's own downloads.
 */
@SuppressLint("SetJavaScriptEnabled")
class Browser(private val activity: Activity, private val onDownload: (Adding) -> Unit) {
    private val main = Handler(Looper.getMainLooper())

    var url by mutableStateOf("")
        private set
    var title by mutableStateOf("")
        private set
    var progress by mutableFloatStateOf(0f)
        private set
    var canBack by mutableStateOf(false)
        private set
    var canForward by mutableStateOf(false)
        private set
    /** Ads and trackers stopped on this page. */
    var blockedHere by mutableIntStateOf(0)
        private set
    var media by mutableStateOf(listOf<Media>())
        private set
    var notice by mutableStateOf<Notice?>(null)
    /** Full screen video (the page's own player). */
    var fullscreen by mutableStateOf<View?>(null)
        private set
    private var fullscreenDone: WebChromeClient.CustomViewCallback? = null

    @Volatile private var pageUrl = ""
    @Volatile private var pageHost: String? = null
    /** The page finished loading once (later page moves by themselves are watched). */
    @Volatile private var settled = false

    /** The bridge's name: different on every phone, so pages can't look for it. */
    private val bridge = "_" + (1..9).map { "abcdefghijklmnopqrstuvwxyz".random() }.joinToString("")
    private val docStart = WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT)
    private var startScript: ScriptHandler? = null
    private var startOrigin: String? = null

    val web: WebView = WebView(activity).apply {
        settings.apply {
            javaScriptEnabled = true
            domStorageEnabled = true
            databaseEnabled = true
            loadWithOverviewMode = true
            useWideViewPort = true
            builtInZoomControls = true
            displayZoomControls = false
            setSupportMultipleWindows(true)
            javaScriptCanOpenWindowsAutomatically = false
            mediaPlaybackRequiresUserGesture = true
            mixedContentMode = WebSettings.MIXED_CONTENT_COMPATIBILITY_MODE
            // the phone's own browser engine, as it is: without the marks of
            // an app's page view, which some sites refuse
            userAgentString = userAgentString.replace("; wv)", ")").replace(Regex(""" Version/\d+(\.\d+)*"""), "")
        }
        CookieManager.getInstance().setAcceptThirdPartyCookies(this, true)
        addJavascriptInterface(Bridge(), bridge)
        webViewClient = Client()
        webChromeClient = Chrome()
        setDownloadListener { link, agent, disposition, mime, length -> download(link, agent, disposition, mime, length) }
    }

    // ------------------------------------------------------------ moving around

    fun open(text: String) {
        val t = text.trim()
        if (t.isEmpty()) return
        val target = when {
            Regex("""^[a-zA-Z][a-zA-Z0-9+.-]*://""").containsMatchIn(t) -> t
            !t.contains(' ') && t.contains('.') -> "https://$t"
            else -> "https://www.google.com/search?q=" + Uri.encode(t)
        }
        go(target)
    }

    private fun go(target: String) {
        prepare(target)
        web.loadUrl(target)
    }

    fun back() {
        val list = web.copyBackForwardList()
        list.getItemAtIndex(list.currentIndex - 1)?.url?.let { prepare(it) }
        web.goBack()
    }

    fun forward() {
        val list = web.copyBackForwardList()
        list.getItemAtIndex(list.currentIndex + 1)?.url?.let { prepare(it) }
        web.goForward()
    }

    fun reload() {
        prepare(pageUrl)
        web.reload()
    }

    fun exitFullscreen() {
        fullscreenDone?.onCustomViewHidden()
    }

    fun pause() {
        web.onPause()
    }

    fun resume() {
        web.onResume()
    }

    /** The site of the page now, without "www." */
    val site: String? get() = pageHost?.removePrefix("www.")

    /** Turn the protection off (or on again) for this site, and reload. */
    fun toggleSite() {
        val h = pageHost ?: return
        Shield.allow(h, !Shield.isAllowed(h))
        reload()
    }

    // ------------------------------------------------------------ protection

    /**
     * Before a page starts: what it gets at its very start (the lists' page
     * fixes run before the page's own scripts, the ad boxes are hidden
     * before they show).
     */
    private fun prepare(target: String) {
        if (!docStart) return
        val origin = originOf(target)
        startScript?.remove()
        startScript = null
        startOrigin = null
        val host = hostOf(target)
        if (origin == null || !Shield.activeFor(host)) return
        try {
            startScript = WebViewCompat.addDocumentStartJavaScript(web, pageScript(target), setOf(origin))
            startOrigin = origin
        } catch (_: Exception) {
        }
    }

    private fun pageScript(target: String): String {
        val c = try {
            JSONObject(AdBlock.cosmetic(target))
        } catch (_: Exception) {
            JSONObject()
        }
        val css = JSONObject.quote(c.optString("css"))
        val script = c.optString("script")
        val generic = !c.optBoolean("generichide") && Shield.ads
        return """(function(){
var N=${JSONObject.quote(bridge)},B=window[N],U=${JSONObject.quote(target)};
function add(t){if(!t)return;try{var s=new CSSStyleSheet();s.replaceSync(t);document.adoptedStyleSheets=document.adoptedStyleSheets.concat([s]);}catch(e){try{var st=document.createElement('style');st.textContent=t;(document.head||document.documentElement).appendChild(st);}catch(e2){}}}
add($css);
try{
$script
}catch(e){}
if($generic&&B){var C=new Set(),I=new Set(),w=false;
function scan(){w=false;var c=[],i=[];var all=document.querySelectorAll('[class],[id]');for(var k=0;k<all.length&&k<20000;k++){var el=all[k];if(el.classList)for(var j=0;j<el.classList.length;j++){var x=el.classList[j];if(!C.has(x)){C.add(x);c.push(x);}}if(el.id&&!I.has(el.id)){I.add(el.id);i.push(el.id);}}if(c.length||i.length){try{add(B.g(U,c.join(' '),i.join(' ')));}catch(e){}}}
function later(){if(w)return;w=true;setTimeout(scan,250);}
if(document.readyState==='loading')document.addEventListener('DOMContentLoaded',scan);else scan();
try{new MutationObserver(later).observe(document.documentElement,{childList:true,subtree:true,attributes:true,attributeFilter:['class','id']});}catch(e){}}
})();"""
    }

    /** The bridge the page's start script asks (generic hiding). */
    private inner class Bridge {
        @JavascriptInterface
        fun g(page: String, classes: String, ids: String): String = AdBlock.generic(page, classes, ids)
    }

    private fun kindOf(r: WebResourceRequest): String {
        if (r.isForMainFrame) return "document"
        val accept = r.requestHeaders["Accept"] ?: r.requestHeaders["accept"] ?: ""
        val path = (r.url.path ?: "").lowercase(Locale.ROOT)
        val ext = path.substringAfterLast('.', "")
        return when {
            accept.contains("text/html") -> "sub_frame"
            ext == "js" || ext == "mjs" -> "script"
            ext == "css" || accept.startsWith("text/css") -> "stylesheet"
            ext in IMAGES || accept.startsWith("image/") -> "image"
            ext in FONTS -> "font"
            ext in MEDIA -> "media"
            accept.contains("application/json") -> "xmlhttprequest"
            accept == "*/*" -> "script"
            else -> "other"
        }
    }

    private fun stopped(kind: String): WebResourceResponse {
        // an empty answer of the kind asked: the page sees a file that is
        // there, not an error
        val (mime, body) = when (kind) {
            "script" -> "application/javascript" to ByteArray(0)
            "stylesheet" -> "text/css" to ByteArray(0)
            "image" -> "image/gif" to GIF
            "sub_frame" -> "text/html" to "<!doctype html><html><head></head><body></body></html>".toByteArray()
            "document" -> "text/html" to blockedPage().toByteArray()
            else -> "text/plain" to ByteArray(0)
        }
        return WebResourceResponse(mime, "utf-8", 200, "OK", mapOf("Access-Control-Allow-Origin" to "*", "Cache-Control" to "no-store"), ByteArrayInputStream(body))
    }

    private fun standIn(dataUrl: String): WebResourceResponse? {
        // data:<mime>[;base64],<data>
        val comma = dataUrl.indexOf(',')
        if (!dataUrl.startsWith("data:") || comma < 0) return null
        val head = dataUrl.substring(5, comma)
        val mime = head.substringBefore(';').ifEmpty { "text/plain" }
        val data = dataUrl.substring(comma + 1)
        val bytes = if (head.endsWith(";base64")) Base64.decode(data, Base64.DEFAULT) else URLDecoder.decode(data, "UTF-8").toByteArray()
        return WebResourceResponse(mime, "utf-8", 200, "OK", mapOf("Access-Control-Allow-Origin" to "*"), ByteArrayInputStream(bytes))
    }

    private fun blockedPage(): String = """<!doctype html><html dir="rtl"><head><meta name="viewport" content="width=device-width,initial-scale=1"><meta charset="utf-8"></head>
<body style="font-family:sans-serif;background:#f6f7fb;color:#151826;display:flex;align-items:center;justify-content:center;height:90vh;margin:0">
<div style="text-align:center;padding:24px"><div style="font-size:52px">🛡</div><h2>الصفحة دي إعلان واتمنعت</h2>
<p style="color:#7b80a3">Ezoo Download منع الصفحة دي علشان هي في قوائم الإعلانات.</p></div></body></html>"""

    // ------------------------------------------------------------ the page

    private inner class Client : WebViewClient() {
        override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse? {
            val u = request.url.toString()
            if (!u.startsWith("http")) return null
            val frame = request.requestHeaders["Referer"] ?: request.requestHeaders["referer"]
            val source = if (request.isForMainFrame) u else frame ?: pageUrl
            val host = if (request.isForMainFrame) request.url.host else pageHost
            if (!Shield.activeFor(host)) {
                sniff(u, frame)
                return null
            }
            val kind = kindOf(request)
            val r = AdBlock.check(u, source, kind)
            if (r.isEmpty()) {
                sniff(u, frame)
                return null
            }
            main.post {
                blockedHere++
                Shield.countAd()
            }
            return if (r.startsWith("R")) standIn(r.substring(1)) ?: stopped(kind) else stopped(kind)
        }

        override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
            val u = request.url
            val scheme = (u.scheme ?: "").lowercase(Locale.ROOT)
            if (scheme == "http" || scheme == "https") {
                if (request.isForMainFrame) {
                    // the page tries to take the user to another site by
                    // itself (no tap): the trick of ad pages
                    if (Shield.popups && settled && !request.hasGesture() && !request.isRedirect &&
                        !Shield.isAllowed(pageHost) && siteOf(u.host) != siteOf(pageHost)
                    ) {
                        Shield.countPopup()
                        notice = Notice("اتمنع تحويل لموقع تاني: ${u.host}", u.toString())
                        return true
                    }
                    prepare(u.toString())
                }
                return false
            }
            if (scheme in setOf("about", "data", "blob", "javascript")) return false
            // apps' addresses (store, phone, mail, intent...): only on a tap
            if (!request.hasGesture()) {
                Shield.countPopup()
                return true
            }
            openOutside(u.toString())
            return true
        }

        override fun onPageStarted(view: WebView, url: String, favicon: Bitmap?) {
            if (url != pageUrl) {
                val newPage = hostOf(url) != pageHost || url.substringBefore('#') != pageUrl.substringBefore('#')
                if (newPage) {
                    media = emptyList()
                    blockedHere = 0
                }
            }
            pageUrl = url
            pageHost = hostOf(url)
            this@Browser.url = url
            settled = false
            notice = null
            // a page this browser had no start script ready for (a server
            // sent it elsewhere): its fixes now, as early as it can
            if (Shield.activeFor(pageHost) && (!docStart || startOrigin != originOf(url))) {
                prepare(url)
                view.evaluateJavascript(pageScript(url), null)
            }
        }

        override fun onPageFinished(view: WebView, url: String) {
            settled = true
            progress = 1f
            canBack = view.canGoBack()
            canForward = view.canGoForward()
        }

        override fun doUpdateVisitedHistory(view: WebView, url: String, isReload: Boolean) {
            this@Browser.url = url
            pageUrl = url
            pageHost = hostOf(url)
            canBack = view.canGoBack()
            canForward = view.canGoForward()
        }
    }

    private inner class Chrome : WebChromeClient() {
        override fun onProgressChanged(view: WebView, newProgress: Int) {
            progress = newProgress / 100f
        }

        override fun onReceivedTitle(view: WebView, t: String?) {
            title = t ?: ""
        }

        override fun onShowCustomView(view: View, callback: CustomViewCallback) {
            fullscreen = view
            fullscreenDone = callback
        }

        override fun onHideCustomView() {
            fullscreen = null
            fullscreenDone = null
        }

        /**
         * A new window: a tapped link that opens one (target=_blank) opens
         * here; any other (pop-ups, pop-unders) is stopped, with a way to
         * open it.
         */
        override fun onCreateWindow(view: WebView, isDialog: Boolean, isUserGesture: Boolean, resultMsg: Message): Boolean {
            val hit = view.hitTestResult
            val link = hit.extra
            val tappedLink = isUserGesture && link != null && link.startsWith("http") &&
                (hit.type == WebView.HitTestResult.SRC_ANCHOR_TYPE || hit.type == WebView.HitTestResult.SRC_IMAGE_ANCHOR_TYPE)
            val free = !Shield.popups || Shield.isAllowed(pageHost)
            if (tappedLink && (free || AdBlock.check(link!!, pageUrl, "document").isEmpty())) {
                go(link!!)
                return false
            }
            // the address of the new window: a page view that only listens
            val catcher = WebView(activity)
            catcher.webViewClient = object : WebViewClient() {
                override fun shouldOverrideUrlLoading(v: WebView, r: WebResourceRequest): Boolean {
                    val target = r.url.toString()
                    main.post { catcher.destroy() }
                    if (free) {
                        go(target)
                    } else {
                        Shield.countPopup()
                        notice = Notice("اتمنعت نافذة منبثقة", target)
                    }
                    return true
                }
            }
            (resultMsg.obj as WebView.WebViewTransport).webView = catcher
            resultMsg.sendToTarget()
            main.postDelayed({ try { catcher.destroy() } catch (_: Exception) {} }, 10_000)
            return true
        }
    }

    /** Opens a stopped address when the user asks for it. */
    fun openNotice() {
        val n = notice?.url ?: return
        notice = null
        go(n)
    }

    private fun openOutside(link: String) {
        try {
            val i = if (link.startsWith("intent:")) Intent.parseUri(link, Intent.URI_INTENT_SCHEME) else Intent(Intent.ACTION_VIEW, Uri.parse(link))
            i.addCategory(Intent.CATEGORY_BROWSABLE)
            i.component = null
            i.selector = null
            activity.startActivity(i)
        } catch (_: ActivityNotFoundException) {
            // an intent: address may carry a page for phones without the app
            try {
                val fallback = Intent.parseUri(link, Intent.URI_INTENT_SCHEME).getStringExtra("browser_fallback_url")
                if (fallback != null && fallback.startsWith("http")) go(fallback)
            } catch (_: Exception) {
            }
        } catch (_: Exception) {
        }
    }

    // ------------------------------------------------------------ downloads

    private fun download(link: String, agent: String?, disposition: String?, mime: String?, length: Long) {
        if (!link.startsWith("http")) {
            notice = Notice("الملف ده بيتعمل جوه الصفحة نفسها ومينفعش يتحمل من بره", null)
            return
        }
        val name = URLUtil.guessFileName(link, disposition, mime)
        onDownload(
            Adding(
                link = link,
                fromClipboard = false,
                referer = pageUrl.ifEmpty { null },
                cookies = CookieManager.getInstance().getCookie(link),
                userAgent = agent ?: web.settings.userAgentString,
                fileName = name,
                size = length.takeIf { it > 0 },
                dialog = true,
            )
        )
    }

    /** What the page plays: videos, sounds, streams. */
    private fun sniff(u: String, frame: String?) {
        val path = u.substringBefore('?').substringBefore('#').lowercase(Locale.ROOT)
        val ext = path.substringAfterLast('.', "")
        if (ext !in SNIFF) return
        // a stream's small pieces, or YouTube's (its page is the video)
        if (u.contains("googlevideo.com") || u.contains("/seg-") || u.contains("range=")) return
        val referer = frame ?: pageUrl
        main.post {
            if (media.none { it.url == u } && media.size < 40) media = media + Media(u, ext, referer)
        }
    }

    /** The page is a video page (yt-dlp knows the site). */
    fun isVideoPage(): Boolean = Video.isVideoSite(url) && (Uri.parse(url).path ?: "/").length > 1

    /** The add sheet for this page's video and files. */
    fun videoSheet(): Adding = Adding(
        link = url,
        fromClipboard = false,
        referer = url,
        cookies = CookieManager.getInstance().getCookie(url),
        userAgent = web.settings.userAgentString,
        videoPage = true,
        media = media,
    )

    fun destroy() {
        Shield.flush()
        web.destroy()
    }

    companion object {
        private val IMAGES = setOf("png", "jpg", "jpeg", "gif", "webp", "svg", "ico", "bmp", "avif")
        private val FONTS = setOf("woff", "woff2", "ttf", "otf", "eot")
        private val MEDIA = setOf("mp4", "webm", "m3u8", "mp3", "m4a", "ogg", "ts", "mpd")
        private val SNIFF = setOf("mp4", "webm", "m3u8", "mpd", "m4v", "mov", "mkv", "mp3", "m4a", "aac", "flac", "ogg", "opus", "wav")
        private val GIF = Base64.decode("R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7", Base64.DEFAULT)

        fun hostOf(u: String): String? = try {
            Uri.parse(u).host?.lowercase(Locale.ROOT)
        } catch (_: Exception) {
            null
        }

        fun originOf(u: String): String? = try {
            val p = Uri.parse(u)
            val s = p.scheme?.lowercase(Locale.ROOT)
            if ((s == "http" || s == "https") && p.host != null) {
                val port = if (p.port > 0) ":${p.port}" else ""
                "$s://${p.host!!.lowercase(Locale.ROOT)}$port"
            } else null
        } catch (_: Exception) {
            null
        }

        /** The site of a host: "news.example.co.uk" -> "example.co.uk". */
        fun siteOf(host: String?): String? {
            val h = host?.lowercase(Locale.ROOT)?.trimEnd('.') ?: return null
            val parts = h.split('.')
            if (parts.size <= 2) return h
            val second = parts[parts.size - 2]
            val keep = if (parts.last().length == 2 && second in setOf("com", "co", "net", "org", "gov", "edu", "ac", "or", "ne", "go")) 3 else 2
            return parts.takeLast(keep).joinToString(".")
        }
    }
}
