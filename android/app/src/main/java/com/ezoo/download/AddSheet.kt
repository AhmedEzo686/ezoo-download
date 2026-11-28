package com.ezoo.download

import android.widget.Toast
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDirection
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.ezoo.download.engine.Engine
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import org.json.JSONObject

/** What the link turned out to be. */
private sealed class Found {
    data object Empty : Found()
    data object Checking : Found()
    data object LookingVideo : Found()
    data class File(val name: String, val size: Long?, val category: String) : Found()
    data class Video(val page: VideoPage) : Found()
    data class Problem(val text: String) : Found()
}

private val CONNECTIONS = listOf(1, 2, 4, 8, 16, 32)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AddSheet(first: Adding, onClose: () -> Unit) {
    val ctx = LocalContext.current
    val state = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    var link by remember(first) { mutableStateOf(first.link) }
    var fromClipboard by remember(first) { mutableStateOf(first.fromClipboard) }
    var name by remember { mutableStateOf("") }
    var nameEdited by remember { mutableStateOf(false) }
    var found by remember { mutableStateOf<Found>(Found.Empty) }
    var connections by remember { mutableStateOf(8) }
    var connMenu by remember { mutableStateOf(false) }
    var picked by remember { mutableStateOf<String?>(null) }

    // the link is looked at once it stops changing: a file (name, size,
    // folder) or a video page (its qualities, from yt-dlp)
    LaunchedEffect(link) {
        val url = link.trim()
        if (!Files.isLink(url)) {
            found = if (url.isEmpty()) Found.Empty else Found.Problem("ده مش رابط تحميل")
            return@LaunchedEffect
        }
        delay(500)
        if (url.startsWith("magnet:", ignoreCase = true)) {
            found = Found.File("", null, "General")
            return@LaunchedEffect
        }
        val asVideo = suspend {
            found = Found.LookingVideo
            val r = withContext(Dispatchers.IO) { Video.look(ctx, url) }
            found = r.fold(
                onSuccess = { p ->
                    picked = p.qualities.firstOrNull()?.key
                    Found.Video(p)
                },
                onFailure = { Found.Problem("مالقيتش فيديو في الرابط ده: ${it.message}") },
            )
        }
        if (Video.isVideoSite(url)) {
            asVideo()
            return@LaunchedEffect
        }
        found = Found.Checking
        val o = withContext(Dispatchers.IO) { JSONObject(Engine.probe(url)) }
        when {
            o.has("error") -> found = Found.Problem(o.getString("error"))
            o.optBoolean("page") -> asVideo()
            else -> {
                val n = o.optString("name")
                if (!nameEdited) name = n
                found = Found.File(n, if (o.isNull("size")) null else o.optLong("size"), o.optString("category", "General"))
            }
        }
    }

    fun add(start: Boolean) {
        val url = link.trim()
        val f = found
        val id = when {
            !Files.isLink(url) -> 0L
            f is Found.Video -> {
                val key = picked ?: return
                Engine.addVideo(f.page.page, key, null, start)
            }
            f is Found.LookingVideo || f is Found.Checking -> return
            else -> {
                if (Engine.find(url) != 0L) {
                    Toast.makeText(ctx, "الرابط ده موجود في القائمة", Toast.LENGTH_SHORT).show()
                    return
                }
                Engine.add(url, name.trim().ifEmpty { null }, null, null, null, connections, start)
            }
        }
        if (id == 0L) {
            Toast.makeText(ctx, "ماقدرتش أضيف التحميل", Toast.LENGTH_SHORT).show()
            return
        }
        if (start) DownloadService.start(ctx)
        Toast.makeText(ctx, if (start) "التحميل بدأ" else "اتضاف للقائمة", Toast.LENGTH_SHORT).show()
        onClose()
    }

    ModalBottomSheet(
        onDismissRequest = onClose,
        sheetState = state,
        containerColor = Color.White,
        shape = RoundedCornerShape(topStart = 28.dp, topEnd = 28.dp),
        dragHandle = {
            Box(Modifier.padding(top = 14.dp, bottom = 6.dp).size(46.dp, 5.dp).clip(RoundedCornerShape(3.dp)).background(Color(0xFFD5D8EA)))
        },
    ) {
        Column(
            Modifier
                .padding(horizontal = 18.dp)
                .padding(bottom = 18.dp)
                .navigationBarsPadding()
                .imePadding()
                .verticalScroll(rememberScrollState()),
        ) {
            Text("تحميل جديد", fontWeight = FontWeight.ExtraBold, fontSize = 21.sp, color = Ui.Ink)

            Label(if (fromClipboard) "الرابط (اتلقط من الحافظة)" else "الرابط")
            Field(link, { link = it; fromClipboard = false }, placeholder = "https://…", trailing = {
                Text(
                    "لصق",
                    color = Ui.Violet,
                    fontWeight = FontWeight.Bold,
                    fontSize = 14.sp,
                    modifier = Modifier.clip(RoundedCornerShape(8.dp)).clickable {
                        clipboardLink(ctx)?.let { link = it; fromClipboard = true }
                    }.padding(horizontal = 10.dp, vertical = 6.dp),
                )
            })

            when (val f = found) {
                Found.Empty -> {}
                Found.Checking -> Waiting("بيشوف الرابط…")
                Found.LookingVideo -> Waiting("بيدور على الفيديو والجودات…")
                is Found.Problem -> Text(f.text, color = Ui.Red, fontSize = 14.sp, modifier = Modifier.padding(top = 10.dp))
                is Found.Video -> VideoChoices(f.page, picked) { picked = it }
                is Found.File -> {
                    Label("اسم الملف")
                    Field(name, { name = it; nameEdited = true }, placeholder = "من السيرفر")
                    Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                        Column(Modifier.weight(1f)) {
                            Label("الحجم")
                            Box_(f.size?.let { Format.size(it) } ?: "مش معروف")
                        }
                        Column(Modifier.weight(1f)) {
                            Label("الاتصالات")
                            Box {
                                Box_("$connections ▾", Modifier.clickable { connMenu = true })
                                DropdownMenu(expanded = connMenu, onDismissRequest = { connMenu = false }, containerColor = Color.White) {
                                    CONNECTIONS.forEach { n ->
                                        DropdownMenuItem(text = { Text("$n") }, onClick = { connections = n; connMenu = false })
                                    }
                                }
                            }
                        }
                    }
                    val cat = Engine.categoryOf(name.ifBlank { f.name })
                    Label("مكان الحفظ")
                    Box_("📁 Download / Ezoo" + if (cat != "General") " / $cat" else "")
                }
            }

            Spacer(Modifier.height(16.dp))
            val ready = found is Found.File || (found is Found.Video && picked != null)
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                Box(
                    Modifier
                        .weight(2f)
                        .height(50.dp)
                        .clip(RoundedCornerShape(14.dp))
                        .background(if (ready) Ui.Gradient else androidx.compose.ui.graphics.SolidColor(Color(0xFFB9B3E8)))
                        .clickable(enabled = ready) { add(true) },
                    contentAlignment = Alignment.Center,
                ) { Text("⬇ ابدأ التحميل", color = Color.White, fontWeight = FontWeight.ExtraBold, fontSize = 16.sp) }
                Box(
                    Modifier
                        .weight(1f)
                        .height(50.dp)
                        .clip(RoundedCornerShape(14.dp))
                        .background(Ui.Chip)
                        .clickable(enabled = ready) { add(false) },
                    contentAlignment = Alignment.Center,
                ) { Text("بعدين", color = Ui.Label, fontWeight = FontWeight.ExtraBold, fontSize = 16.sp) }
            }
        }
    }
}

