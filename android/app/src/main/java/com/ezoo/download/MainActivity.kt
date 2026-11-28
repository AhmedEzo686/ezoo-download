package com.ezoo.download

import android.Manifest
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextDirection
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.ContextCompat
import com.ezoo.download.engine.Download
import com.ezoo.download.engine.Engine
import kotlinx.coroutines.delay

class MainActivity : ComponentActivity() {
    /** A link shared to the app (or opened with it): the add sheet shows it. */
    private var shared by mutableStateOf<String?>(null)

    private val askPermissions =
        registerForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { got ->
            if (got[Manifest.permission.WRITE_EXTERNAL_STORAGE] == false) {
                Toast.makeText(this, "اسمح بالتخزين علشان الملفات تتحفظ في Download/Ezoo", Toast.LENGTH_LONG).show()
            }
        }

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.light(android.graphics.Color.WHITE, android.graphics.Color.WHITE),
            navigationBarStyle = SystemBarStyle.light(android.graphics.Color.WHITE, android.graphics.Color.WHITE),
        )
        super.onCreate(savedInstanceState)
        askWhatIsNeeded()
        shared = linkOf(intent)
        setContent {
            EzooTheme {
                Shell(shared = shared, onSharedTaken = { shared = null })
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        linkOf(intent)?.let { shared = it }
    }

    private fun linkOf(i: Intent?): String? = when (i?.action) {
        Intent.ACTION_SEND -> Files.findLink(i.getStringExtra(Intent.EXTRA_TEXT))
        Intent.ACTION_VIEW -> i.dataString
        else -> null
    }

    private fun askWhatIsNeeded() {
        val want = mutableListOf<String>()
        if (Build.VERSION.SDK_INT <= 28) want += Manifest.permission.WRITE_EXTERNAL_STORAGE
        if (Build.VERSION.SDK_INT >= 33) want += Manifest.permission.POST_NOTIFICATIONS
        val missing = want.filter { ContextCompat.checkSelfPermission(this, it) != PackageManager.PERMISSION_GRANTED }
        if (missing.isNotEmpty()) askPermissions.launch(missing.toTypedArray())
    }
}

private enum class Page(val icon: String, val label: String) {
    Downloads("⬇️", "التحميلات"), Browser("🌐", "المتصفح"), Torrent("🧲", "تورنت"), Settings("⚙️", "الإعدادات")
}

/** A link the add sheet opens with, and whether it came from the clipboard. */
data class Adding(val link: String, val fromClipboard: Boolean)

@Composable
private fun Shell(shared: String?, onSharedTaken: () -> Unit) {
    val ctx = LocalContext.current
    var page by remember { mutableStateOf(Page.Downloads) }
    var list by remember { mutableStateOf(emptyList<Download>()) }
    var adding by remember { mutableStateOf<Adding?>(null) }

    // the list, fresh every half second while the screen shows
    LaunchedEffect(Unit) {
        while (true) {
            list = Download.parseList(Engine.list())
            delay(500)
        }
    }
    LaunchedEffect(shared) {
        if (shared != null) {
            adding = Adding(shared, false)
            page = Page.Downloads
            onSharedTaken()
        }
    }
    EzooApp.engineError?.let { err ->
        LaunchedEffect(err) {
            Toast.makeText(ctx, "محرك التحميل ماشتغلش: $err", Toast.LENGTH_LONG).show()
        }
    }
    BackHandler(enabled = page != Page.Downloads) { page = Page.Downloads }

    Column(Modifier.fillMaxSize().background(Ui.Page)) {
        Box(Modifier.weight(1f)) {
            when (page) {
                Page.Downloads -> DownloadsScreen(list, torrentsOnly = false, onSettings = { page = Page.Settings })
                Page.Torrent -> DownloadsScreen(list, torrentsOnly = true, onSettings = { page = Page.Settings })
                Page.Browser -> SoonScreen()
                Page.Settings -> SettingsScreen()
            }
            if (page == Page.Downloads || page == Page.Torrent) {
                Box(
                    Modifier
                        .align(Alignment.BottomEnd)
                        .padding(22.dp)
                        .size(64.dp)
                        .shadow(10.dp, RoundedCornerShape(20.dp), ambientColor = Ui.Violet, spotColor = Ui.Violet)
                        .clip(RoundedCornerShape(20.dp))
                        .background(Ui.Gradient)
                        .clickable { adding = Adding(clipboardLink(ctx) ?: "", clipboardLink(ctx) != null) },
                    contentAlignment = Alignment.Center,
                ) { Text("+", color = Color.White, fontSize = 34.sp, fontWeight = FontWeight.Bold) }
            }
        }
        BottomBar(page) { page = it }
    }

    adding?.let { a -> AddSheet(first = a, onClose = { adding = null }) }
}

