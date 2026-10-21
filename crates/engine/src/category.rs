//! File categories, used for the sidebar and for per-category save folders.

use crate::filename::extension;

pub const CATEGORIES: [&str; 7] = ["Compressed", "Documents", "Music", "Programs", "Video", "Images", "General"];

pub fn category_for(file_name: &str) -> &'static str {
    match extension(file_name).as_str() {
        "zip" | "rar" | "7z" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "iso" | "img" | "cab" => {
            "Compressed"
        }
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "rtf" | "odt" | "ods"
        | "epub" | "csv" | "md" => "Documents",
        "mp3" | "wav" | "flac" | "aac" | "ogg" | "m4a" | "wma" | "opus" => "Music",
        "exe" | "msi" | "msix" | "appx" | "apk" | "dmg" | "deb" | "rpm" | "bat" | "jar" => "Programs",
        "mp4" | "mkv" | "avi" | "mov" | "wmv" | "flv" | "webm" | "m4v" | "ts" | "3gp" | "mpg"
        | "mpeg" => "Video",
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "svg" | "tif" | "tiff" | "heic" | "ico" | "psd" => {
            "Images"
        }
        _ => "General",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn categories() {
        assert_eq!(category_for("x.ZIP"), "Compressed");
        assert_eq!(category_for("movie.mkv"), "Video");
        assert_eq!(category_for("setup.exe"), "Programs");
        assert_eq!(category_for("noext"), "General");
    }
}
