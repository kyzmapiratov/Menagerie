// App preferences saved on disk (~/.local/share/menagerie/prefs.json).
//
// Sort order, grouping, startup options, "install new archives automatically"
// and so on used to live in the webview's localStorage, which is tied to one
// origin and to the webview's own cache: `tauri dev` and a release build do not
// share it, and clearing the webview data wipes it. A plain file next to the
// rest of the app's data survives all of that.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn file() -> PathBuf {
    crate::library::data_dir().join("prefs.json")
}

fn read(path: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Writes through a temporary file so a crash mid-write cannot leave a
/// half-written prefs.json behind.
fn write(path: &Path, map: &BTreeMap<String, String>) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(map).map_err(|e| format!("could not serialize: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("could not save preferences: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("could not save preferences: {e}"))
}

pub fn load() -> BTreeMap<String, String> {
    read(&file())
}

/// `None` removes the key.
pub fn set(key: &str, value: Option<&str>) -> Result<(), String> {
    set_at(&file(), key, value)
}

fn set_at(path: &Path, key: &str, value: Option<&str>) -> Result<(), String> {
    let mut map = read(path);
    match value {
        Some(v) => map.insert(key.to_string(), v.to_string()),
        None => map.remove(key),
    };
    write(path, &map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_survive_a_reload_and_can_be_removed() {
        let dir = std::env::temp_dir().join(format!("menagerie-prefs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("nested/prefs.json");

        assert!(read(&path).is_empty(), "a missing file is just empty");
        set_at(&path, "mine-sort", Some("size")).unwrap();
        set_at(&path, "cacho-auto", Some("0")).unwrap();
        set_at(&path, "startup", Some("{\"overlay\":true}")).unwrap();

        let back = read(&path); // a fresh read, as after a restart
        assert_eq!(back.get("mine-sort").map(String::as_str), Some("size"));
        assert_eq!(back.get("startup").map(String::as_str), Some("{\"overlay\":true}"));

        set_at(&path, "cacho-auto", None).unwrap();
        assert!(!read(&path).contains_key("cacho-auto"));
        assert!(!path.with_extension("json.tmp").exists(), "no temp file left behind");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_corrupt_file_reads_as_empty_instead_of_failing() {
        let dir = std::env::temp_dir().join(format!("menagerie-prefs-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("prefs.json");
        std::fs::write(&path, "{ not json").unwrap();
        assert!(read(&path).is_empty());
        set_at(&path, "a", Some("1")).unwrap();
        assert_eq!(read(&path).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
