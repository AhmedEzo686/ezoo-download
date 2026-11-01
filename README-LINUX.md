<div align="center">

# Ezoo Download — نسخة لينكس

نفس برنامج **Ezoo Download** (Rust + واجهة Slint) مبنيًّا ليعمل على لينكس.
</div>

---

## ✅ ما يعمل على لينكس

| الميزة | الحالة |
|---|---|
| التحميل متعدد الاتصالات (حتى 32 اتصالًا) والاستئناف بعد الإغلاق | يعمل |
| الإيقاف المؤقت/الاستئناف، قوائم الانتظار، الجدولة، إيقاف السرعة، التصنيف التلقائي | يعمل |
| Grabber، قائمة التشغيل (Playlist)، تحميل الفيديو عبر yt-dlp | يعمل |
| التورنت والمغناطيس (عبر `aria2c`) | يعمل عند تثبيت `aria2` |
| إضافة المتصفح والمنفذ المحلي (127.0.0.1:9614) | يعمل |
| أيقونة بجانب الساعة + إخفاء النوافذ إليها | يعمل (StatusNotifierItem) |
| التشغيل عند تسجيل الدخول | يعمل (ملف XDG في `~/.config/autostart`) |
| أصوات الانتهاء (WAV) | يعمل (paplay / pw-play / ffplay / aplay) |
| فتح المجلد، «فتح باستخدام…»، معاينة أثناء التحميل | يعمل (xdg-open) |
| إفلات ملف `.torrent` على النافذة | يعمل |
| ثماني لغات، منها العربية (RTL) | يعمل |
| التحديث الذاتي | لا يوجد مثبّت ويندوز على لينكس: يخبرك بالإصدار الجديد ويوجّهك لصفحة الإصدارات |

### أشياء لا تُوجد على لينكس (كما في نسخة ويندوز فقط)

- شفافية النافذة (Opacity) — لا تُطبَّق على لينكس.
- شريط التقدّم على زر شريط المهام (Taskbar progress).
- سحب الملف النهائي من القائمة إلى سطح المكتب (Drag out).
- فحص الفيروسات عبر برنامج ويندوز (اختياري أصلًا).

---

## 📦 التثبيت

### 1) حزمة Debian (مستحسن)

```bash
sudo apt install ./dist/ezoodownload_1.0.8_amd64.deb
# أو
sudo dpkg -i dist/ezoodownload_1.0.8_amd64.deb && sudo apt -f install
```

ثم ابحث عن **Ezoo Download** في قائمة التطبيقات.

### 2) AppImage (ملف واحد، بدون تثبيت)

```bash
chmod +x EzooDownload-1.0.8-x86_64.AppImage
./EzooDownload-1.0.8-x86_64.AppImage
```

- يحتوي البرنامج + مجلدات الإضافة + الأيقونة + المكتبات التي يحتاجها، ويعمل على معظم التوزيعات.
- على نظام بدون FUSE (بعض الحاويات) شغّله بـ: `./EzooDownload-*.AppImage --appimage-extract-and-run`.
- لا يضيف مدخلًا في قائمة التطبيقات؛ لو أردت ذلك استخدم حزمة `.deb` أو النسخة المحمولة بالأسفل.

### 3) نسخة محمولة بدون صلاحيات مدير

```bash
tar xzf dist/EzooDownload-1.0.8-linux-x86_64.tar.gz
./EzooDownload-1.0.8-linux-x86_64/install.sh
```

تُثبَّت في `~/.local` (البرنامج + أيقونة القائمة + الأيقونات). للإزالة:

```bash
rm -rf ~/.local/lib/ezoodownload ~/.local/bin/EzooDownload \
       ~/.local/share/applications/ezoodownload.desktop \
       ~/.local/share/icons/hicolor/*/apps/ezoodownload.png
```

---

## 🧰 ما يُستحسن تثبيته

```bash
sudo apt install ffmpeg aria2 yt-dlp xdg-desktop-portal
```

| الحزمة | لماذا |
|---|---|
| `ffmpeg` | دمج الفيديو مع الصوت، وتحويل MP3، ولقطات المعاينة |
| `aria2` | التورنت والمغناطيس (البرنامج يستدعي `aria2c`) |
| `yt-dlp` | روابط الفيديو من المواقع (وإلا ينزّله البرنامج تلقائيًا مرة واحدة) |
| `xdg-desktop-portal` | نوافذ اختيار الملفات والمجلدات |

> على GNOME تحتاج إضافة **AppIndicator and KStatusNotifierItem Support** ليظهر رمز البرنامج بجانب الساعة.
> على KDE وXfce وCinnamon وBudgie يعمل مباشرة.

---

## 🏗️ البناء من الشيفرة

```bash
sudo apt install build-essential pkg-config libfontconfig1-dev libfreetype-dev \
                 libxkbcommon-dev libwayland-dev libx11-dev libegl-dev libgl1-mesa-dev \
                 libdbus-1-dev imagemagick
cargo build --release
./packaging/linux/build-linux.sh      # ينتج dist/*.deb و dist/*.tar.gz
```

OpenSSL يُبنى داخل البرنامج (vendored)، لذلك لا يعتمد على إصدار OpenSSL المثبّت على الجهاز.

---

## 🗂️ أماكن الملفات

| ما | أين |
|---|---|
| الإعدادات وقائمة التحميلات والسجل | `~/.local/share/ezoodownload/` (`settings.json`, `downloads.json`, `events.log`) |
| أدوات نزّلها البرنامج (yt-dlp) | `~/.local/share/ezoodownload/tools/` |
| مجلد الإضافة (Chrome/Chromium) | `~/.local/share/ezoodownload/extension/` |
| مجلد الإضافة (Firefox) | `~/.local/share/ezoodownload/extension-firefox/` |
| التشغيل مع تسجيل الدخول | `~/.config/autostart/ezoodownload.desktop` |
| ملفات الحزمة | `/usr/bin/EzooDownload` و`/usr/share/ezoodownload/` |

---

## 🖱️ إضافة المتصفح

من داخل البرنامج: **Options ← General ← Add Ezoo Download to my browser…**
- **Chrome / Chromium / Edge / Brave / Opera:** `chrome://extensions` ← فعّل *Developer mode* ← *Load unpacked* ← اختر مجلد `~/.local/share/ezoodownload/extension`.
- **Firefox:** إن وُجد ملف `ezoo-download-firefox.xpi` تُثبَّت الإضافة الموقّعة مباشرةً، وإلا فمن `about:debugging`.

المنفذ الافتراضي `9614` ويمكن تغييره من الإعدادات (أعد التشغيل بعدها).

---

## ⚠️ ملاحظات

- البرنامج يسجّل نفسه للتشغيل عند الدخول مرة واحدة عند أول تشغيل لنسخة محمولة (يمكن إيقافه من الإعدادات). نسخة `.deb` لا تفعل ذلك تلقائيًا.
- «السرعة» و«عدد الاتصالات» و«الوكيل» كلها تعمل كما في ويندوز.
- سجل الأحداث في `events.log` مفيد عند الإبلاغ عن مشكلة.
