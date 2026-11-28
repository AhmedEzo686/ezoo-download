package com.ezoo.download

import android.content.Context
import android.content.Intent
import android.webkit.MimeTypeMap
import androidx.core.content.FileProvider
import java.io.File

object Files {
    /** An intent that opens the file in the app the user picks (null: no file). */
    fun openIntent(ctx: Context, path: String): Intent? {
        val f = File(path)
        if (!f.isFile) return null
        val uri = try {
            FileProvider.getUriForFile(ctx, "${ctx.packageName}.files", f)
        } catch (_: IllegalArgumentException) {
            return null
        }
        val ext = f.extension.lowercase()
        val mime = MimeTypeMap.getSingleton().getMimeTypeFromExtension(ext) ?: "*/*"
        return Intent(Intent.ACTION_VIEW)
            .setDataAndType(uri, mime)
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
    }

    private val LINK = Regex("""(?i)\b(?:https?|ftp)://[^\s<>"']+|\bmagnet:\?[^\s<>"']+""")

    /** The first link in a shared text ("Look at this: https://..."). */
    fun findLink(text: String?): String? =
        text?.let { LINK.find(it)?.value?.trimEnd('.', ',', ')', ']', '»', '"') }

    fun isLink(text: String): Boolean = LINK.matchEntire(text.trim()) != null
}