@Composable
private fun BottomBar(page: Page, onPick: (Page) -> Unit) {
    Column(Modifier.fillMaxWidth().background(Color.White)) {
        Box(Modifier.fillMaxWidth().height(1.dp).background(Ui.Line))
        Row(
            Modifier.fillMaxWidth().navigationBarsPadding().height(72.dp),
            horizontalArrangement = Arrangement.SpaceAround,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Page.entries.forEach { p ->
                val on = p == page
                Column(
                    Modifier.weight(1f).fillMaxHeight().clickable { onPick(p) },
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.Center,
                ) {
                    Text(p.icon, fontSize = 22.sp)
                    Text(p.label, fontSize = 13.sp, fontWeight = FontWeight.Bold, color = if (on) Ui.Violet else Ui.Muted)
                }
            }
        }
    }
}

@Composable
fun TopBar(title: String, actions: @Composable () -> Unit = {}) {
    Column(Modifier.fillMaxWidth().background(Color.White).statusBarsPadding()) {
        Row(
            Modifier.fillMaxWidth().height(62.dp).padding(horizontal = 18.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(title, fontWeight = FontWeight.ExtraBold, fontSize = 22.sp, color = Ui.Ink, modifier = Modifier.weight(1f))
            actions()
        }
        Box(Modifier.fillMaxWidth().height(1.dp).background(Ui.Line))
    }
}

@Composable
fun SquareButton(onClick: () -> Unit, content: @Composable () -> Unit) {
    Box(
        Modifier.size(38.dp).clip(RoundedCornerShape(12.dp)).background(Ui.Soft).clickable(onClick = onClick),
        contentAlignment = Alignment.Center,
    ) { content() }
}

private enum class Tab(val label: String) { All("الكل"), Running("شغال"), Done("خلص"), Video("فيديو"), Torrent("تورنت") }

@Composable
private fun DownloadsScreen(list: List<Download>, torrentsOnly: Boolean, onSettings: () -> Unit) {
    var tab by remember { mutableStateOf(Tab.All) }
    var searching by remember { mutableStateOf(false) }
    var query by remember { mutableStateOf("") }
    var menu by remember { mutableStateOf(false) }
    val ctx = LocalContext.current
    BackHandler(enabled = searching) { searching = false; query = "" }

    Column(Modifier.fillMaxSize()) {
        TopBar(if (torrentsOnly) "تورنت" else "التحميلات") {
            SquareButton({ searching = !searching; if (!searching) query = "" }) { Text("🔍", fontSize = 18.sp) }
            Box {
                SquareButton({ menu = true }) { Text("⚙️", fontSize = 18.sp) }
                DropdownMenu(expanded = menu, onDismissRequest = { menu = false }, containerColor = Color.White) {
                    DropdownMenuItem(text = { Text("استكمال الكل") }, onClick = {
                        menu = false
                        Engine.startAll()
                        DownloadService.start(ctx)
                    })
                    DropdownMenuItem(text = { Text("إيقاف الكل") }, onClick = { menu = false; Engine.pauseAll() })
                    DropdownMenuItem(text = { Text("الإعدادات") }, onClick = { menu = false; onSettings() })
                }
            }
        }
        if (searching) {
            Field(query, { query = it }, placeholder = "دور باسم الملف", ltr = false, modifier = Modifier.padding(horizontal = 14.dp, vertical = 8.dp))
        }
        if (!torrentsOnly) {
            Row(
                Modifier.horizontalScroll(rememberScrollState()).padding(start = 14.dp, end = 14.dp, top = 12.dp, bottom = 6.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Tab.entries.forEach { t ->
                    val on = t == tab
                    Text(
                        t.label,
                        modifier = Modifier
                            .clip(RoundedCornerShape(20.dp))
                            .background(if (on) Ui.Violet else Ui.Chip)
                            .clickable { tab = t }
                            .padding(horizontal = 16.dp, vertical = 5.dp),
                        color = if (on) Color.White else Ui.Label,
                        fontWeight = FontWeight.Bold,
                        fontSize = 15.sp,
                    )
                }
            }
        }
        val shown = list.filter {
            val inTab = if (torrentsOnly) it.torrent else when (tab) {
                Tab.All -> true
                Tab.Running -> it.running || it.queued
                Tab.Done -> it.completed
                Tab.Video -> it.category == "Video"
                Tab.Torrent -> it.torrent
            }
            inTab && (query.isBlank() || it.name.contains(query.trim(), ignoreCase = true))
        }
        if (shown.isEmpty()) {
            Box(Modifier.fillMaxSize().padding(32.dp), contentAlignment = Alignment.Center) {
                Text(
                    if (torrentsOnly) "مفيش تورنت لسه.\nشارك رابط magnet لـ Ezoo، أو دوس + والصقه."
                    else "مفيش تحميلات لسه.\nدوس + أو شارك أي رابط لـ Ezoo من أي تطبيق.",
                    color = Ui.Muted, textAlign = TextAlign.Center, fontSize = 15.sp,
                )
            }
        } else {
            LazyColumn(contentPadding = PaddingValues(top = 4.dp, bottom = 100.dp)) {
                items(shown, key = { it.id }) { d -> DownloadRow(d) }
            }
        }
    }
}

fun clipboardLink(ctx: Context): String? {
    val cm = ctx.getSystemService(ClipboardManager::class.java) ?: return null
    val clip = try { cm.primaryClip } catch (_: Exception) { null } ?: return null
    if (clip.itemCount == 0) return null
    return Files.findLink(clip.getItemAt(0).coerceToText(ctx)?.toString())
}

fun kindIcon(category: String, torrent: Boolean = false): Pair<String, Color> = when {
    torrent -> "🧲" to Color(0xFFECFDF3)
    category == "Video" -> "🎬" to Color(0xFFFDE7E7)
    category == "Music" -> "🎵" to Color(0xFFFFF4E0)
    category == "Compressed" -> "📦" to Color(0xFFE6F0FF)
    category == "Programs" -> "💿" to Color(0xFFECFDF3)
    category == "Documents" -> "📄" to Color(0xFFE6F0FF)
    category == "Images" -> "🖼️" to Color(0xFFF2ECFF)
    else -> "📁" to Color(0xFFF0F1F8)
}

fun kindName(category: String): String = when (category) {
    "Video" -> "فيديو"
    "Music" -> "صوت"
    "Compressed" -> "مضغوط"
    "Programs" -> "برامج"
    "Documents" -> "مستندات"
    "Images" -> "صور"
    else -> "عام"
}

private fun statusText(d: Download): String = when {
    d.waiting -> "مستني النت يرجع…"
    d.completed -> "✓ اتحمل · ${Format.size(d.total ?: d.done)} · ${kindName(d.category)}"
    d.failed -> "فشل · " + (d.error ?: "")
    d.status == "Downloading" && d.torrent ->
        listOfNotNull("تورنت", "${d.connections} مصدر".takeIf { d.connections > 0 }, Format.speed(d.speed).takeIf { d.speed > 0 })
            .joinToString(" · ")
    d.status == "Downloading" -> Format.progressLine(d.done, d.total, d.speed, d.eta)
    d.status == "Connecting" -> "بيتصل…"
    d.status == "Queued" -> "في الانتظار"
    d.status == "Assembling" -> "بيجهز الملف…"
    else -> "⏸ متوقف" + if (d.done > 0) " · " + Format.progressLine(d.done, d.total, 0, null) else ""
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun DownloadRow(d: Download) {
    val ctx = LocalContext.current
    var menu by remember { mutableStateOf(false) }
    fun open() {
        val i = Files.openIntent(ctx, d.path)
        if (i == null) {
            Toast.makeText(ctx, "الملف مش موجود", Toast.LENGTH_SHORT).show()
            return
        }
        try {
            ctx.startActivity(Intent.createChooser(i, null).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        } catch (_: Exception) {
            Toast.makeText(ctx, "مفيش تطبيق يفتح الملف ده", Toast.LENGTH_SHORT).show()
        }
    }
    fun resume() {
        if (d.failed) Engine.restart(d.id) else Engine.start(d.id)
        DownloadService.start(ctx)
    }
    Box(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 12.dp, vertical = 6.dp)
            .shadow(1.dp, RoundedCornerShape(18.dp))
            .clip(RoundedCornerShape(18.dp))
            .background(Color.White)
            .combinedClickable(onClick = { if (d.completed) open() else { menu = true } }, onLongClick = { menu = true }),
    ) {
        Row(Modifier.padding(horizontal = 14.dp, vertical = 12.dp), verticalAlignment = Alignment.CenterVertically) {
            val (emoji, bg) = kindIcon(d.category, d.torrent)
            Box(Modifier.size(46.dp).clip(RoundedCornerShape(13.dp)).background(bg), contentAlignment = Alignment.Center) {
                Text(emoji, fontSize = 22.sp)
            }
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                Text(
                    d.name,
                    fontWeight = FontWeight.Bold,
                    fontSize = 16.sp,
                    color = Ui.Ink,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    style = TextStyle(textDirection = TextDirection.Content),
                )
                Text(
                    statusText(d),
                    color = if (d.failed) Ui.Red else Ui.Muted,
                    fontSize = 13.sp,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
                if (!d.completed) {
                    Spacer(Modifier.height(7.dp))
                    ProgressBar(d)
                }
            }
            Spacer(Modifier.width(12.dp))
            SquareButton({
                when {
                    d.running || d.queued -> Engine.pause(d.id)
                    d.completed -> open()
                    else -> resume()
                }
            }) {
                when {
                    d.running || d.queued -> PauseGlyph()
                    d.completed -> Text("↗", fontSize = 18.sp, color = Ui.Ink, fontWeight = FontWeight.Bold)
                    d.failed -> Icon(Icons.Filled.Refresh, "حاول تاني", tint = Ui.Ink)
                    else -> Icon(Icons.Filled.PlayArrow, "استكمال", tint = Ui.Ink)
                }
            }
        }
        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }, containerColor = Color.White) {
            if (d.completed) DropdownMenuItem(text = { Text("فتح") }, onClick = { menu = false; open() })
            if (d.running || d.queued) DropdownMenuItem(text = { Text("إيقاف") }, onClick = { menu = false; Engine.pause(d.id) })
            if (d.paused) DropdownMenuItem(text = { Text("استكمال") }, onClick = { menu = false; resume() })
            if (d.failed || d.completed) DropdownMenuItem(text = { Text("حمّل تاني") }, onClick = {
                menu = false
                Engine.restart(d.id)
                DownloadService.start(ctx)
            })
            if (d.failed && d.error != null) DropdownMenuItem(text = { Text("السبب: ${d.error}", fontSize = 13.sp, color = Ui.Muted) }, onClick = { menu = false })
            DropdownMenuItem(text = { Text("حذف من القائمة") }, onClick = { menu = false; Engine.remove(d.id, false) })
            DropdownMenuItem(text = { Text("حذف مع الملف", color = Ui.Red) }, onClick = { menu = false; Engine.remove(d.id, true) })
        }
    }
}

@Composable
private fun ProgressBar(d: Download) {
    val unknown = (d.running && (d.total == null || d.total == 0L)) || d.status == "Connecting"
    Box(Modifier.fillMaxWidth().height(6.dp).clip(RoundedCornerShape(4.dp)).background(Ui.Track)) {
        val frac = if (unknown) 0.15f else d.progress
        if (frac > 0f) {
            Box(
                Modifier
                    .fillMaxWidth(frac)
                    .fillMaxHeight()
                    .clip(RoundedCornerShape(4.dp))
                    .then(if (d.running || d.queued) Modifier.background(Ui.Bar) else Modifier.background(Color(0xFFA9ADC9))),
            )
        }
    }
}

@Composable
private fun PauseGlyph() {
    Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
        Box(Modifier.size(width = 4.dp, height = 14.dp).background(Ui.Ink, RoundedCornerShape(1.dp)))
        Box(Modifier.size(width = 4.dp, height = 14.dp).background(Ui.Ink, RoundedCornerShape(1.dp)))
    }
}

