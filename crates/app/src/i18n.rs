//! The texts made in Rust (statuses, messages) in the language of the
//! program. Arabic is the table below; the other languages are the files
//! lang/rs/<code>.json (English text -> translation). The window texts
//! themselves are translated by Slint (lang/<code>/LC_MESSAGES/*.po).

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

/// The languages of the program: code and own name, in the order of the
/// Language list in Options.
pub const LANGS: [(&str, &str); 8] = [
    ("en", "English"),
    ("ar", "العربية"),
    ("fr", "Français"),
    ("es", "Español"),
    ("de", "Deutsch"),
    ("pt", "Português"),
    ("tr", "Türkçe"),
    ("ru", "Русский"),
];

static CURRENT: AtomicUsize = AtomicUsize::new(0);

/// A language code from LANGS, or "" = the language of Windows.
pub fn effective(setting: &str) -> &'static str {
    if let Some((code, _)) = LANGS.iter().find(|(c, _)| *c == setting) {
        return code;
    }
    let sys = sys_locale::get_locale().unwrap_or_default().to_ascii_lowercase();
    LANGS.iter().map(|(c, _)| *c).find(|c| sys.starts_with(c)).unwrap_or("en")
}

/// Place of a language in LANGS (0 = English for anything unknown).
pub fn index_of(code: &str) -> usize {
    LANGS.iter().position(|(c, _)| *c == code).unwrap_or(0)
}

/// Switch every window and every Rust text to this language.
pub fn apply(lang: &str) {
    CURRENT.store(index_of(lang), Ordering::Relaxed);
    let _ = slint::select_bundled_translation(LANGS[index_of(lang)].0);
}

pub fn current() -> &'static str {
    LANGS[CURRENT.load(Ordering::Relaxed) % LANGS.len()].0
}

#[allow(dead_code)]
pub fn is_arabic() -> bool {
    current() == "ar"
}

fn table(code: &str) -> Option<&'static HashMap<String, String>> {
    static FR: OnceLock<HashMap<String, String>> = OnceLock::new();
    static ES: OnceLock<HashMap<String, String>> = OnceLock::new();
    static DE: OnceLock<HashMap<String, String>> = OnceLock::new();
    static PT: OnceLock<HashMap<String, String>> = OnceLock::new();
    static TR: OnceLock<HashMap<String, String>> = OnceLock::new();
    static RU: OnceLock<HashMap<String, String>> = OnceLock::new();
    let (cell, src) = match code {
        "fr" => (&FR, include_str!("../lang/rs/fr.json")),
        "es" => (&ES, include_str!("../lang/rs/es.json")),
        "de" => (&DE, include_str!("../lang/rs/de.json")),
        "pt" => (&PT, include_str!("../lang/rs/pt.json")),
        "tr" => (&TR, include_str!("../lang/rs/tr.json")),
        "ru" => (&RU, include_str!("../lang/rs/ru.json")),
        _ => return None,
    };
    Some(cell.get_or_init(|| serde_json::from_str(src).unwrap_or_default()))
}

/// The text in the current language.
pub fn t(en: &str) -> String {
    match current() {
        "en" => {}
        "ar" => {
            if let Some(ar) = lookup(en) {
                return ar.to_string();
            }
        }
        code => {
            if let Some(v) = table(code).and_then(|m| m.get(en)) {
                return v.clone();
            }
        }
    }
    en.to_string()
}

/// A text with "{}" places, filled in order.
pub fn tf(en: &str, args: &[&dyn std::fmt::Display]) -> String {
    let template = t(en);
    let mut out = String::new();
    let mut rest = template.as_str();
    let mut i = 0;
    while let Some(p) = rest.find("{}") {
        out.push_str(&rest[..p]);
        if let Some(a) = args.get(i) {
            out.push_str(&a.to_string());
        }
        i += 1;
        rest = &rest[p + 2..];
    }
    out.push_str(rest);
    out
}

fn lookup(en: &str) -> Option<&'static str> {
    AR.iter().find(|(k, _)| *k == en).map(|(_, v)| *v)
}

