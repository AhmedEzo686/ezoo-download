package com.ezoo.download

import android.Manifest
import android.content.ClipboardManager
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.horizontalScroll
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.material3.lightColorScheme
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
import androidx.compose.ui.res.stringResource
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

private val Violet = Color(0xFF6D4AFF)
private val Ink = Color(0xFF151826)
private val Muted = Color(0xFF6B7090)
private val Page = Color(0xFFF6F7FB)

class MainActivity : ComponentActivity() {
    /** A link shared to the app (or opened with it): the add sheet shows it. */
    private var shared by mutableStateOf<String?>(null)

    private val askPermissions =
        registerForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { got ->
            if (got[Manifest.permission.WRITE_EXTERNAL_STORAGE] == false) {
                Toast.makeText(this, R.string.storage_needed, Toast.LENGTH_LONG).show()
            }
        }

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        askWhatIsNeeded()
        shared = linkOf(intent)
        setContent {
            MaterialTheme(colorScheme = lightColorScheme(primary = Violet, background = Page, surface = Color.White)) {
                Home(
                    shared = shared,
                    onSharedTaken = { shared = null },
                )
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

private enum class Tab(val label: Int) { All(R.string.tab_all), Running(R.string.tab_running), Done(R.string.tab_done), Stopped(R.string.tab_failed) }

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun Home(shared: String?, onSharedTaken: () -> Unit) {
    val ctx = LocalContext.current
    var list by remember { mutableStateOf(emptyList<Download>()) }
    var tab by remember { mutableStateOf(Tab.All) }
    var adding by remember { mutableStateOf<String?>(null) }
    var menu by remember { mutableStateOf(false) }

    // the list, fresh every half second while the screen shows
    LaunchedEffect(Unit) {
        while (true) {
            list = Download.parseList(Engine.list())
            delay(500)
        }
    }
    LaunchedEffect(shared) {
        if (shared != null) {
            adding = shared
            onSharedTaken()
        }
    }
    EzooApp.engineError?.let { err ->
        LaunchedEffect(err) {
            Toast.makeText(ctx, ctx.getString(R.string.engine_error, err), Toast.LENGTH_LONG).show()
        }
    }

    Scaffold(
        containerColor = Page,
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.downloads), fontWeight = FontWeight.ExtraBold) },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = Color.White),
                actions = {
                    Box {
                        IconButton(onClick = { menu = true }) { Icon(Icons.Filled.MoreVert, null) }
                        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                            DropdownMenuItem(text = { Text(stringResource(R.string.resume_all)) }, onClick = {
                                menu = false
                                Engine.startAll()
                                DownloadService.start(ctx)
                            })
                            DropdownMenuItem(text = { Text(stringResource(R.string.pause_all)) }, onClick = {
                                menu = false
                                Engine.pauseAll()
                            })
                        }
                    }
                },
            )
        },
        floatingActionButton = {
            FloatingActionButton(
                onClick = { adding = clipboardLink(ctx) ?: "" },
                containerColor = Violet,
                contentColor = Color.White,
                shape = RoundedCornerShape(18.dp),
            ) { Icon(Icons.Filled.Add, stringResource(R.string.new_download)) }
        },
    ) { pad ->
        Column(Modifier.padding(pad).fillMaxSize()) {
            Row(
                Modifier.horizontalScroll(rememberScrollState()).padding(horizontal = 12.dp, vertical = 8.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Tab.entries.forEach { t ->
                    FilterChip(
                        selected = tab == t,
                        onClick = { tab = t },
                        label = { Text(stringResource(t.label), fontWeight = FontWeight.Bold) },
                        shape = RoundedCornerShape(20.dp),
                        colors = FilterChipDefaults.filterChipColors(
                            selectedContainerColor = Violet,
                            selectedLabelColor = Color.White,
                        ),
                    )
                }
            }
            val shown = list.filter {
                when (tab) {
                    Tab.All -> true
                    Tab.Running -> it.running || it.queued
                    Tab.Done -> it.completed
                    Tab.Stopped -> it.paused || it.failed
                }
            }
            if (shown.isEmpty()) {
                Box(Modifier.fillMaxSize().padding(32.dp), contentAlignment = Alignment.Center) {
                    Text(stringResource(R.string.empty), color = Muted, textAlign = TextAlign.Center)
                }
            } else {
                LazyColumn(contentPadding = PaddingValues(bottom = 96.dp)) {
                    items(shown, key = { it.id }) { d -> DownloadRow(d) }
                }
            }
        }
    }

    adding?.let { first ->
        AddSheet(first = first, onClose = { adding = null })
    }
}

