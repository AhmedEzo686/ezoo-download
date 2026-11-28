package com.ezoo.download.engine

/**
 * The download engine of Ezoo Download (the same one as the Windows
 * program), in libezoo.so: crates/android.
 */
object Engine {
    init {
        System.loadLibrary("ezoo")
    }

    /** Starts it once. Null when it runs, else what went wrong. */
    external fun init(dataDir: String, downloadDir: String, nativeDir: String): String?

    /** A new download (started at once when [start]): its number (0 = not added). */
    external fun add(
        url: String, fileName: String?, referer: String?, cookies: String?, userAgent: String?,
        connections: Int, start: Boolean,
    ): Long

    /** What the link gives (JSON: page, name, size, category / error). Waits for the server. */
    external fun probe(url: String): String

    /** The folder (kind) of a file name: Video, Music, Programs ... */
    external fun categoryOf(name: String): String

    /** yt-dlp's JSON for a page made into the qualities (JSON: title, site, options / error). */
    external fun videoOptions(page: String, info: String): String

    /** Download a quality given by [videoOptions]: its number (0 = not added). */
    external fun addVideo(page: String, key: String, title: String?, start: Boolean): Long

    /** The number of the download of this link already in the list, else 0. */
    external fun find(url: String): Long

    external fun start(id: Long)
    external fun pause(id: Long)
    external fun restart(id: Long)
    external fun remove(id: Long, deleteFile: Boolean)
    external fun startAll()
    external fun pauseAll()

    /** Something downloads or waits its turn. */
    external fun busy(): Boolean

    /** Every download, newest first (JSON). */
    external fun list(): String

    /** The downloads finished since the last call. */
    external fun takeCompleted(): LongArray

    /** Everything stops and the list is saved. */
    external fun shutdown()
}
