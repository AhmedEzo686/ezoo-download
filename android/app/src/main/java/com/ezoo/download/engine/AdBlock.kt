package com.ezoo.download.engine

/** The browser's ad blocking, in libezoo.so (crates/android/src/adblock.rs). */
object AdBlock {
    init {
        System.loadLibrary("ezoo")
    }

    /** Fetch every filter list into [dir] (JSON: ok, failed). Slow. */
    external fun update(dir: String): String

    /** Build the blocker of these groups ("ads,privacy"): lists read. Takes a moment. */
    external fun load(dir: String, groups: String): Int

    /** When the lists were last fetched (seconds since 1970, 0 = never). */
    external fun listsTime(dir: String): Long

    /** "" let it go, "B" block, "R<data:...>" answer with this stand-in. */
    external fun check(url: String, source: String, kind: String): String

    /** What a page gets at its start (JSON: css, script, generichide). */
    external fun cosmetic(url: String): String

    /** CSS hiding the generic ad boxes among these classes and ids (space-separated). */
    external fun generic(url: String, classes: String, ids: String): String
}