/** A text box like the sketches': light, rounded, a thin line around. */
@Composable
fun Field(
    value: String,
    onChange: (String) -> Unit,
    modifier: Modifier = Modifier,
    placeholder: String = "",
    ltr: Boolean = true,
    trailing: @Composable (() -> Unit)? = null,
) {
    Row(
        modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(12.dp))
            .background(Ui.Field)
            .border(1.dp, Ui.FieldLine, RoundedCornerShape(12.dp))
            .padding(start = 12.dp, end = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.weight(1f).padding(vertical = 11.dp)) {
            if (value.isEmpty() && placeholder.isNotEmpty()) Text(placeholder, color = Ui.Muted, fontSize = 15.sp)
            BasicTextField(
                value = value,
                onValueChange = onChange,
                singleLine = true,
                textStyle = TextStyle(
                    fontSize = 15.sp, color = Ui.Ink, fontFamily = Ui.Cairo,
                    textDirection = if (ltr) TextDirection.Ltr else TextDirection.Content,
                ),
                modifier = Modifier.fillMaxWidth(),
            )
        }
        trailing?.invoke()
    }
}

@Composable
private fun SoonScreen() {
    Column(Modifier.fillMaxSize()) {
        TopBar("المتصفح")
        Box(Modifier.fillMaxSize().padding(32.dp), contentAlignment = Alignment.Center) {
            Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Text("🌐", fontSize = 48.sp)
                Text("المتصفح الداخلي جاي في التحديث الجاي", fontWeight = FontWeight.ExtraBold, fontSize = 18.sp, color = Ui.Ink, textAlign = TextAlign.Center)
                Text(
                    "متصفح من غير إعلانات، بيلقط الفيديوهات والملفات من أي صفحة.\nلحد ما ييجي: شارك الرابط لـ Ezoo من أي تطبيق.",
                    color = Ui.Muted, textAlign = TextAlign.Center, fontSize = 15.sp,
                )
            }
        }
    }
}

