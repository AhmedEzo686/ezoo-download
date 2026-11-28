package com.ezoo.download

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Intent
import android.net.Uri
import android.view.ViewGroup
import android.widget.Toast
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextDirection
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

@Composable
fun BrowserScreen(b: Browser, onShieldSettings: () -> Unit, onAdd: (Adding) -> Unit) {
    val ctx = LocalContext.current
    var editing by remember { mutableStateOf(false) }
    var typed by remember { mutableStateOf("") }
    var menu by remember { mutableStateOf(false) }
    var shieldMenu by remember { mutableStateOf(false) }
    val focus = LocalFocusManager.current

    BackHandler(enabled = b.canBack && b.fullscreen == null) { b.back() }

    Column(Modifier.fillMaxSize()) {
        // the address bar: the page's protection, its address, the menu
        Column(Modifier.fillMaxWidth().background(Color.White).statusBarsPadding()) {
            Row(
                Modifier.fillMaxWidth().height(62.dp).padding(horizontal = 12.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                val off = Shield.isAllowed(b.site) || !Shield.ready
                Box {
                    Row(
                        Modifier
                            .clip(RoundedCornerShape(12.dp))
                            .background(if (off) Ui.Soft else Color(0xFFE8F8EE))
                            .clickable { shieldMenu = true }
                            .padding(horizontal = 10.dp, vertical = 8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(
                            if (off) "🛡" else "${b.blockedHere} 🛡",
                            color = if (off) Ui.Muted else Ui.Green, fontWeight = FontWeight.ExtraBold, fontSize = 15.sp,
                        )
                    }
                    DropdownMenu(expanded = shieldMenu, onDismissRequest = { shieldMenu = false }, containerColor = Color.White) {
                        Text(
                            when {
                                !Shield.ready -> "قوائم الحماية لسه بتتجهز…"
                                b.site == null -> "منع الإعلانات شغال"
                                Shield.isAllowed(b.site) -> "منع الإعلانات متوقف في ${b.site}"
                                else -> "اتمنع ${b.blockedHere} إعلان ومتتبع في الصفحة دي"
                            },
                            modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp), color = Ui.Label, fontSize = 14.sp,
                        )
                        if (b.site != null) {
                            DropdownMenuItem(
                                text = { Text(if (Shield.isAllowed(b.site)) "شغّل منع الإعلانات في الموقع ده" else "وقّف منع الإعلانات في الموقع ده") },
                                onClick = { shieldMenu = false; b.toggleSite() },
                            )
                        }
                        DropdownMenuItem(text = { Text("إعدادات منع الإعلانات") }, onClick = { shieldMenu = false; onShieldSettings() })
                    }
                }
                Row(
                    Modifier
                        .weight(1f)
                        .height(42.dp)
                        .clip(RoundedCornerShape(12.dp))
                        .background(Ui.Soft)
                        .padding(horizontal = 12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    if (!editing && b.url.startsWith("https")) Text("🔒 ", fontSize = 13.sp)
                    BasicTextField(
                        value = if (editing) typed else b.url.removePrefix("https://").removePrefix("http://").removeSuffix("/"),
                        onValueChange = { typed = it },
                        singleLine = true,
                        textStyle = TextStyle(fontSize = 15.sp, color = Ui.Ink, fontFamily = Ui.Cairo, textDirection = TextDirection.Ltr),
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Go),
                        keyboardActions = KeyboardActions(onGo = {
                            b.open(typed)
                            editing = false
                            focus.clearFocus()
                        }),
                        modifier = Modifier.weight(1f).onFocusChanged {
                            if (it.isFocused && !editing) {
                                editing = true
                                typed = b.url
                            } else if (!it.isFocused) {
                                editing = false
                            }
                        },
                        decorationBox = { inner ->
                            Box(contentAlignment = Alignment.CenterStart) {
                                if (!editing && b.url.isEmpty()) Text("ابحث أو اكتب عنوان موقع", color = Ui.Muted, fontSize = 15.sp)
                                inner()
                            }
                        },
                    )
                }
                Box {
                    SquareButton({ menu = true }) { Text("☰", fontSize = 18.sp, color = Ui.Ink) }
                    DropdownMenu(expanded = menu, onDismissRequest = { menu = false }, containerColor = Color.White) {
                        if (b.canBack) DropdownMenuItem(text = { Text("رجوع") }, onClick = { menu = false; b.back() })
                        if (b.canForward) DropdownMenuItem(text = { Text("قدام") }, onClick = { menu = false; b.forward() })
                        if (b.url.isNotEmpty()) {
                            DropdownMenuItem(text = { Text("تحديث الصفحة") }, onClick = { menu = false; b.reload() })
                            DropdownMenuItem(text = { Text("نسخ الرابط") }, onClick = {
                                menu = false
                                ctx.getSystemService(ClipboardManager::class.java)?.setPrimaryClip(ClipData.newPlainText("link", b.url))
                                Toast.makeText(ctx, "اتنسخ", Toast.LENGTH_SHORT).show()
                            })
                            DropdownMenuItem(text = { Text("حمّل من الصفحة دي") }, onClick = { menu = false; onAdd(b.videoSheet()) })
                            DropdownMenuItem(text = { Text("افتح في متصفح تاني") }, onClick = {
                                menu = false
                                try {
                                    ctx.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(b.url)))
                                } catch (_: Exception) {
                                }
                            })
                        }
                        DropdownMenuItem(text = { Text("إعدادات منع الإعلانات") }, onClick = { menu = false; onShieldSettings() })
                    }
                }
            }
            // how far the page is
            Box(Modifier.fillMaxWidth().height(2.dp).background(Ui.Line)) {
                if (b.progress in 0.01f..0.99f) {
                    Box(Modifier.fillMaxWidth(b.progress).fillMaxHeight().background(Ui.Bar))
                }
            }
        }

        Box(Modifier.weight(1f).fillMaxWidth()) {
            AndroidView(
                factory = {
                    (b.web.parent as? ViewGroup)?.removeView(b.web)
                    b.web
                },
                modifier = Modifier.fillMaxSize(),
            )
            if (b.url.isEmpty()) StartPage(b)

            // what the page has to download
            val files = b.media.size
            if (b.url.isNotEmpty() && (files > 0 || b.isVideoPage())) {
                Row(
                    Modifier
                        .align(Alignment.BottomEnd)
                        .padding(18.dp)
                        .shadow(10.dp, RoundedCornerShape(18.dp), ambientColor = Ui.Violet, spotColor = Ui.Violet)
                        .clip(RoundedCornerShape(18.dp))
                        .background(Ui.Gradient)
                        .clickable { onAdd(b.videoSheet()) }
                        .padding(horizontal = 18.dp, vertical = 12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        if (b.isVideoPage()) "⬇ حمّل الفيديو" else "⬇ $files ${if (files == 1) "ملف" else "ملفات"} في الصفحة",
                        color = Color.White, fontWeight = FontWeight.ExtraBold, fontSize = 15.sp,
                    )
                }
            }

            // something the browser stopped
            b.notice?.let { n ->
                Row(
                    Modifier
                        .align(Alignment.BottomCenter)
                        .padding(start = 12.dp, end = 12.dp, bottom = 84.dp)
                        .fillMaxWidth()
                        .shadow(6.dp, RoundedCornerShape(14.dp))
                        .clip(RoundedCornerShape(14.dp))
                        .background(Ui.Ink)
                        .padding(horizontal = 14.dp, vertical = 10.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text("🛡 " + n.text, color = Color.White, fontSize = 14.sp, modifier = Modifier.weight(1f), maxLines = 2, overflow = TextOverflow.Ellipsis)
                    if (n.url != null) {
                        Text("افتح", color = Color(0xFFB9A8FF), fontWeight = FontWeight.ExtraBold, modifier = Modifier.clickable { b.openNotice() }.padding(8.dp))
                    }
                    Text("✕", color = Color.White, modifier = Modifier.clickable { b.notice = null }.padding(8.dp))
                }
            }
        }
    }
}

