// Finds freshly downloaded archives in the Downloads folder.
//
// For sources the app does not download from itself (cachomon.com), the
// user downloads the archive in a browser and we pick it up here: fresh .zip
// files are listed, the ones that look like Shimeji-EE are flagged, and they
// go into the regular local-archive install flow.

use serde::Serialize;
use std::path::PathBuf;

#[derive(Serialize, Clone, Debug)]
pub struct DownloadFile {
    pub path: String,
    pub name: String,
    pub size_mb: f32,
    /// How many seconds ago the file was modified.
    pub age_secs: u64,
    /// Contains actions.xml / behaviors.xml or an img/ folder.
    pub looks_like_shimeji: bool,
    /// Characters inside the archive (`img/<Name>/` folders).
    pub characters: Vec<String>,
    /// Every character in the archive is already installed, so the archive is hidden.
    pub installed: bool,
}

/// Name for comparison: ignores case, spaces and punctuation.
pub fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

/// Downloads folder: XDG_DOWNLOAD_DIR from ~/.config/user-dirs.dirs, otherwise ~/Downloads.
pub fn downloads_dir() -> PathBuf {
    // A folder the person chose themselves wins: browsers can be told to save anywhere,
    // and the app has to watch the place the archives actually land.
    if let Some(chosen) = crate::prefs::load().get("downloads-dir") {
        let path = PathBuf::from(chosen);
        if path.is_dir() {
            return path;
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let cfg = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(&home).join(".config"));

    if let Ok(text) = std::fs::read_to_string(cfg.join("user-dirs.dirs")) {
        for line in text.lines() {
            if let Some(v) = line.trim().strip_prefix("XDG_DOWNLOAD_DIR=") {
                let v = v.trim().trim_matches('"').replace("$HOME", &home);
                if !v.is_empty() {
                    return PathBuf::from(v);
                }
            }
        }
    }
    PathBuf::from(home).join("Downloads")
}

/// None means it does not look like Shimeji; Some(names) means it does (the names may
/// be empty when the conf files sit in the root without an img/ folder).
fn inspect(path: &std::path::Path) -> Option<Vec<String>> {
    let f = std::fs::File::open(path).ok()?;
    let zip = zip::ZipArchive::new(f).ok()?;

    let mut names: Vec<String> = Vec::new();
    let mut shimeji = false;
    for n in zip.file_names() {
        let parts: Vec<&str> = n.split('/').collect();
        // Exactly these file names (default-actions.xml is not a Shimeji conf).
        let base = parts.last().map(|b| b.to_lowercase()).unwrap_or_default();
        if base == "actions.xml" || base == "behaviors.xml" {
            shimeji = true;
        }
        if let Some(i) = parts.iter().position(|p| *p == "img") {
            shimeji = true;
            if let Some(name) = parts.get(i + 1).filter(|_| parts.len() > i + 2) {
                if !names.iter().any(|x| x == name) {
                    names.push(name.to_string());
                }
            }
        }
    }
    shimeji.then_some(names)
}

/// .zip files modified in the last week, newest first; Shimeji-looking ones come first.
pub fn recent_archives() -> Vec<DownloadFile> {
    let now = std::time::SystemTime::now();
    let have: std::collections::HashSet<String> = crate::shimejictl::installed_names_on_disk()
        .iter()
        .map(|n| norm(n))
        .collect();
    let Ok(entries) = std::fs::read_dir(downloads_dir()) else {
        return Vec::new();
    };

    let mut out: Vec<DownloadFile> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.extension()?.to_str()?.to_lowercase() != "zip" {
                return None;
            }
            let meta = e.metadata().ok()?;
            let age = now.duration_since(meta.modified().ok()?).ok()?.as_secs();
            if age > 7 * 24 * 3600 {
                return None;
            }
            let found = inspect(&path);
            let characters = found.clone().unwrap_or_default();
            Some(DownloadFile {
                name: path.file_name()?.to_string_lossy().to_string(),
                size_mb: (meta.len() as f32 / 1_048_576.0 * 10.0).round() / 10.0,
                age_secs: age,
                looks_like_shimeji: found.is_some(),
                installed: !characters.is_empty() && characters.iter().all(|c| have.contains(&norm(c))),
                characters,
                path: path.display().to_string(),
            })
        })
        .collect();

    out.sort_by_key(|f| (!f.looks_like_shimeji, f.age_secs));
    out.truncate(12);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn inspect_finds_character_folders() {
        let path = std::env::temp_dir().join(format!("menagerie-dl-{}.zip", std::process::id()));
        {
            let mut z = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
            let o = zip::write::SimpleFileOptions::default();
            for n in ["img/Zooble/shime1.png", "img/Zooble/conf/actions.xml", "img/.Needle/shime1.png", "readme.txt"] {
                z.start_file(n, o).unwrap();
                z.write_all(b"x").unwrap();
            }
            z.finish().unwrap();
        }
        assert_eq!(inspect(&path), Some(vec!["Zooble".to_string(), ".Needle".to_string()]));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn norm_ignores_case_dots_and_spaces() {
        assert_eq!(norm(".Hornet_Needle"), norm("hornet needle"));
        assert_eq!(norm("Mosscreep (Orange)"), "mosscreeporange");
    }

    /// Reads the real Downloads folder (read-only): `cargo test live_downloads -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_downloads() {
        for f in recent_archives() {
            println!("{:45} shimeji={} installed={} chars={:?}", f.name, f.looks_like_shimeji, f.installed, f.characters);
        }
    }
}
