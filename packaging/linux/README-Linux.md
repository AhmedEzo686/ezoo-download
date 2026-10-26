<div dir="rtl" align="center">

# 🦅 Ezoo Download — النسخة اللينكسية

**مدير تنزيلات مكتوب بلغة Rust** — يحلّل الملف على عدة اتصالات (حتى 32 اتصالًا)، ويستأنف التحميل بعد الانقطاع، وينزّل الفيديو والصوت عبر yt-dlp و FFmpeg، مع إضافات لكروم وفايرفوكس.

**إصدار 1.0.6 — لينكس** · تطوير **Ahmed Ezzat** (المشروع الأصلي) · حزمة لينكس مع أيقونة Tray أصلية وشعار SVG احترافي

![Linux](https://img.shields.io/badge/Linux-KDE%20%7C%20GNOME%20%7C%20XFCE-2E7BF6?style=flat-square&logo=linux&logoColor=white)
![Rust](https://img.shields.io/badge/Made%20with-Rust-B7410E?style=flat-square&logo=rust&logoColor=white)
![Tray](https://img.shields.io/badge/System%20Tray-StatusNotifierItem-8A57F5?style=flat-square)

</div>

---

<div dir="rtl">

## 📦 محتويات الحزمة

| الملف | الوصف |
|---|---|
| `ezoo-download_1.0.6-1_amd64.deb` | حزمة Debian/Ubuntu/Mint ومشتقاتها |
| `Ezoo-Download-1.0.6-x86_64.AppImage` | نسخة محمولة بملف واحد تعمل دون تثبيت |
| `ezoo-download-chrome-1.0.6.crx` | إضافة كروم (Chromium / Edge / Brave / Opera) |
| `ezoo-download-firefox-1.0.6.xpi` | إضافة فايرفوكس |
| `screenshots/` | لقطات شاشة للبرنامج بحلته الجديدة على لينكس |
| `SHA256SUMS.txt` | بصمات التحقق من سلامة الملفات |

---

## ✨ الجديد في نسخة لينكس

- 🔔 **أيقونة Tray أصلية** تعمل بنظام **StatusNotifierItem** المعياري في لينكس: تظهر بجوار الساعة في KDE Plasma و GNOME (مع إضافة AppIndicator المدمجة في أوبونتو) و XFCE، وبها قائمة كاملة: استعادة النافذة، إضافة تحميل، تشغيل/إيقاف الكل، محدد السرعة، المجدول، الإعدادات، خروج. عند إغلاق النافذة يبقى البرنامج يعمل من الشريط.
- 🎨 **أيقونات SVG احترافية** بمعيار freedesktop (hicolor): تُعرض بشكل صحيح في قوائم KDE و GNOME و XFCE بأحجام من 16 حتى 512 بكسل + نسخة Scalable + نسخة Symbolic أحادية اللون، وأيقونة خاصة بالشريط بتصميم أوضح للأحجام الصغيرة.
- 🖼️ نوافذ تُفتح في **منتصف الشاشة** تلقائيًا.
- ⚙️ **تشغيل تلقائي عند الدخول** بمعيار XDG (`~/.config/autostart`).
- 💾 البيانات في مواضع XDG القياسية: الإعدادات والقوائم في `~/.local/share/ezoodownload/`، والتنزيلات في مجلد التنزيلات لديك.
- ⏻ إيقاف/سكون الجهاز بعد انتهاء التنزيلات عبر `systemctl` (systemd).
- 🛠️ يجلب **yt-dlp و ffmpeg بنسخ لينكس** تلقائيًا ويتحقق من بصمة SHA-256 قبل تشغيلها (ويمكنه استخدام نسخ النظام من PATH إن وُجدت).

---

## 🖥️ لقطات الشاشة

| | |
|---|---|
| ![الواجهة الرئيسية](screenshots/main-ar-light-2.png) | ![الإضافة إلى التحميل](screenshots/main-ar-dark.png) |
| **الواجهة العربية** — قوائم التصنيف، التحميلات الحية، نافذة التقدم | **حوار إضافة تحميل** — 32 اتصالًا، اختيار الجودة، الثيم الداكن |
| ![حوار الإضافة](screenshots/add-dialog-ar.png) | ![أيقونة الشريط](screenshots/tray-ar-illustration.png) |
| **حوار معلومات الملف** | **أيقونة الشريط وقائمتها** (صورة توضيحية) |

---

## 🚀 التثبيت

### ١) نسخة deb — التثبيت الكامل (مستحسن)

```bash
sudo dpkg -i ezoo-download_1.0.6-1_amd64.deb
# أو إذا ظهرت رسالة نواقص في التبعيات:
sudo apt-get install -f
```

بعدها ستجد البرنامج في قائمة تطبيقات النظام باسم **Ezoo Download**، ويمكن تشغيله بالأمر `ezoo-download`.

ما يثبّته البرنامج:

```
/usr/lib/ezoo-download/EzooDownload     البرنامج
/usr/bin/ezoo-download                  اختصار التشغيل
/usr/share/applications/ezoo-download.desktop
/usr/share/icons/hicolor/...            الأيقونات (16 → 512 + SVG)
/usr/share/ezoo-download/extension/            مصدر إضافة كروم
/usr/share/ezoo-download/extension-firefox/    مصدر إضافة فايرفوكس
/usr/share/ezoo-download/ezoo-download-firefox.xpi
```

> لا يحتاج التثبيت إلى صلاحيات مدير بعد أمر dpkg نفسه، ولا يعدّل أي إعدادات نظام.

### ٢) نسخة AppImage — محمولة دون تثبيت

```bash
chmod +x Ezoo-Download-1.0.6-x86_64.AppImage
./Ezoo-Download-1.0.6-x86_64.AppImage
```

- إذا لم تعمل مباشرة فنظامك ينقصه مكوّن FUSE: ثبّته مرة واحدة `sudo apt install libfuse2`، أو شغّل النسخة بفكّها:

```bash
./Ezoo-Download-1.0.6-x86_64.AppImage --appimage-extract
./squashfs-root/AppRun
```

---

## 🧩 تثبيت إضافة المتصفح

1. افتح البرنامج، ثم من القائمة: **إضافات** (أيقونة الكرة الأرضية في شريط الأدوات).
2. البرنامج يكتشف متصفحاتك ويشرح الخطوات وينسخ مسار الإضافة تلقائيًا.

### كروم (Chrome / Edge / Brave / Opera) — ملف `.crx`

- الطريقة الأسهل: من داخل البرنامج اختر كروم من قائمة الإضافات ثم اتبع الخطوات (تحميل الإضافة غير المعبّأة من `/usr/share/ezoo-download/extension`).
- لتثبيت ملف `.crx` نفسه: فعّل **Developer mode** من صفحة `chrome://extensions`، ثم اسحب الملف وأفلته في الصفحة.
- ملاحظة: كروم يعرض تحذيرًا للإضافات غير المنشورة في المتجر — اختر **Keep / إبقاء** للاستمرار. الملف موقّع ببصمة RSA-2048 ومعرّفه الثابت: `bilpkcfhiogfelpdfpgmnjkpblgmcbkd`.

### فايرفوكس — ملف `.xpi`

- **تجربة سريعة:** من `about:debugging#/runtime/this-firefox` ← **Load Temporary Add-on** ← اختر ملف الـ xpi (تبقى حتى إغلاق فايرفوكس).
- **تثبيت دائم:** فايرفوكس يشترط توقيع Mozilla للإضافات الدائمة؛ يمكن رفع هذا الـ xpi إلى [addons.mozilla.org](https://addons.mozilla.org/developers/) للتوقيع المجاني، أو استخدامه الدائم على Firefox Developer Edition / Nightly بعد تعطيل `xpinstall.signatures.required` من `about:config`.
- معرف الإضافة: `ezoo-download@ezoo-tech.com` (متوافقة أيضًا مع فايرفوكس أندرويد).

---

## 🔔 أيقونة الشريط (Tray)

تظهر الأيقونة بجوار الساعة بعد تشغيل البرنامج:

| النقرة | الفعل |
|---|---|
| نقرة على الأيقونة (KDE) | إظهار النافذة |
| قائمة الزر الأيمن (وفي GNOME/XFCE أي نقرة) | القائمة كاملة |

من القائمة: **Restore** إظهار النافذة · **Add new download** إضافة رابط · **Resume/Stop all** تشغيل/إيقاف الكل · **Speed Limiter** تشغيل محدد السرعة (بعلامة صح) · **Scheduler** المجدول · **Options** الإعدادات · **Exit** خروج نهائي.

> **ملاحظة لمستخدمي GNOME خالص:** تحتاج إضافة AppIndicator Support وهي مدمجة افتراضيًا في أوبونتو، وللتنصيب: `sudo apt install gnome-shell-extension-appindicator` ثم إعادة الدخول.

> عند إغلاق نافذة البرنامج يبقى يعمل من الشريط (قابل للتغيير من الإعدادات ← عام ← الإغلاق إلى الشريط). لتعطيل الشريط كليًا أغلق البرنامج من أيقونته.

---

## 📖 استخدام سريع

- **إضافة رابط:** زر «إضافة مجموعه رابط» أو Ctrl+N، أو الصق رابطًا في الحافظة وسيعرض عليك البرنامج إضافته، أو أرسله من إضافة المتصفح.
- **الفيديو والصوت:** الصق رابط صفحة الفيديو (يوتيوب وأكثر من 1800 موقع) وسيجلب الخلفيات بجوداتها عبر yt-dlp.
- **الاستئناف:** أيقونة الاستئناف للملف، حتى بعد إعادة تشغيل البرنامج أو الجهاز.
- **قوائم الانتظار والجدولة:** من «قوائم التنضيل»، مع إمكانية إيقاف الجهاز عند الانتهاء.
- **تحديد السرعة:** من «محدد السرعة» — حد عام أو لكل ملف.
- **اللغات:** ثماني لغات من الإعدادات (العربية والإنجليزية وغيرها).

---

## 🩹 استكشاف الأخطاء

| المشكلة | الحل |
|---|---|
| AppImage لا يفتح | `sudo apt install libfuse2` أو استخدم `--appimage-extract` كما في الأعلى |
| لا تظهر أيقونة الشريط في GNOME | ثبّت `gnome-shell-extension-appindicator` وأعد الدخول |
| نافذة اختيار المجلد لا تفتح | البرنامج يستخدم بوابة xdg-desktop-portal: `sudo apt install xdg-desktop-portal` (موجود افتراضيًا في بيئات سطح المكتب) |
| تنزيل الفيديو يقول ffmpeg مفقود | انتظر دقائق — يجلب الأداة تلقائيًا، أو ثبّتها من النظام: `sudo apt install ffmpeg` |
| التحميلات لا تستأنف من السيرفر | بعض المواقع لا تدعم الاستئناف (يظهر "لا يمكن الاستئناف" بجانب الملف) |

---

## 🗂️ الحزمة المصدرية

ملف `Ezoo-Download-1.0.5-Linux-Source.zip` يحتوي السورس كاملًا بكل الإصدارات (تطبيق سطح المكتب + المحرك + الإضافتين + أيقونات SVG + سكربتات التغليف `packaging/linux/`):

```bash
# بناء كل شيء من المصدر:
./packaging/linux/build-linux.sh all
```

المتطلبات: Rust (rustup) + بنية تطوير قياسية (pkg-config و libssl-dev)، ولا يحتاج GTK.

---

## 🛡️ الخصوصية والشكر

البرنامج لا يجمع ولا يرسل أي بيانات إلى أي جهة. الاتصال بالإضافة يمر على `127.0.0.1:9614` محليًا فقط. © 2026 **Ahmed Ezzat** — الترخيص MIT، ومكونات مفتوحة المصدر في `THIRD-PARTY-NOTICES.txt`.

النسخة اللينكسية أُعدّت بناءً على طلب المجتمع: أيقونة Tray أصلية (ksni/StatusNotifierItem)، أيقونات SVG بمعيار freedesktop، وحِزم deb/AppImage وامتدادات crx/xpi.
</div>