@Composable
private fun StartPage(b: Browser) {
    var q by remember { mutableStateOf("") }
    Column(
        Modifier.fillMaxSize().background(Ui.Page).verticalScroll(rememberScrollState()).padding(22.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Spacer(Modifier.height(30.dp))
        Box(Modifier.size(72.dp).clip(RoundedCornerShape(20.dp)).background(Ui.Gradient), contentAlignment = Alignment.Center) {
            Text("E", color = Color.White, fontWeight = FontWeight.ExtraBold, fontSize = 36.sp)
        }
        Text("متصفح Ezoo", fontWeight = FontWeight.ExtraBold, fontSize = 22.sp, color = Ui.Ink)
        Text("من غير إعلانات، ونوافذ منبثقة، وتتبع · وأي ملف أو فيديو تقدر تحمله", color = Ui.Muted, fontSize = 14.sp, textAlign = TextAlign.Center)
        Field(q, { q = it }, placeholder = "ابحث أو اكتب عنوان موقع", trailing = {
            Text(
                "روح",
                color = Ui.Violet, fontWeight = FontWeight.Bold, fontSize = 14.sp,
                modifier = Modifier.clip(RoundedCornerShape(8.dp)).clickable { b.open(q) }.padding(horizontal = 10.dp, vertical = 6.dp),
            )
        })
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            Stat("${Shield.adsBlocked}", "إعلان ومتتبع اتمنع", Modifier.weight(1f))
            Stat("${Shield.popupsBlocked}", "نافذة وتحويل اتمنع", Modifier.weight(1f))
        }
    }
}