@Composable
private fun SettingsScreen() {
    val ctx = LocalContext.current
    var ytdlp by remember { mutableStateOf("…") }
    LaunchedEffect(Unit) {
        ytdlp = kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) { Video.ytdlpVersion() }
    }
    val version = try {
        ctx.packageManager.getPackageInfo(ctx.packageName, 0).versionName ?: ""
    } catch (_: Exception) {
        ""
    }
    Column(Modifier.fillMaxSize()) {
        TopBar("الإعدادات")
        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            SettingCard("📁", "مكان الحفظ", "Download / Ezoo — كل نوع في فولدر (Video، Music، Programs…)")
            SettingCard("⚡", "الاتصالات", "لحد 16 اتصال للملف الواحد، وتقدر تغيرها في كل تحميل")
            SettingCard("🎬", "الفيديوهات", "yt-dlp $ytdlp")
            SettingCard("ℹ️", "Ezoo Download", "الإصدار $version · نسخة تجريبية")
        }
    }
}

@Composable
private fun SettingCard(icon: String, title: String, sub: String) {
    Row(
        Modifier
            .fillMaxWidth()
            .shadow(1.dp, RoundedCornerShape(18.dp))
            .clip(RoundedCornerShape(18.dp))
            .background(Color.White)
            .padding(14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.size(46.dp).clip(RoundedCornerShape(13.dp)).background(Ui.Soft), contentAlignment = Alignment.Center) {
            Text(icon, fontSize = 22.sp)
        }
        Spacer(Modifier.width(12.dp))
        Column {
            Text(title, fontWeight = FontWeight.Bold, fontSize = 16.sp, color = Ui.Ink)
            Text(sub, color = Ui.Muted, fontSize = 13.sp)
        }
    }
}