@Composable
private fun Label(text: String) {
    Text(text, fontSize = 13.sp, color = Ui.Muted, fontWeight = FontWeight.Bold, modifier = Modifier.padding(top = 10.dp, bottom = 6.dp))
}

/** A read-only box like the text boxes. */
@Composable
private fun Box_(text: String, modifier: Modifier = Modifier) {
    Box(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(12.dp))
            .background(Ui.Field)
            .border(1.dp, Ui.FieldLine, RoundedCornerShape(12.dp))
            .then(modifier)
            .padding(horizontal = 12.dp, vertical = 11.dp),
    ) { Text(text, fontSize = 15.sp, color = Ui.Ink, maxLines = 1, overflow = TextOverflow.Ellipsis) }
}

@Composable
private fun Waiting(text: String) {
    Row(Modifier.padding(top = 14.dp), verticalAlignment = Alignment.CenterVertically) {
        CircularProgressIndicator(Modifier.size(18.dp), color = Ui.Violet, strokeWidth = 2.dp)
        Spacer(Modifier.width(10.dp))
        Text(text, color = Ui.Label, fontSize = 14.sp, fontWeight = FontWeight.Bold)
    }
}

@Composable
private fun VideoChoices(page: VideoPage, picked: String?, onPick: (String) -> Unit) {
    Spacer(Modifier.height(12.dp))
    Row(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(18.dp))
            .border(2.dp, Ui.Violet.copy(alpha = 0.2f), RoundedCornerShape(18.dp))
            .padding(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.size(46.dp).clip(RoundedCornerShape(13.dp)).background(Ui.Gradient), contentAlignment = Alignment.Center) {
            Text("🎬", fontSize = 22.sp)
        }
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            Text(
                page.title.ifBlank { "فيديو" },
                fontWeight = FontWeight.ExtraBold, fontSize = 15.sp, color = Ui.Ink, maxLines = 2,
                overflow = TextOverflow.Ellipsis, style = TextStyle(textDirection = TextDirection.Content),
            )
            Text("لقيت فيديو في الرابط · اختار الجودة", color = Ui.Muted, fontSize = 13.sp)
        }
    }
    Column(Modifier.heightIn(max = 330.dp).verticalScroll(rememberScrollState())) {
        page.qualities.forEach { q ->
            val on = q.key == picked
            Row(
                Modifier
                    .padding(top = 8.dp)
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(14.dp))
                    .background(if (on) Ui.Violet.copy(alpha = 0.07f) else Color.White)
                    .border(if (on) 2.dp else 1.dp, if (on) Ui.Violet else Ui.Line, RoundedCornerShape(14.dp))
                    .clickable { onPick(q.key) }
                    .padding(horizontal = 12.dp, vertical = 10.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                Column(Modifier.weight(1f)) {
                    Text(if (q.audio) "صوت بس" else q.label, fontWeight = FontWeight.ExtraBold, fontSize = 16.sp, color = Ui.Ink)
                    if (q.sub.isNotBlank()) Text(q.sub, fontSize = 12.sp, color = Ui.Muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
                Text(
                    q.ext.uppercase(),
                    fontSize = 12.sp, fontWeight = FontWeight.ExtraBold, color = Ui.Label,
                    modifier = Modifier.clip(RoundedCornerShape(8.dp)).background(Ui.Chip).padding(horizontal = 8.dp, vertical = 2.dp),
                )
                if (q.size > 0) Text(Format.size(q.size), fontSize = 14.sp, color = Ui.Muted)
            }
        }
    }
}