@Composable
private fun Stat(n: String, label: String, modifier: Modifier = Modifier) {
    Column(
        modifier
            .shadow(1.dp, RoundedCornerShape(18.dp))
            .clip(RoundedCornerShape(18.dp))
            .background(Color.White)
            .padding(vertical = 16.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(n, color = Ui.Violet, fontWeight = FontWeight.ExtraBold, fontSize = 26.sp)
        Text(label, color = Ui.Muted, fontSize = 13.sp)
    }
}

/** The protection's settings (as in the sketches). */
@Composable
fun ShieldSettings(onBack: () -> Unit) {
    val ctx = LocalContext.current
    var sites by remember { mutableStateOf(false) }
    BackHandler { if (sites) sites = false else onBack() }
    Column(Modifier.fillMaxSize()) {
        TopBar(if (sites) "المواقع المسموحة" else "منع الإعلانات") {
            SquareButton({ if (sites) sites = false else onBack() }) { Text("→", fontSize = 18.sp, color = Ui.Ink, fontWeight = FontWeight.Bold) }
        }
        Column(
            Modifier.verticalScroll(rememberScrollState()).padding(12.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            if (sites) {
                if (Shield.allowed.isEmpty()) {
                    Text(
                        "مفيش مواقع مسموحة.\nمن المتصفح: دوس على 🛡 واختار \"وقّف منع الإعلانات في الموقع ده\".",
                        color = Ui.Muted, textAlign = TextAlign.Center, modifier = Modifier.fillMaxWidth().padding(24.dp),
                    )
                }
                Shield.allowed.sorted().forEach { s ->
                    Card {
                        Text(s, fontWeight = FontWeight.Bold, color = Ui.Ink, modifier = Modifier.weight(1f), style = TextStyle(textDirection = TextDirection.Ltr))
                        Text("شيل", color = Ui.Red, fontWeight = FontWeight.Bold, modifier = Modifier.clickable { Shield.allow(s, false) }.padding(8.dp))
                    }
                }
                return@Column
            }
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                Stat("%,d".format(Shield.adsBlocked), "إعلان اتمنع", Modifier.weight(1f))
                Stat("%,d".format(Shield.popupsBlocked), "نافذة منبثقة", Modifier.weight(1f))
            }
            Toggle("منع الإعلانات", "قوائم مفتوحة بتتحدّث لوحدها", Shield.ads) { Shield.set("ads", it) }
            Toggle("منع النوافذ المنبثقة", "والتحويل لمواقع تانية لوحدها", Shield.popups) { Shield.set("popups", it) }
            Toggle("منع التتبع", "عدّادات الزيارات وملفات التتبع", Shield.trackers) { Shield.set("trackers", it) }
            Toggle("إخفاء طلبات الكوكيز", "الرسايل اللي بتغطي الصفحة", Shield.cookies) { Shield.set("cookies", it) }
            Toggle("إخفاء الحاجات المزعجة", "إشعارات وتطبيقات بتطلع فوق الصفحة", Shield.annoyances) { Shield.set("annoyances", it) }
            Card(Modifier.clickable { sites = true }) {
                Column(Modifier.weight(1f)) {
                    Text("المواقع المسموحة", fontWeight = FontWeight.Bold, fontSize = 16.sp, color = Ui.Ink)
                    Text("مواقع مش عايز تمنع فيها الإعلانات", color = Ui.Muted, fontSize = 13.sp)
                }
                Text("‹ ${Shield.allowed.size}", color = Ui.Violet, fontWeight = FontWeight.ExtraBold, fontSize = 16.sp)
            }
            Card {
                Column(Modifier.weight(1f)) {
                    Text("القوائم", fontWeight = FontWeight.Bold, fontSize = 16.sp, color = Ui.Ink)
                    val t = Shield.listsTime
                    Text(
                        Shield.status.ifEmpty {
                            if (t == 0L) "لسه متحملتش" else "آخر تحديث: " + SimpleDateFormat("d/M · h:mm a", Locale("ar")).format(Date(t * 1000))
                        },
                        color = Ui.Muted, fontSize = 13.sp,
                    )
                }
                Text(
                    "تحديث",
                    color = Ui.Violet, fontWeight = FontWeight.ExtraBold, fontSize = 16.sp,
                    modifier = Modifier.clip(RoundedCornerShape(8.dp)).clickable { Shield.updateNow(ctx) }.padding(8.dp),
                )
            }
            Text(
                "قوائم الحماية قوائم مفتوحة معروفة (منها قايمة للمواقع العربية)، بتتحمل من مصادرها وبتتحدث لوحدها كل كام يوم.",
                color = Ui.Muted, fontSize = 12.sp, modifier = Modifier.padding(horizontal = 6.dp),
            )
        }
    }
}

@Composable
private fun Card(modifier: Modifier = Modifier, content: @Composable androidx.compose.foundation.layout.RowScope.() -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .shadow(1.dp, RoundedCornerShape(18.dp))
            .clip(RoundedCornerShape(18.dp))
            .background(Color.White)
            .then(modifier)
            .padding(horizontal = 16.dp, vertical = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
        content = content,
    )
}

@Composable
private fun Toggle(title: String, sub: String, on: Boolean, onChange: (Boolean) -> Unit) {
    Card {
        Column(Modifier.weight(1f)) {
            Text(title, fontWeight = FontWeight.Bold, fontSize = 16.sp, color = Ui.Ink)
            Text(sub, color = Ui.Muted, fontSize = 13.sp)
        }
        Switch(
            checked = on,
            onCheckedChange = onChange,
            colors = SwitchDefaults.colors(checkedTrackColor = Ui.Violet, checkedThumbColor = Color.White),
        )
    }
}
