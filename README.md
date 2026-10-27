<div align="center">

<img src="packaging/linux/hicolor/scalable/apps/ezoo-download.svg" alt="Ezoo Download" width="140" />

# Ezoo Download

**مدير تنزيلات لسطح المكتب — ويندوز ولينكس — مكتوب بلغة Rust**

المشروع الأصلي: **Ahmed Ezzat (Ezoo Tech)** · نسخة لينكس والتطوير المنفصل في هذا المستودع: **kerlif**

[![Version](https://img.shields.io/badge/version-1.0.7-8A57F5?style=flat-square)](https://github.com/kerlif/ezoo-download/releases/tag/v1.0.7)
[![Linux build](https://github.com/kerlif/ezoo-download/actions/workflows/linux-release.yml/badge.svg)](https://github.com/kerlif/ezoo-download/actions/workflows/linux-release.yml)
[![Windows build](https://github.com/kerlif/ezoo-download/actions/workflows/build.yml/badge.svg)](https://github.com/kerlif/ezoo-download/actions/workflows/build.yml)
[![Release](https://img.shields.io/github/v/release/kerlif/ezoo-download?display_name=tag&style=flat-square)](https://github.com/kerlif/ezoo-download/releases)
![Windows](https://img.shields.io/badge/Windows-10%20%2C%2011-0078D6?style=flat-square&logo=windows11&logoColor=white)
![Linux](https://img.shields.io/badge/Linux-.deb%20%7C%20AppImage%20%7C%20.rpm-FCC624?style=flat-square&logo=linux&logoColor=black)
![Rust](https://img.shields.io/badge/Made%20with-Rust-B7410E?style=flat-square&logo=rust&logoColor=white)
![Language](https://img.shields.io/badge/UI-عربي%20%7C%20English-success?style=flat-square)

</div>

---

## 📥 تنزيل

| النظام | الملف | |
|---|---|---|
| 🐧 Linux — Debian / Ubuntu | `EzooDownload_1.0.7_amd64.deb` | [تنزيل](https://github.com/kerlif/ezoo-download/releases/download/v1.0.7/EzooDownload_1.0.7_amd64.deb) |
| 🐧 Linux — محمول (أي توزيعة) | `EzooDownload-1.0.7-x86_64.AppImage` | [تنزيل](https://github.com/kerlif/ezoo-download/releases/download/v1.0.7/EzooDownload-1.0.7-x86_64.AppImage) |
| 🐧 Linux — Fedora / RHEL / openSUSE | `ezoo-download-1.0.7-1.x86_64.rpm` | [تنزيل](https://github.com/kerlif/ezoo-download/releases/download/v1.0.7/ezoo-download-1.0.7-1.x86_64.rpm) |
| 🌐 إضافة Chrome (موقّعة) | `ezoo-download-chrome-1.0.7.crx` | [تنزيل](https://github.com/kerlif/ezoo-download/releases/download/v1.0.7/ezoo-download-chrome-1.0.7.crx) |
| 🌐 إضافة Chrome (zip) | `ezoo-download-chrome-1.0.7.zip` | [تنزيل](https://github.com/kerlif/ezoo-download/releases/download/v1.0.7/ezoo-download-chrome-1.0.7.zip) |
| 🦊 إضافة Firefox | `ezoo-download-firefox-1.0.7.xpi` | [تنزيل](https://github.com/kerlif/ezoo-download/releases/download/v1.0.7/ezoo-download-firefox-1.0.7.xpi) |
| 🪟 Windows | ملف الإعداد `EzooDownload-Setup` | [صفحة الإصدارات](https://github.com/kerlif/ezoo-download/releases/tag/v1.0.7) |

> جميع ملفات الإصدار مرفقة مع بصمات SHA-256 في ملف `SHA256SUMS.txt` على [صفحة الإصدار](https://github.com/kerlif/ezoo-download/releases/tag/v1.0.7).
> نسخ لينكس تتضمن **ffmpeg مدمجًا داخل الحزمة** — لا حاجة لتثبيت أي شيء إضافي.

<p align="center">
  <img src="docs/screenshots/01-main-ar.png" alt="النافذة الرئيسية" width="32%" />
  <img src="docs/screenshots/02-progress-ar.png" alt="تقدّم التنزيل" width="32%" />
  <img src="docs/screenshots/03-add-dialog-ar.png" alt="إضافة تحميل" width="32%" />
</p>

---

## 📌 نبذة عن المشروع

**Ezoo Download** برنامج لنظام ويندوز كُتب من الصفر بلغة **Rust**، يجلب الملفات من الروابط التي يحددها المستخدم ويحفظها على جهازه.
يقسّم البرنامج الملف على عدة اتصالات، ويستأنف العمل من حيث توقف إذا انقطع الاتصال. وتدعم واجهته **ثماني لغات**: العربية والإنجليزية والفرنسية والإسبانية والألمانية والبرتغالية والتركية والروسية.

> [!IMPORTANT]
> يتحمّل المستخدم مسؤولية احترام حقوق الملكية وشروط استخدام أي محتوى يتعامل معه من خلال البرنامج.

---

## 🆕 ما الجديد في 1.0.7 — دعم لينكس الكامل

- 📦 **حزم لينكس جاهزة**: `.deb` و`AppImage` و`.rpm`، تُبنى تلقائيًا عند كل تحديث للشيفرة (GitHub Actions) وتُنشر في الإصدارات.
- 🎬 **ffmpeg مدمج داخل الحزمة**: دمج الفيديو والصوت وتحويل MP3 يعمل فورًا دون تثبيت أي شيء إضافي — مثالي للمبتدئين في لينكس.
- 🔔 **أيقونة Tray أصلية** بنظام StatusNotifierItem: تظهر بجوار الساعة في KDE Plasma وGNOME وXFCE، بقائمة كاملة، ويبقى البرنامج يعمل من الشريط عند إغلاق النافذة.
- 🚀 **تشغيل تلقائي** عبر XDG Autostart، وحوارات ملفات عبر xdg-desktop-portal (دون حاجة إلى GTK عند البناء)، ووضع Slint البرمجي للأجهزة دون OpenGL.
- 📡 **دعم DASH** عبر محلّل MPD مدمج (fluxDown)، مع دعم HLS كما في الأصل.
- 🔤 **الخطوط مدمجة**: خط InstaArabi للعربية وTimes New Roman لغيرها، يعملان على أي جهاز دون تثبيت مسبق.
- 🌐 **إضافات المتصفح** لكروم وفايرفوكس، وإضافة كروم **موقّعة بالمفتاح الأصلي** (نفس معرّف الإضافة دون تغيير).

---

## ✨ المزايا

| الميزة | التفاصيل |
|---|---|
| 🔀 **اتصالات متعددة** | تقسيم الملف على ما يصل إلى 32 اتصالًا في الوقت نفسه |
| ⏯️ **الاستئناف** | الإيقاف المؤقت والاستئناف في أي وقت، حتى بعد إغلاق البرنامج |
| 🌐 **إضافة للمتصفح** | استقبال الملفات من المتصفح، مع خطوات تركيب الإضافة من داخل البرنامج |
| 🎬 **الفيديو والصوت** | حفظ الفيديو والصوت من الصفحات وقوائم التشغيل مع اختيار الجودة |
| 🗂️ **تصنيف تلقائي** | وضع كل ملف في المجلد المناسب لنوعه |
| ⏰ **قوائم انتظار وجدولة** | قوائم انتظار، وتشغيل في موعد محدد، وإيقاف تشغيل الجهاز عند الانتهاء |
| 🐢 **تحديد السرعة** | حدّ عام للسرعة أو حدّ خاص بكل ملف |
| 🔎 **Grabber** | جمع الملفات من صفحة كاملة دفعة واحدة |
| 📋 **القوائم** | إضافة روابط متعددة دفعة واحدة، واستيراد القوائم وتصديرها |
| 🖱️ **التحديد المتعدد** | باستخدام Ctrl و Shift و Ctrl+A والسحب بالفأرة |
| 🌍 **ثماني لغات** | العربية، English، Français، Español، Deutsch، Português، Türkçe، Русский |

---

## 📦 التثبيت

### Windows 🪟

ثبّت البرنامج من **GitHub Releases** في صفحة المشروع.
يُثبَّت البرنامج للمستخدم الحالي ولا يحتاج إلى صلاحيات المسؤول.

### لينكس 🐧

```bash
# Debian / Ubuntu
sudo dpkg -i EzooDownload_1.0.7_amd64.deb

# Fedora / RHEL / openSUSE
sudo dnf install ./ezoo-download-1.0.7-1.x86_64.rpm

# AppImage (أي توزيعة)
chmod +x EzooDownload-1.0.7-x86_64.AppImage
./EzooDownload-1.0.7-x86_64.AppImage
```

للتثبيت والاستعمال وتفاصيل إضافة المتصفح واستكشاف الأخطاء، راجع **[دليل لينكس الكامل](packaging/linux/README-Linux.md)**.

---

## 🔨 البناء من الشيفرة المصدرية

### Windows

شغّل الملف **`BUILD.bat`**، وستجد ملف الإعداد `EzooDownload-Setup` في مجلد **`dist`**.

### لينكس

```bash
# كل شيء: deb + AppImage + rpm + الإضافات (ffmpeg مدمج تلقائيًا)
bash packaging/linux/build-linux.sh all

# الإضافات فقط (zip + xpi + crx عند توفر المفتاح)
bash packaging/linux/build-extensions.sh dist
```

المتطلبات: Rust (stable) وأدوات البناء المذكورة في سير العمل، وNode.js 22 لتوقيع إضافة كروم (crx3)، و`rpmbuild` لحزمة rpm.
كل دفع إلى `main` أو `linux-support` يبني كل شيء تلقائيًا، وكل وسم `v*` مطابق لإصدار `Cargo.toml` ينشر إصدارًا كاملًا بالأصول وبصمات SHA-256.

---

## ⚙️ متطلبات التشغيل

- **Windows 10 أو Windows 11** (إصدار 64-bit)
- **Linux** (إصدار 64-bit؛ التبعيات مذكورة في `packaging/linux/debian/control`)
- اتصال بالإنترنت

---

## 🔍 سلوك البرنامج واتصالاته

حرصًا على الوضوح، هذه هي المهام التي يؤديها البرنامج تلقائيًا:

| الوظيفة | التفاصيل |
|---|---|
| 🔄 **تحديث البرنامج** | عند التشغيل وكل 6 ساعات يقرأ البرنامج ملف `version.json` من GitHub Releases الخاصة بالمشروع. إذا وُجد إصدار جديد، يجلبه من GitHub Releases فقط، ويتحقق من بصمة SHA-256 الخاصة به، ثم يثبّته دون إظهار نوافذ ويعيد تشغيل نفسه. التحديث إلزامي، ويمكن البحث عنه فورًا من قائمة Help ← Check for updates. |
| 🧰 **الأدوات المساعدة** | يستخدم البرنامج أداتي yt-dlp و FFmpeg (الموجودتين في مجلد `tools`)، ويحدّثهما مرة يوميًا من صفحات إصداراتهما على GitHub. |
| 🔌 **الاتصال بالإضافة** | يفتح البرنامج منفذًا محليًا على `127.0.0.1` (الافتراضي 9614) لتتواصل معه إضافة المتصفح. هذا المنفذ غير متاح من الشبكة، ويرفض أي طلب لا يأتي من الإضافة. |
| 🍪 **ملفات تعريف الارتباط** | ترسل الإضافة إلى البرنامج ملفات تعريف الارتباط وعنوان الصفحة الخاصين بالملف المطلوب فقط، لكي تعمل الروابط التي تتطلب تسجيل الدخول. تنتقل هذه البيانات إلى البرنامج على الجهاز نفسه فقط. |
| 📋 **الحافظة** | يراقب البرنامج الحافظة، وإذا نُسخ رابط لملف يعرض إضافته. يمكن إيقاف ذلك من الإعدادات. |
| 🚀 **التشغيل مع ويندوز** | خيار اختياري أثناء التثبيت، ويمكن تغييره من الإعدادات. |
| 💾 **البيانات** | تُحفظ الإعدادات وقائمة الملفات في `%APPDATA%\Ezoo\EzooDownload\data`، ومنها بيانات تسجيل الدخول للمواقع والوكيل (Proxy) إن أدخلها المستخدم، وهي محفوظة نصًا عاديًا دون تشفير. |

البرنامج **لا يجمع ولا يرسل** أي بيانات استخدام أو إحصاءات إلى أي جهة.

---

## 🛡️ ملاحظة حول برامج الحماية

البرنامج غير موقّع بشهادة رقمية حتى الآن، لذا قد تُظهر بعض برامج الحماية أو **Windows SmartScreen** تحذيرًا، كما يحدث مع أي برنامج جديد.
الشيفرة المصدرية كاملة متاحة هنا لمن يرغب في مراجعتها.

---

## 🔐 الإبلاغ عن مشكلة أمنية

إذا وجدت مشكلة أمنية في البرنامج، يُرجى الإبلاغ عنها عبر **Issues** في صفحة المشروع.

---

## 🗂️ هيكل المشروع

```
├─ crates/engine/      المحرك
├─ crates/app/         البرنامج (واجهة Slint)
│  ├─ ui/              النوافذ والأيقونات والخطوط
│  └─ lang/            ترجمات الواجهة
├─ extension/          إضافة المتصفح
├─ packaging/linux/    حزم لينكس (.deb وAppImage وrpm) + دليل لينكس
├─ docs/screenshots/   لقطات الواجهة
├─ .github/workflows/  الأتمتة: بناء لينكس وويندوز
└─ BUILD.bat           البناء (ويندوز)
```

---

## 👨‍💻 المطوّرون

- **Ahmed Ezzat** — Ezoo Tech — المشروع الأصلي.
- **kerlif** — دعم لينكس والحزم والأتمتة والتطوير المنفصل في هذا المستودع.

---

## © حقوق الملكية

© 2026 **Ahmed Ezzat** — جميع الحقوق محفوظة.

يستخدم البرنامج مكونات مفتوحة المصدر، وتراخيصها موجودة في ملف `THIRD-PARTY-NOTICES.txt` — بما فيها نسخة ffmpeg المدمجة في حزم لينكس (GPLv3).