private fun clipboardLink(ctx: android.content.Context): String? {
    val cm = ctx.getSystemService(ClipboardManager::class.java) ?: return null
    val clip = cm.primaryClip ?: return null
    if (clip.itemCount == 0) return null
    return Files.findLink(clip.getItemAt(0).coerceToText(ctx)?.toString())
}

private fun icon(category: String): Pair<String, Color> = when (category) {
    "Video" -> "🎬" to Color(0xFFFDECEC)
    "Music" -> "🎵" to Color(0xFFFFF4DE)
    "Compressed" -> "📦" to Color(0xFFEAF1FF)
    "Programs" -> "💿" to Color(0xFFEAF7EE)
    "Documents" -> "📄" to Color(0xFFEAF1FF)
    "Images" -> "🖼️" to Color(0xFFF1EBFF)
    else -> "📁" to Color(0xFFF0F1F8)
}

@Composable
private fun statusText(d: Download): String {
    val ctx = LocalContext.current
    return when {
        d.waiting -> stringResource(R.string.waiting_net)
        d.status == "Downloading" -> Format.progressLine(ctx, d.done, d.total, d.speed, d.eta)
        d.completed -> listOfNotNull(stringResource(R.string.st_Completed), d.total?.let { Format.size(it) } ?: Format.size(d.done)).joinToString(" · ")
        d.failed -> listOfNotNull(stringResource(R.string.st_Failed), d.error).joinToString(" · ")
        else -> {
            val st = when (d.status) {
                "Queued" -> stringResource(R.string.st_Queued)
                "Connecting" -> stringResource(R.string.st_Connecting)
                "Assembling" -> stringResource(R.string.st_Assembling)
                else -> stringResource(R.string.st_Paused)
            }
            if (d.done > 0) "$st · " + Format.progressLine(ctx, d.done, d.total, 0, null) else st
        }
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun DownloadRow(d: Download) {
    val ctx = LocalContext.current
    var menu by remember { mutableStateOf(false) }
    val open = {
        val i = Files.openIntent(ctx, d.path)
        if (i != null) {
            try {
                ctx.startActivity(Intent.createChooser(i, null).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            } catch (_: Exception) {
                Toast.makeText(ctx, R.string.no_app, Toast.LENGTH_SHORT).show()
            }
        }
        Unit
    }
    val resume = {
        if (d.failed) Engine.restart(d.id) else Engine.start(d.id)
        DownloadService.start(ctx)
    }
    Card(
        modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 5.dp),
        shape = RoundedCornerShape(18.dp),
        colors = CardDefaults.cardColors(containerColor = Color.White),
        elevation = CardDefaults.cardElevation(defaultElevation = 1.dp),
    ) {
        Box {
            Row(
                Modifier
                    .combinedClickable(onClick = { if (d.completed) open() else { menu = true } }, onLongClick = { menu = true })
                    .padding(horizontal = 14.dp, vertical = 12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                val (emoji, bg) = icon(d.category)
                Box(Modifier.size(46.dp).clip(RoundedCornerShape(13.dp)).background(bg), contentAlignment = Alignment.Center) {
                    Text(emoji, fontSize = 22.sp)
                }
                Spacer(Modifier.width(12.dp))
                Column(Modifier.weight(1f)) {
                    Text(
                        d.name,
                        fontWeight = FontWeight.Bold,
                        color = Ink,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        style = MaterialTheme.typography.bodyLarge.copy(textDirection = TextDirection.Content),
                    )
                    Text(
                        statusText(d),
                        color = if (d.failed) Color(0xFFD03A3A) else Muted,
                        fontSize = 13.sp,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                    )
                    if (!d.completed) {
                        Spacer(Modifier.height(6.dp))
                        if ((d.running && d.total == null) || d.status == "Connecting") {
                            LinearProgressIndicator(Modifier.fillMaxWidth().height(5.dp).clip(RoundedCornerShape(3.dp)), color = Violet)
                        } else {
                            LinearProgressIndicator(
                                progress = { d.progress },
                                modifier = Modifier.fillMaxWidth().height(5.dp).clip(RoundedCornerShape(3.dp)),
                                color = if (d.running) Violet else Color(0xFFB3B7CC),
                            )
                        }
                    }
                }
                Spacer(Modifier.width(10.dp))
                val btn = Modifier.size(42.dp).clip(RoundedCornerShape(12.dp)).background(Color(0xFFF0F1F8))
                when {
                    d.running || d.queued -> IconButton(onClick = { Engine.pause(d.id) }, modifier = btn) { PauseGlyph() }
                    d.completed -> IconButton(onClick = { open() }, modifier = btn) { Text("↗", fontSize = 20.sp, color = Ink) }
                    d.failed -> IconButton(onClick = { resume() }, modifier = btn) { Icon(Icons.Filled.Refresh, stringResource(R.string.retry), tint = Ink) }
                    else -> IconButton(onClick = { resume() }, modifier = btn) { Icon(Icons.Filled.PlayArrow, stringResource(R.string.resume), tint = Ink) }
                }
            }
            DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                if (d.completed) DropdownMenuItem(text = { Text(stringResource(R.string.open)) }, onClick = { menu = false; open() })
                if (d.running || d.queued) DropdownMenuItem(text = { Text(stringResource(R.string.pause)) }, onClick = { menu = false; Engine.pause(d.id) })
                if (d.paused) DropdownMenuItem(text = { Text(stringResource(R.string.resume)) }, onClick = { menu = false; resume() })
                if (d.failed || d.completed) DropdownMenuItem(text = { Text(stringResource(R.string.retry)) }, onClick = {
                    menu = false
                    Engine.restart(d.id)
                    DownloadService.start(ctx)
                })
                DropdownMenuItem(text = { Text(stringResource(R.string.remove)) }, onClick = { menu = false; Engine.remove(d.id, false) })
                DropdownMenuItem(text = { Text(stringResource(R.string.remove_file), color = Color(0xFFD03A3A)) }, onClick = {
                    menu = false
                    Engine.remove(d.id, true)
                })
            }
        }
    }
}

@Composable
private fun PauseGlyph() {
    Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
        Box(Modifier.size(width = 4.dp, height = 14.dp).background(Ink, RoundedCornerShape(1.dp)))
        Box(Modifier.size(width = 4.dp, height = 14.dp).background(Ink, RoundedCornerShape(1.dp)))
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun AddSheet(first: String, onClose: () -> Unit) {
    val ctx = LocalContext.current
    val state = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    var link by remember(first) { mutableStateOf(first) }
    var name by remember { mutableStateOf("") }
    var problem by remember { mutableStateOf<Int?>(null) }
    ModalBottomSheet(onDismissRequest = onClose, sheetState = state, containerColor = Color.White) {
        Column(
            Modifier.padding(horizontal = 20.dp).padding(bottom = 16.dp).navigationBarsPadding().imePadding(),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(stringResource(R.string.new_download), fontWeight = FontWeight.ExtraBold, fontSize = 22.sp, color = Ink)
            OutlinedTextField(
                value = link,
                onValueChange = { link = it; problem = null },
                label = { Text(stringResource(R.string.link)) },
                singleLine = true,
                isError = problem != null,
                supportingText = { problem?.let { Text(stringResource(it)) } },
                textStyle = MaterialTheme.typography.bodyLarge.copy(textDirection = TextDirection.Ltr),
                trailingIcon = {
                    TextButton(onClick = { clipboardLink(ctx)?.let { link = it; problem = null } }) {
                        Text(stringResource(R.string.paste))
                    }
                },
                modifier = Modifier.fillMaxWidth(),
            )
            OutlinedTextField(
                value = name,
                onValueChange = { name = it },
                label = { Text(stringResource(R.string.file_name)) },
                singleLine = true,
                textStyle = MaterialTheme.typography.bodyLarge.copy(textDirection = TextDirection.Content),
                modifier = Modifier.fillMaxWidth(),
            )
            Text(
                stringResource(R.string.save_to) + ": 📁 Download / Ezoo",
                color = Muted,
                fontSize = 14.sp,
            )
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                Button(
                    onClick = {
                        val url = link.trim()
                        when {
                            !Files.isLink(url) -> problem = R.string.bad_link
                            Engine.find(url) != 0L -> problem = R.string.already
                            else -> {
                                val id = Engine.add(url, name.trim().ifEmpty { null }, null, null, null)
                                if (id == 0L) {
                                    problem = R.string.bad_link
                                } else {
                                    DownloadService.start(ctx)
                                    Toast.makeText(ctx, R.string.added, Toast.LENGTH_SHORT).show()
                                    onClose()
                                }
                            }
                        }
                    },
                    colors = ButtonDefaults.buttonColors(containerColor = Violet),
                    shape = RoundedCornerShape(14.dp),
                    modifier = Modifier.weight(1f).height(52.dp),
                ) { Text(stringResource(R.string.start), fontWeight = FontWeight.Bold) }
                FilledTonalButton(
                    onClick = onClose,
                    shape = RoundedCornerShape(14.dp),
                    modifier = Modifier.height(52.dp),
                ) { Text(stringResource(R.string.cancel)) }
            }
        }
    }
}
