//! File-type categories by extension, with a system-path override.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Category {
    Video = 0,
    Images = 1,
    Audio = 2,
    Archives = 3,
    Installers = 4,
    Documents = 5,
    Code = 6,
    DiskImages = 7,
    System = 8,
    Other = 9,
}

pub const ALL: [Category; 10] = [
    Category::Video,
    Category::Images,
    Category::Audio,
    Category::Archives,
    Category::Installers,
    Category::Documents,
    Category::Code,
    Category::DiskImages,
    Category::System,
    Category::Other,
];

impl Category {
    pub fn key(self) -> &'static str {
        match self {
            Category::Video => "video",
            Category::Images => "images",
            Category::Audio => "audio",
            Category::Archives => "archives",
            Category::Installers => "installers",
            Category::Documents => "documents",
            Category::Code => "code",
            Category::DiskImages => "disk-images",
            Category::System => "system",
            Category::Other => "other",
        }
    }

    pub fn from_code(code: u8) -> Category {
        ALL.get(code as usize).copied().unwrap_or(Category::Other)
    }
}

const VIDEO: &[&str] =
    &["mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "mpg", "mpeg", "m2ts", "mts", "vob", "3gp", "ogv"];
const IMAGES: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "bmp", "tif", "tiff", "webp", "heic", "heif", "raw", "cr2", "cr3", "nef", "arw",
    "dng", "orf", "rw2", "psd", "svg", "ico", "avif", "jxl",
];
const AUDIO: &[&str] =
    &["mp3", "flac", "wav", "aac", "ogg", "m4a", "wma", "opus", "aiff", "aif", "alac", "mid", "midi", "ape"];
const ARCHIVES: &[&str] = &["zip", "rar", "7z", "tar", "gz", "tgz", "bz2", "xz", "zst", "cab", "lz", "lzma", "lz4"];
const INSTALLERS: &[&str] =
    &["exe", "msi", "msix", "msixbundle", "appx", "appxbundle", "iso", "img", "dmg", "msp", "msu"];
const DOCUMENTS: &[&str] = &[
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "odp", "txt", "rtf", "md", "csv", "epub", "mobi",
    "one", "pst", "ost", "eml", "msg",
];
const CODE: &[&str] = &[
    "rs", "py", "js", "mjs", "cjs", "ts", "jsx", "tsx", "java", "c", "cc", "cpp", "h", "hpp", "cs", "go", "rb", "php",
    "swift", "kt", "json", "yaml", "yml", "toml", "xml", "html", "css", "scss", "sql", "sh", "ps1", "bat", "ipynb",
    "pyc", "class", "o", "obj", "lib", "pdb", "jar", "whl", "lock", "map", "rlib", "rmeta", "ilk", "idb", "pch",
    "nupkg", "node",
];
const DISK_IMAGES: &[&str] =
    &["vhd", "vhdx", "avhdx", "vmdk", "vdi", "qcow2", "vhdpart", "wim", "esd", "hdd", "vmem", "vmsn"];
const SYSTEM: &[&str] = &["sys", "dll", "drv", "mui", "cpl", "ocx", "cat", "efi"];

/// Lowercase extension after the last dot (empty when none, or for dotfiles).
pub fn extension(name: &str) -> String {
    match name.rfind('.') {
        Some(i) if i > 0 && i + 1 < name.len() => name[i + 1..].to_ascii_lowercase(),
        _ => String::new(),
    }
}

pub fn by_extension(ext: &str) -> Category {
    // `.ts` counts as TypeScript (code); video streams use .m2ts/.mts.
    let lists: [(&[&str], Category); 9] = [
        (VIDEO, Category::Video),
        (IMAGES, Category::Images),
        (AUDIO, Category::Audio),
        (ARCHIVES, Category::Archives),
        (INSTALLERS, Category::Installers),
        (DOCUMENTS, Category::Documents),
        (DISK_IMAGES, Category::DiskImages),
        (SYSTEM, Category::System),
        (CODE, Category::Code),
    ];
    lists.iter().find(|(l, _)| l.contains(&ext)).map(|(_, c)| *c).unwrap_or(Category::Other)
}

/// Paths treated as system regardless of extension.
pub struct SystemPaths {
    windows_dir: String,
}

const ROOT_SYSTEM_FILES: &[&str] = &["pagefile.sys", "hiberfil.sys", "swapfile.sys", "dumpstack.log.tmp"];

impl SystemPaths {
    pub fn new(windows_dir: &str) -> Self {
        Self { windows_dir: windows_dir.trim_end_matches('\\').to_lowercase() }
    }

    pub fn from_env() -> Self {
        Self::new(&std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()))
    }

    /// `dir_path` is the containing directory of a file named `name`.
    pub fn is_system(&self, dir_path: &str, name: &str) -> bool {
        let dir = dir_path.to_lowercase();
        let in_windows = dir == self.windows_dir || dir.starts_with(&format!("{}\\", self.windows_dir));
        let in_svi = dir.contains(r"\system volume information");
        let root_sys =
            dir.len() == 3 && dir.ends_with(":\\") && ROOT_SYSTEM_FILES.contains(&name.to_lowercase().as_str());
        in_windows || in_svi || root_sys
    }
}

pub fn categorize(system: &SystemPaths, dir_path: &str, name: &str, ext: &str) -> Category {
    if system.is_system(dir_path, name) {
        Category::System
    } else {
        by_extension(ext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_examples() {
        let sys = SystemPaths::new(r"C:\Windows");
        let cases = [
            ("a.MKV", Category::Video),
            ("b.vhdx", Category::DiskImages),
            ("c.msi", Category::Installers),
            ("d.py", Category::Code),
            ("e.xyz123", Category::Other),
        ];
        for (name, cat) in cases {
            assert_eq!(categorize(&sys, r"D:\stuff", name, &extension(name)), cat, "{name}");
        }
        assert_eq!(extension("a.MKV"), "mkv");
        assert_eq!(extension(".gitignore"), "");
        assert_eq!(extension("noext"), "");
        assert_eq!(extension("trailing."), "");
    }

    #[test]
    fn system_override() {
        let sys = SystemPaths::new(r"C:\Windows");
        assert_eq!(categorize(&sys, r"C:\Windows\System32", "photo.jpg", "jpg"), Category::System);
        assert_eq!(categorize(&sys, r"C:\", "pagefile.sys", "sys"), Category::System);
        assert_eq!(categorize(&sys, r"C:\", "hiberfil.sys", "sys"), Category::System);
        assert_eq!(categorize(&sys, r"D:\System Volume Information", "x.dat", "dat"), Category::System);
        assert_eq!(
            categorize(&sys, r"C:\WindowsApps", "a.jpg", "jpg"),
            Category::Images,
            "prefix must match a path component"
        );
    }

    #[test]
    fn codes_round_trip() {
        for c in ALL {
            assert_eq!(Category::from_code(c as u8), c);
        }
    }
}