const AR: &[(&str, &str)] = &[
    // statuses
    ("Queued", "في الانتظار"),
    ("Connecting…", "جارٍ الاتصال…"),
    ("Downloading", "جارٍ التحميل"),
    ("Assembling…", "جارٍ التجميع…"),
    ("Paused", "متوقف مؤقتاً"),
    ("Complete", "مكتمل"),
    ("Error", "خطأ"),
    ("Merging…", "جارٍ الدمج…"),
    ("{}/{} parts", "{}/{} جزء"),
    ("{}: video + audio", "{}: فيديو + صوت"),
    ("{}: stream", "{}: بث"),
    ("Receiving data…", "جارٍ استقبال البيانات…"),
    ("Joining the parts…", "جارٍ تجميع الأجزاء…"),
    ("Unknown", "غير معروف"),
    ("Yes", "نعم"),
    ("No", "لا"),
    ("Finished", "انتهى"),
    ("Waiting", "في الانتظار"),
    ("{} downloading  ·  {}/s", "{} قيد التحميل  ·  {}/ث"),
    ("Ready  ·  {} files in the list", "جاهز  ·  {} ملف في القائمة"),
    ("Speed limiter: on ({} KB/s)", "محدد السرعة: يعمل ({} كيلو/ث)"),
    ("Speed limiter: off", "محدد السرعة: متوقف"),
    // categories (shown names)
    ("Compressed", "مضغوطة"),
    ("Documents", "مستندات"),
    ("Music", "موسيقى"),
    ("Programs", "برامج"),
    ("Video", "فيديو"),
    ("Images", "صور"),
    ("General", "عام"),
    // download file info
    ("Paste a link and press Check", "الصق رابطاً واضغط فحص"),
    ("Unknown size", "حجم غير معروف"),
    ("+ audio", "+ صوت"),
    ("Video + audio, joined after download", "فيديو + صوت، يُدمجان بعد التحميل"),
    ("Video stream (HLS)", "بث فيديو (HLS)"),
    ("Resume supported", "يدعم الاستئناف"),
    ("No resume support", "لا يدعم الاستئناف"),
    ("This address is already in the download list.", "هذا الرابط موجود بالفعل في قائمة التحميل."),
    (
        "This address opens a web page, not a file. The link may have expired or need a login.",
        "هذا الرابط يفتح صفحة ويب وليس ملفاً. ربما انتهت صلاحيته أو يحتاج تسجيل دخول.",
    ),
    ("Please enter a valid http:// or https:// address", "من فضلك اكتب رابطاً صحيحاً يبدأ بـ http:// أو https://"),
    // messages
    (
        "Remove every finished download from the list?\n(The files stay on the disk.)",
        "حذف كل التحميلات المكتملة من القائمة؟\n(الملفات تبقى على القرص.)",
    ),
    (
        "A short message about Ezoo Download was copied to the clipboard.\nPaste it in a chat or an e-mail to a friend.",
        "تم نسخ رسالة قصيرة عن Ezoo Download إلى الحافظة.\nالصقها في محادثة أو بريد لصديقك.",
    ),
    ("Ezoo Download {}\n\nThe video downloader (yt-dlp) is up to date.\n\n{}", "Ezoo Download {}\n\nأداة الفيديو (yt-dlp) محدّثة.\n\n{}"),
    ("Could not update yt-dlp:\n\n{}", "تعذّر تحديث yt-dlp:\n\n{}"),
    ("Download these {} files again from the beginning?", "إعادة تحميل هذه الملفات ({}) من البداية؟"),
    ("Download “{}” again from the beginning?", "إعادة تحميل “{}” من البداية؟"),
    ("No valid http:// or https:// links", "لا توجد روابط صحيحة (http:// أو https://)"),
    ("The address must start with http:// or https://", "يجب أن يبدأ الرابط بـ http:// أو https://"),
    ("Put a * in the address where the number goes", "ضع * في الرابط مكان الرقم"),
    ("From must be a letter", "خانة «من» يجب أن تكون حرفاً"),
    ("To must be a letter", "خانة «إلى» يجب أن تكون حرفاً"),
    ("From must come before To", "«من» يجب أن تأتي قبل «إلى»"),
    ("From must be a number", "خانة «من» يجب أن تكون رقماً"),
    ("To must be a number", "خانة «إلى» يجب أن تكون رقماً"),
    ("From must be smaller than To", "«من» يجب أن تكون أصغر من «إلى»"),
    ("That's more than 10000 files", "هذا أكثر من 10000 ملف"),
    ("{} files\n\nFirst:  {}", "{} ملف\n\nالأول:  {}"),
    ("\nSecond:  {}", "\nالثاني:  {}"),
    ("\nLast:  {}", "\nالأخير:  {}"),
    ("Could not change where it is saved: {}", "تعذّر تغيير مكان الحفظ: {}"),
    ("Could not move the file:\n\n{}", "تعذّر نقل الملف:\n\n{}"),
    ("SHA-256 of {}:\n\n{}\n\n(copied to the clipboard)", "SHA-256 للملف {}:\n\n{}\n\n(تم النسخ إلى الحافظة)"),
    ("Could not read the file: {}", "تعذّرت قراءة الملف: {}"),
    ("Delete downloads", "حذف التحميلات"),
    ("Delete download", "حذف التحميل"),
    (
        "Remove these {} downloads from the download list?\n\nOnly the list entries are removed: the downloaded files stay on your computer.\n(Parts of unfinished downloads are cleaned up.)",
        "إزالة هذه التحميلات ({}) من قائمة التحميل؟\n\nستُزال من القائمة فقط، وتبقى الملفات المحمّلة على جهازك كما هي.\n(تُحذف الأجزاء المؤقتة للتحميلات غير المكتملة.)",
    ),
    (
        "Remove “{}” from the download list?\n\nOnly the list entry is removed: the file stays on your computer.",
        "إزالة “{}” من قائمة التحميل؟\n\nستُزال من القائمة فقط، ويبقى الملف على جهازك كما هو.",
    ),
    (
        "Delete the unfinished download “{}”?\nThe partly downloaded data will be removed.",
        "حذف التحميل غير المكتمل “{}”؟\nسيتم حذف الجزء الذي تم تحميله.",
    ),
    (
        "The download limit of {} MB every {} hours was reached, so the downloads were stopped.\nThey continue by themselves when the limit allows it again (Options > Connection).",
        "تم الوصول لحد التحميل ({} ميجابايت كل {} ساعة) فتوقفت التحميلات.\nستكمل وحدها عندما يسمح الحد مرة أخرى (الخيارات > الاتصال).",
    ),
    ("Give the category a name", "اكتب اسماً للتصنيف"),
    ("A category with this name already exists", "يوجد تصنيف بهذا الاسم بالفعل"),
    (
        "This category comes with Ezoo Download and can't be deleted (you can change its file types in Properties).",
        "هذا التصنيف أساسي في Ezoo Download ولا يمكن حذفه (يمكنك تغيير أنواع ملفاته من الخصائص).",
    ),
    (
        "Delete the category “{}”?\nIts files stay where they are and move to their file type's category.",
        "حذف التصنيف “{}”؟\nملفاته تبقى مكانها وتنتقل لتصنيف نوعها.",
    ),
    ("Found: {}", "تم العثور: {}"),
    ("Not found", "غير موجود"),
    ("The file was downloaded successfully", "تم تحميل الملف بنجاح"),
    ("{} file  ·  {}", "ملف {}  ·  {}"),
    ("Tip {} of {}", "نصيحة {} من {}"),
    ("Could not save the list: {}", "تعذّر حفظ القائمة: {}"),
    ("No download addresses were found in this file.", "لم يتم العثور على روابط تحميل في هذا الملف."),
    (
        "{} download(s) were added to the main download queue.\nStart them with Start Queue, or one by one with Resume.",
        "تمت إضافة {} تحميل إلى قائمة الانتظار الرئيسية.\nابدأها بـ «بدء القائمة»، أو واحداً واحداً بـ «استئناف».",
    ),
    ("shut down the computer", "إيقاف تشغيل الكمبيوتر"),
    ("hibernate the computer", "إسبات الكمبيوتر"),
    ("put the computer to sleep", "وضع الكمبيوتر في وضع السكون"),
    ("Type the address of the start page (http:// or https://)", "اكتب رابط صفحة البداية (http:// أو https://)"),
    ("Starting…", "جارٍ البدء…"),
    ("Check the files you want first", "حدّد الملفات التي تريدها أولاً"),
    ("{} file(s) checked", "{} ملف محدد"),
    ("{} file(s) added to the queue", "تمت إضافة {} ملف إلى القائمة"),
    ("{} of {} pages explored  ·  {} files found", "تم استكشاف {} من {} صفحة  ·  {} ملف"),
    ("  ·  done", "  ·  انتهى"),
    ("Give the queue a name", "اكتب اسماً للقائمة"),
    ("The start time must look like 02:30", "وقت البدء يجب أن يكون مثل 02:30"),
    ("The stop time must look like 08:00", "وقت الإيقاف يجب أن يكون مثل 08:00"),
    ("The date must look like 2026-12-31", "التاريخ يجب أن يكون مثل 2026-12-31"),
    ("Could not create the queue: {}", "تعذّر إنشاء القائمة: {}"),
    ("The main download queue can't be deleted.", "لا يمكن حذف قائمة التحميل الرئيسية."),
    ("Delete the queue “{}”?\nIts files move to the main download queue.", "حذف القائمة “{}”؟\nملفاتها تنتقل إلى قائمة التحميل الرئيسية."),
    ("Downloaded in the current period: {}", "تم تحميل في الفترة الحالية: {}"),
    (
        "When the limit is reached the downloads stop, and go on by themselves later.",
        "عند الوصول للحد تتوقف التحميلات، وتكمل وحدها لاحقاً.",
    ),
    ("Choose a download folder (Save To)", "اختر مجلد التحميل (الحفظ في)"),
    ("Type the proxy address (Proxy / Server)", "اكتب عنوان البروكسي (البروكسي / الخادم)"),
    ("The remembered category folders were cleared.", "تم مسح مجلدات التصنيفات المحفوظة."),
    ("Type the server, e.g. example.com", "اكتب الخادم، مثلاً example.com"),
    ("Type the site and the user name", "اكتب الموقع واسم المستخدم"),
    ("Could not save: {}", "تعذّر الحفظ: {}"),
    (
        "Restart Ezoo Download to use the new port. Set the same port in the browser extension.",
        "أعد تشغيل Ezoo Download لاستخدام المنفذ الجديد، واضبط نفس المنفذ في إضافة المتصفح.",
    ),
    (
        "Paste the playlist address (https://…/playlist?list=…)",
        "الصق رابط قائمة التشغيل (https://…/playlist?list=…)",
    ),
    ("Getting the list of videos… (a few seconds)", "جارٍ جلب قائمة الفيديوهات… (ثوانٍ قليلة)"),
    ("No videos were found at this address.", "لم يتم العثور على فيديوهات في هذا الرابط."),
    ("Could not get the list: {}", "تعذّر جلب القائمة: {}"),
    ("Check the videos you want first", "حدّد الفيديوهات التي تريدها أولاً"),
    ("Choose where to save the videos", "اختر مكان حفظ الفيديوهات"),
    ("{} of {} videos checked", "{} من {} فيديو محدد"),
    ("{} videos", "{} فيديو"),
    ("Click to add", "اضغط للإضافة"),
    ("Ezoo Download – download manager", "Ezoo Download – برنامج تحميل"),
    ("Click to add from the store", "اضغط للإضافة من المتجر"),
    ("{} is not installed on this computer.", "{} غير مثبّت على هذا الكمبيوتر."),
    ("{} opened the store page: click \"Add\" / \"Get\" there.", "فتح {} صفحة المتجر: اضغط \"إضافة\" / \"Get\" هناك."),
    ("Could not start the update:\n\n{}", "تعذّر بدء التحديث:\n\n{}"),
    ("A new version is being prepared. It is installed by itself when it is ready.", "يجري تجهيز إصدار جديد، وسيُثبَّت تلقائياً عندما يكون جاهزاً."),
    ("Could not check for updates:\n\n{}", "تعذّر البحث عن تحديثات:\n\n{}"),
    ("Ezoo Download is up to date (version {}).", "Ezoo Download محدَّث (الإصدار {})."),
    ("Version {} is available. This copy was not installed with the setup, so it cannot update itself.", "الإصدار {} متاح. هذه النسخة لم تُثبَّت بملف الإعداد، لذلك لا يمكنها تحديث نفسها."),
    ("Version {} is available. It is being prepared now and installed by itself; Ezoo Download opens again after that.", "الإصدار {} متاح. يجري تجهيزه الآن وتثبيته تلقائياً، ثم يُفتح Ezoo Download من جديد."),
    ("Could not get the update:\n\n{}", "تعذّر الحصول على التحديث:\n\n{}"),
    (
        "The extension folder was not found next to EzooDownload.exe. Install Ezoo Download again.",
        "لم يتم العثور على مجلد الإضافة بجانب EzooDownload.exe. ثبّت Ezoo Download مرة أخرى.",
    ),
    (
        "Firefox opened its add-ons debugging page.\n1. Click \"Load Temporary Add-on…\".\n2. Paste the folder path (Ctrl+V, it is already copied) and open the file \"manifest.json\" in it.\n\nFirefox removes add-ons loaded this way when it closes. To keep it for good the extension must be signed by Mozilla (free) – ask for the signed version.",
        "فتح Firefox صفحة تصحيح الإضافات.\n1. اضغط \"Load Temporary Add-on…\".\n2. الصق مسار المجلد (Ctrl+V، تم نسخه بالفعل) وافتح ملف \"manifest.json\" بداخله.\n\nيحذف Firefox الإضافات المحمّلة بهذه الطريقة عند إغلاقه. لتبقى دائماً يجب أن توقّعها Mozilla (مجاناً) – اطلب النسخة الموقّعة.",
    ),
    (
        "{} opened its extensions page.\n1. Turn on \"Developer mode\" (a switch on that page).\n2. Click \"Load unpacked\".\n3. Paste the folder path (Ctrl+V – it is already copied) and click \"Select folder\".\n\nThat's all: the Ezoo Download icon appears next to the address bar. You do this only once.",
        "فتح {} صفحة الإضافات.\n1. شغّل \"Developer mode\" (وضع المطوّر) من المفتاح الموجود في الصفحة.\n2. اضغط \"Load unpacked\".\n3. الصق مسار المجلد (Ctrl+V – تم نسخه بالفعل) واضغط \"Select folder\".\n\nهذا كل شيء: ستظهر أيقونة Ezoo Download بجانب شريط العنوان. تفعل هذا مرة واحدة فقط.",
    ),
    // tips
    (
        "Copy a download link anywhere (Ctrl+C) and Ezoo Download offers to download it.",
        "انسخ أي رابط تحميل (Ctrl+C) وسيعرض عليك Ezoo Download تحميله.",
    ),
    (
        "Play a video on a web page and click \"Download this video\" above it to choose the quality.",
        "شغّل أي فيديو في صفحة ويب واضغط \"Download this video\" فوقه لتختار الجودة.",
    ),
    (
        "Right click a web page in Chrome or Edge and choose \"Download all links with Ezoo Download\" to pick files from the page.",
        "اضغط كليك يمين على أي صفحة في كروم أو إيدج واختر \"Download all links with Ezoo Download\" لتختار ملفات من الصفحة.",
    ),
    (
        "Every download uses up to 32 connections. If a server is slow with many, add it to Options > Connection > Exceptions.",
        "كل تحميل يستخدم حتى 32 اتصالاً. إذا كان خادم ما بطيئاً مع اتصالات كثيرة، أضفه في الخيارات > الاتصال > الاستثناءات.",
    ),
    (
        "Make a queue (Queues > New queue) and set \"1 file at the same time\" to download files one after another.",
        "أنشئ قائمة انتظار واضبط \"ملف واحد في نفس الوقت\" لتحميل الملفات واحداً تلو الآخر.",
    ),
    (
        "The Scheduler can start a queue at night and shut down the computer when everything is finished.",
        "المجدول يستطيع بدء قائمة بالليل وإيقاف الكمبيوتر عند انتهاء كل شيء.",
    ),
    (
        "Downloads > When all downloads finish lets you shut down, sleep or hibernate the computer after the last file.",
        "التحميلات > عند انتهاء كل التحميلات تتيح لك إيقاف الكمبيوتر أو السكون أو الإسبات بعد آخر ملف.",
    ),
    (
        "The speed limiter on the toolbar keeps your internet usable for other things while downloading.",
        "محدد السرعة في شريط الأدوات يبقي الإنترنت متاحاً لأشياء أخرى أثناء التحميل.",
    ),
    (
        "Add Batch > \"Address with a pattern\" downloads img001.jpg … img100.jpg from one address with a *.",
        "إضافة مجموعة > \"رابط بنمط\" يحمّل img001.jpg … img100.jpg من رابط واحد فيه *.",
    ),
    (
        "An old link expired? Right click the download > \"Refresh download address\" and continue from where it stopped.",
        "انتهت صلاحية رابط قديم؟ كليك يمين على التحميل > \"تحديث رابط التحميل\" وأكمل من حيث توقف.",
    ),
    (
        "Right click a category in the sidebar > Properties to change its file types and its own folder.",
        "كليك يمين على تصنيف في الشريط الجانبي > الخصائص لتغيير أنواع ملفاته ومجلده.",
    ),
    (
        "View > Columns shows more columns, like Date Added, Save To and Referer.",
        "عرض > الأعمدة يظهر أعمدة أكثر مثل تاريخ الإضافة والحفظ في والمُحيل.",
    ),
    (
        "Tasks > Run site grabber collects every image, video or archive of a web site.",
        "المهام > تشغيل جامع المواقع يجمع كل صور أو فيديوهات أو ملفات موقع ما.",
    ),
    (
        "Hold Alt while clicking a link in the browser to let the browser download it by itself.",
        "اضغط Alt أثناء الضغط على رابط في المتصفح ليحمّله المتصفح بنفسه.",
    ),
    // 1.0.4
    (
        "Firefox opened the Ezoo Download add-on.\n1. Click \"Add\" in the message Firefox shows.\n2. Then click \"OK\".\n\nThe add-on is signed by Mozilla, so it stays in Firefox after you close it and updates by itself.",
        "فتح فايرفوكس إضافة Ezoo Download.\n1. اضغط \"إضافة\" في الرسالة التي يعرضها فايرفوكس.\n2. ثم اضغط \"حسنًا\".\n\nالإضافة موقّعة من Mozilla، لذلك تبقى في فايرفوكس بعد إغلاقه وتتحدّث وحدها.",
    ),
    ("Ezoo Download was updated to version {} by itself.", "تم تحديث Ezoo Download تلقائيًا إلى الإصدار {}."),
    ("What's new:", "الجديد في هذا الإصدار:"),
    (
        "A \"Download this video\" button on every video of a page, and you can drag it anywhere on the video.",
        "زر \"تحميل هذا الفيديو\" على كل فيديو في الصفحة، ويمكنك سحبه إلى أي مكان على الفيديو.",
    ),
    (
        "Videos with several sound tracks: the original language is downloaded, not a dubbed one.",
        "الفيديوهات ذات المسارات الصوتية المتعددة: يُحمَّل الصوت باللغة الأصلية وليس المدبلج.",
    ),
    (
        "New languages: French, Spanish, German, Portuguese, Turkish and Russian.",
        "لغات جديدة: الفرنسية والإسبانية والألمانية والبرتغالية والتركية والروسية.",
    ),
    (
        "Five new themes and a window opacity setting (View > Theme).",
        "خمسة مظاهر جديدة وإعداد لشفافية النافذة (عرض > المظهر).",
    ),
    (
        "Brave is supported (Help > Add Ezoo Download to my browser).",
        "دعم متصفح Brave (مساعدة > إضافة Ezoo Download إلى متصفحي).",
    ),
    ("Firefox: the add-on stays in Firefox after it is closed.", "فايرفوكس: الإضافة تبقى بعد إغلاق المتصفح."),
    (
        "The browser add-in window is bigger; its settings moved to the add-in's own settings page.",
        "نافذة إضافة المتصفح أكبر، وإعداداتها انتقلت إلى صفحة الإعدادات الخاصة بالإضافة.",
    ),
    ("Windows of the program open in front of the main window.", "نوافذ البرنامج تفتح أمام النافذة الرئيسية."),
    (
        "A new playlist icon, and a smaller icon on the window title.",
        "أيقونة جديدة لقوائم التشغيل، وأيقونة أصغر في شريط عنوان النافذة.",
    ),
    ("Support Ezoo Download with PayPal from Help > About.", "ادعم Ezoo Download عبر PayPal من مساعدة > حول."),
    // 1.0.5
    (
        "The main window can be maximized to the full screen.",
        "نافذة البرنامج الرئيسية يمكن تكبيرها لملء الشاشة.",
    ),
    (
        "Columns of the list can be resized and moved by dragging their titles.",
        "أعمدة القائمة يمكن تكبيرها وتصغيرها ونقلها بسحب عناوينها.",
    ),
    (
        "Toolbar names are bold, and the size of the video button can be chosen in the add-in's window.",
        "أسماء أزرار الشريط بخط عريض، ويمكن اختيار حجم زر الفيديو من نافذة الإضافة.",
    ),
    (
        "Pictures",
        "الصور",
    ),
    (
        "Checking for updates…",
        "جارٍ البحث عن تحديثات…",
    ),
    (
        "This is version {}.",
        "هذا هو الإصدار {}.",
    ),
    (
        "Could not check for updates",
        "تعذّر البحث عن تحديثات",
    ),
    (
        "Ezoo Download is up to date",
        "Ezoo Download محدَّث",
    ),
    (
        "Version {} is the newest version.",
        "الإصدار {} هو أحدث إصدار.",
    ),
    (
        "Version {} is available",
        "الإصدار {} متاح",
    ),
    (
        "This copy was not installed with the setup, so it cannot update itself.",
        "هذه النسخة لم تُثبَّت ببرنامج الإعداد، لذلك لا يمكنها تحديث نفسها.",
    ),
    (
        "Downloading version {}…",
        "جارٍ تحميل الإصدار {}…",
    ),
    (
        "Installing version {}…",
        "جارٍ تثبيت الإصدار {}…",
    ),
    (
        "Ezoo Download closes now and opens again when it is installed.",
        "سيُغلق Ezoo Download الآن ويفتح من جديد بعد التثبيت.",
    ),
    (
        "Could not get the update",
        "تعذّر الحصول على التحديث",
    ),
    (
        "Text size, font and a picture behind the main window (Options > Appearance).",
        "حجم الخط والخط وصورة خلف النافذة الرئيسية (الخيارات > المظهر).",
    ),
    (
        "Windows open in the middle of the screen; only the main window can be maximized.",
        "النوافذ تفتح في منتصف الشاشة، والنافذة الرئيسية وحدها يمكن تكبيرها.",
    ),
    (
        "Drag a finished file from the list onto the desktop or into a folder.",
        "اسحب ملفًا مكتملًا من القائمة إلى سطح المكتب أو إلى أي مجلد.",
    ),
    (
        "The progress of a download shows on its taskbar button.",
        "تقدّم التحميل يظهر على زر النافذة في شريط المهام.",
    ),
    (
        "Check for updates shows the check, the download and the installation in its own window.",
        "البحث عن تحديثات يعرض البحث والتحميل والتثبيت في نافذة خاصة.",
    ),
    (
        "Playlist videos are saved with their own original titles and their original sound.",
        "فيديوهات قوائم التشغيل تُحفظ بعناوينها الأصلية وصوتها الأصلي.",
    ),
    (
        "Closing a download's window (X) stops the download; Resume opens it again.",
        "إغلاق نافذة التحميل (X) يوقف التحميل، و«استئناف» يفتحها من جديد.",
    ),
    (
        "The download button on videos in the browser is smaller, and its size can be changed in the add-in's settings.",
        "زر التحميل على الفيديوهات في المتصفح أصبح أصغر، ويمكن تغيير حجمه من إعدادات الإضافة.",
    ),
    (
        "Updates are signed: Ezoo Download installs only updates made by its developer.",
        "التحديثات أصبحت موقّعة: Ezoo Download لا يثبّت إلا التحديثات الصادرة من مطوّره.",
    ),
    (
        "Downloaded files are marked as coming from the internet, so Windows warns before opening an unsafe file.",
        "الملفات المحمّلة تُعلَّم بأنها من الإنترنت، فيحذّرك ويندوز قبل فتح ملف غير آمن.",
    ),
    (
        "Saved passwords and site logins are encrypted with your Windows account.",
        "كلمات المرور وبيانات دخول المواقع المحفوظة أصبحت مشفّرة بحساب ويندوز الخاص بك.",
    ),
    (
        "The video helpers (yt-dlp and ffmpeg) are checked before they are used.",
        "أدوات الفيديو (yt-dlp و ffmpeg) يتم التحقق منها قبل استخدامها.",
    ),
    (
        "More protection against harmful web pages and links.",
        "حماية أكبر من صفحات الويب والروابط الضارة.",
    ),
    // 1.0.6
    ("Torrents: open a .torrent file or a magnet link and choose which of its files to download.", "التورنت: افتح ملف ‎.torrent أو رابط magnet واختر أي ملفاته تريد تحميلها."),
    ("A dropped internet connection no longer stops a download: it waits and goes on by itself from where it was.", "انقطاع الإنترنت لم يعد يوقف التحميل: ينتظر ثم يكمل وحده من حيث توقف."),
    ("An expired link or a removed file is explained in plain words, with what to do about it.", "الرابط المنتهي أو الملف المحذوف يُشرح بكلمات بسيطة مع ما يجب فعله."),
    ("Watch a video or listen to a song while it is still downloading (right-click > Play while downloading).", "شاهد فيديو أو استمع لأغنية وهي ما زالت تُحمَّل (زر الفأرة الأيمن > تشغيل أثناء التحميل)."),
    ("YouTube videos can be saved as MP3 files.", "يمكن حفظ فيديوهات يوتيوب كملفات MP3."),
    ("Qualities are shown in plain words too: SD, HD, Full HD, 2K, 4K.", "الجودات تظهر أيضًا بكلمات واضحة: SD وHD وFull HD و2K و4K."),
    ("The video button in the browser lists every quality with its format, length and size, has a size slider, and works on videos in frames, in full screen and in more players.", "زر الفيديو في المتصفح يعرض كل جودة بنوعها ومدتها وحجمها، وله شريط لتغيير حجمه، ويعمل على الفيديوهات داخل الإطارات وفي ملء الشاشة وفي مشغلات أكثر."),
    ("Videos that need a login or an age check use your browser's sign-in, also in playlists.", "الفيديوهات التي تحتاج تسجيل دخول أو تأكيد العمر تستخدم تسجيل دخولك في المتصفح، وفي قوائم التشغيل أيضًا."),
    ("Toolbar buttons explain what they do when the mouse rests on them.", "أزرار شريط الأدوات تشرح وظيفتها عند وضع الفأرة عليها."),
    ("A download's window can be hidden next to the clock.", "يمكن إخفاء نافذة التحميل بجوار الساعة."),
    ("File icons are easier to tell apart.", "أيقونات الملفات أصبحت أسهل في التمييز."),
    ("Menus show one clear mark on the chosen option.", "القوائم تعرض علامة واحدة واضحة على الاختيار المحدد."),
    ("The window of a finished download no longer offers \"Open with…\", so a file can't be changed by mistake.", "نافذة التحميل المكتمل لم تعد تعرض \"فتح باستخدام…\"، فلا يتغير الملف بالخطأ."),
    ("Fixed a crash around the Tips window.", "تم إصلاح توقف مفاجئ كان يحدث مع نافذة النصائح."),
    ("Only videos and sound files can be played while they download.", "يمكن تشغيل الفيديوهات والملفات الصوتية فقط أثناء تحميلها."),
    ("This video comes in many small pieces that are joined at the end: it can be played when the download is done.", "هذا الفيديو يأتي في أجزاء صغيرة كثيرة تُجمع في النهاية: يمكن تشغيله بعد انتهاء التحميل."),
    ("Nothing to play yet: wait until a part of the file is downloaded, then try again.", "لا يوجد ما يمكن تشغيله بعد: انتظر حتى يُحمَّل جزء من الملف ثم حاول مرة أخرى."),
    ("Could not open the player: {}", "تعذر فتح المشغل: {}"),
    ("This video needs a login or an age check. Open it once in the browser where you are signed in (with the Ezoo Download extension), then try again.", "هذا الفيديو يحتاج تسجيل دخول أو تأكيد العمر. افتحه مرة في المتصفح الذي سجلت فيه دخولك (مع إضافة Ezoo Download) ثم حاول مرة أخرى."),
    ("The link has expired or the file was removed. Right-click the download and use \"Refresh download address\".", "انتهت صلاحية الرابط أو حُذف الملف. اضغط بزر الفأرة الأيمن على التحميل واستخدم \"تحديث رابط التحميل\"."),
    ("Waiting for connection…", "في انتظار الاتصال…"),
    ("Getting the file list…", "جارٍ جلب قائمة الملفات…"),
    ("Torrent: {} peers", "تورنت: {} أجهزة متصلة"),
    ("Torrent", "تورنت"),
    ("No internet connection - it will continue by itself when it is back…", "لا يوجد اتصال بالإنترنت - سيكمل التحميل وحده عند عودته…"),
    ("{} of {} files checked", "تم تحديد {} من {} ملفات"),
    ("Magnet link", "رابط magnet"),
    ("The file list comes from the other computers when it starts", "قائمة الملفات تأتي من الأجهزة الأخرى عند البدء"),
    ("This is not a magnet link or a .torrent file", "هذا ليس رابط magnet أو ملف ‎.torrent"),
    ("This .torrent file can't be read: {}", "تعذرت قراءة ملف ‎.torrent هذا: {}"),
    ("{} in {} files", "{} في {} ملفات"),
    ("Check at least one file", "حدد ملفًا واحدًا على الأقل"),
    ("Choose where to save it", "اختر مكان الحفظ"),
    ("Show download progress", "عرض تقدّم التحميل"),
    ("Stop download", "إيقاف التحميل"),
    ("This torrent is already running (it may still be sharing its files). Wait a little, or exit Ezoo Download and open it again.", "هذا التورنت يعمل بالفعل (قد يكون ما زال يشارك ملفاته). انتظر قليلًا، أو اخرج من Ezoo Download وافتحه مرة أخرى."),
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fills_places() {
        assert_eq!(tf("Tip {} of {}", &[&1, &14]), "Tip 1 of 14");
        CURRENT.store(index_of("ar"), Ordering::Relaxed);
        assert_eq!(tf("Tip {} of {}", &[&1, &14]), "نصيحة 1 من 14");
        assert_eq!(t("not in the list"), "not in the list");
        CURRENT.store(index_of("fr"), Ordering::Relaxed);
        assert_eq!(tf("Tip {} of {}", &[&1, &14]), "Astuce 1 sur 14");
        CURRENT.store(0, Ordering::Relaxed);
    }

    #[test]
    fn every_language_has_every_text() {
        // the tables are read from JSON: a broken file would silently fall back to English
        for (code, _) in LANGS.iter().skip(2) {
            let m = table(code).expect("table");
            assert!(m.len() > 150, "{code}: only {} texts", m.len());
            for (en, tr) in m {
                assert_eq!(en.matches("{}").count(), tr.matches("{}").count(), "{code}: {en}");
            }
        }
        assert_eq!(index_of("xx"), 0);
        assert_eq!(effective("es"), "es");
    }
}
