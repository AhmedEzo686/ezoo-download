"""Video pages for Ezoo Download on Android: yt-dlp finds the video and its
qualities; the app's engine (crates/android) makes the choices of them and
downloads, as the Windows program does with yt-dlp's -J answer."""

import json


def info(url, cache_dir):
    """yt-dlp's full answer for a page, as JSON (the same as `yt-dlp -J`)."""
    import yt_dlp

    opts = {
        "quiet": True,
        "no_warnings": True,
        "noprogress": True,
        "noplaylist": True,
        "socket_timeout": 20,
        "cachedir": cache_dir or False,
    }
    with yt_dlp.YoutubeDL(opts) as ydl:
        found = ydl.extract_info(url, download=False)
        return json.dumps(ydl.sanitize_info(found))


def version():
    from yt_dlp.version import __version__

    return __version__
