"""Finder layout for the macOS drag-to-Applications disk image."""
import os

application = os.path.abspath(defines["app"])
app_name = os.path.basename(application)

format = "UDZO"
filesystem = "HFS+"
files = [application]
symlinks = {"Applications": "/Applications"}
icon = os.path.join(application, "Contents", "Resources", "EzooDownload.icns")
background = "builtin-arrow"
icon_locations = {app_name: (140, 120), "Applications": (500, 120)}
window_rect = ((200, 200), (640, 280))
default_view = "icon-view"
arrange_by = None
icon_size = 96
text_size = 14
show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False
# FinderInfo on an already-signed bundle makes strict codesign verification
# fail. Keep the bundle's metadata untouched, including extension visibility.
hide_extensions = []
