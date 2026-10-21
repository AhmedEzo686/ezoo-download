//! Arabic / English for the texts made in Rust (statuses, messages).
//! The window texts themselves are translated by Slint (lang/*.po).

use std::sync::atomic::{AtomicBool, Ordering};

static ARABIC: AtomicBool = AtomicBool::new(false);

/// "ar", "en", or "" = the language of Windows.
pub fn effective(setting: &str) -> &'static str {
    match setting {
        "ar" => "ar",
        "en" => "en",
        _ => {
            let sys = sys_locale::get_locale().unwrap_or_default().to_ascii_lowercase();
            if sys.starts_with("ar") {
                "ar"
            } else {
                "en"
            }
        }
    }
}

/// Switch every window and every Rust text to this language.
pub fn apply(lang: &str) {
    ARABIC.store(lang == "ar", Ordering::Relaxed);
    let _ = slint::select_bundled_translation(if lang == "ar" { "ar" } else { "en" });
}

pub fn is_arabic() -> bool {
    ARABIC.load(Ordering::Relaxed)
}

/// The text in the current language.
pub fn t(en: &str) -> String {
    if is_arabic() {
        if let Some(ar) = lookup(en) {
            return ar.to_string();
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
        "Remove these {} downloads from the list?\n\nYes: also delete the finished files from disk\nNo: keep the files\n(Unfinished parts are always deleted.)",
        "حذف هذه التحميلات ({}) من القائمة؟\n\nنعم: احذف الملفات المكتملة من القرص أيضاً\nلا: احتفظ بالملفات\n(الأجزاء غير المكتملة تُحذف دائماً.)",
    ),
    (
        "Remove “{}” from the list?\n\nYes: also delete the file from disk\nNo: keep the file",
        "حذف “{}” من القائمة؟\n\nنعم: احذف الملف من القرص أيضاً\nلا: احتفظ بالملف",
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
        "Paste the playlist address (https://www.youtube.com/playlist?list=…)",
        "الصق رابط قائمة التشغيل (https://www.youtube.com/playlist?list=…)",
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
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fills_places() {
        assert_eq!(tf("Tip {} of {}", &[&1, &14]), "Tip 1 of 14");
        ARABIC.store(true, Ordering::Relaxed);
        assert_eq!(tf("Tip {} of {}", &[&1, &14]), "نصيحة 1 من 14");
        assert_eq!(t("not in the list"), "not in the list");
        ARABIC.store(false, Ordering::Relaxed);
    }
}
