use eframe::egui::{self, RichText};
use egui_phosphor::regular;
use std::path::Path;

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum FileTy {
    Application,
    Archive,
    Audio,
    Book,
    Document,
    Font,
    Image,
    Text,
    Torrent,
    #[default]
    Unknown,
    Video,
}

impl FileTy {
    pub fn from_ext(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "jpg" | "jpeg" | "png" | "gif" | "webp" | "cr2" | "tif" | "bmp" | "heif" | "avif"
            | "jxr" | "psd" | "ico" | "ora" => FileTy::Image,

            "mp4" | "m4v" | "mkv" | "webm" | "mov" | "avi" | "wmv" | "mpg" | "flv" => FileTy::Video,

            "mid" | "mp3" | "m4a" | "ogg" | "flac" | "wav" | "amr" | "aac" | "aiff" | "dsf"
            | "ape" => FileTy::Audio,

            "zip" | "tar" | "rar" | "gz" | "bz2" | "7z" | "xz" | "pkg" | "swf" | "rtf" | "eot"
            | "ps" | "sqlite" | "nes" | "crx" | "cab" | "deb" | "ar" | "Z" | "lz" | "rpm"
            | "dcm" | "zst" | "msi" | "cpio" => FileTy::Archive,

            "djvu" | "epub" | "mobi" => FileTy::Book,

            "txt" => FileTy::Text,

            "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "odt" | "ods" | "odp" | "pdf" => {
                FileTy::Document
            }

            "woff" | "woff2" | "ttf" | "otf" => FileTy::Font,

            "wasm" | "exe" | "dll" | "elf" | "bc" | "mach" | "class" | "dex" | "dey" | "der"
            | "obj" => FileTy::Application,

            "torrent" => FileTy::Torrent,

            _ => FileTy::Unknown,
        }
    }

    pub fn ico(&self) -> &'static str {
        match self {
            FileTy::Archive => regular::FILE_ARCHIVE,
            FileTy::Image => regular::FILE_IMAGE,
            FileTy::Audio => regular::FILE_AUDIO,
            FileTy::Video => regular::FILE_VIDEO,
            FileTy::Book | FileTy::Text | FileTy::Document => regular::FILE_TEXT,
            FileTy::Torrent => regular::FILE_CLOUD,
            _ => regular::FILE,
        }
    }
}

pub fn file_ico_str(path: &Path) -> &'static str {
    path.extension()
        .and_then(|e| e.to_str())
        .map(FileTy::from_ext)
        .unwrap_or_default()
        .ico()
}

pub fn file_ico(path: &Path, ui: &egui::Ui) -> RichText {
    RichText::new(file_ico_str(path)).size(ui.text_style_height(&egui::TextStyle::Body) * 4.0)
}
