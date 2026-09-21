// The app's local library.
//
// shimejictl only knows prototype names, not where a character came from or
// what it looks like. So we cache the sprite, franchise and artist ourselves at
// install time. Favorites and presets ("summon this set") live here as well.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Entry {
    /// local path of the cached sprite
    pub sprite_path: String,
    pub slug: String,
    pub pack_title: String,
    pub artist: String,
    #[serde(default)]
    pub favorite: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Preset {
    pub name: String,
    /// prototype names and how many copies of each to summon
    pub members: Vec<(String, usize)>,
}

#[derive(Serialize, Deserialize, Default)]
struct Store {
    entries: HashMap<String, Entry>,
    #[serde(default)]
    presets: Vec<Preset>,
    /// Characters to launch at login in niri
    #[serde(default)]
    autostart: Vec<(String, usize)>,
}

pub fn data_dir() -> PathBuf {
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(home).join(".local/share")
        });
    base.join("menagerie")
}

fn store_path() -> PathBuf {
    data_dir().join("library.json")
}

pub fn sprites_dir() -> PathBuf {
    data_dir().join("sprites")
}

fn load() -> Store {
    let path = store_path();
    for attempt in 0..3 {
        // No file yet: an empty library, as on a first run.
        let Ok(text) = std::fs::read_to_string(&path) else { return Store::default() };
        if let Ok(store) = serde_json::from_str(&text) {
            return store;
        }
        // Unreadable. Writes are atomic now, so this is a damaged file rather than
        // one caught half-written; ask again once or twice anyway, then keep a copy
        // for a look: the next save would otherwise silently replace it.
        if attempt < 2 {
            std::thread::sleep(std::time::Duration::from_millis(60));
        } else {
            let _ = std::fs::copy(&path, path.with_extension("json.unreadable"));
        }
    }
    Store::default()
}

/// Written to a temporary file and renamed into place: a reader (the collection
/// is read while a background job saves) never sees a half-written library, which
/// used to read as "empty" and cost every character its picture and group.
fn save(store: &Store) -> Result<(), String> {
    std::fs::create_dir_all(data_dir())
        .map_err(|e| format!("could not create the data folder: {e}"))?;
    let json = serde_json::to_string_pretty(store)
        .map_err(|e| format!("could not serialize: {e}"))?;
    let path = store_path();
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| format!("could not save: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("could not save: {e}"))
}

/// Groups used to be editable by hand, which clashed with the automatic fill:
/// `match_sprites` never overwrites a title that is already set, so a name typed
/// in once stuck forever. The manual editing is gone, so any title left over
/// from it has to go too — otherwise those characters keep a group the UI can no
/// longer change. Clearing the titles once makes the next fill derive them from
/// the catalog again. Runs at startup, does nothing on later launches.
pub fn reset_hand_set_groups_once() {
    const KEY: &str = "groups-reset-v1";
    if crate::prefs::load().contains_key(KEY) {
        return;
    }
    let mut store = load();
    for entry in store.entries.values_mut() {
        entry.pack_title.clear();
    }
    if save(&store).is_ok() {
        let _ = crate::prefs::set(KEY, Some("done"));
    }
}

pub fn all_entries() -> HashMap<String, Entry> {
    load().entries
}

pub fn record(name: &str, entry: Entry) -> Result<(), String> {
    let mut store = load();
    // Keep the "favorite" flag if the entry already existed.
    let favorite = store.entries.get(name).map(|e| e.favorite).unwrap_or(false);
    store.entries.insert(
        name.to_string(),
        Entry {
            favorite,
            ..entry
        },
    );
    save(&store)
}

pub fn forget(name: &str) -> Result<(), String> {
    let mut store = load();
    // After an install the metadata sits under two names: "BMO" and
    // "Shimeji.BMO". Remove both, otherwise stale entries survive a delete
    // followed by a reinstall.
    for key in [name.to_string(), format!("Shimeji.{name}")] {
        if let Some(e) = store.entries.remove(&key) {
            if !e.sprite_path.is_empty() {
                let _ = std::fs::remove_file(&e.sprite_path);
            }
        }
    }
    save(&store)
}

pub fn toggle_favorite(name: &str) -> Result<bool, String> {
    let mut store = load();
    let entry = store.entries.entry(name.to_string()).or_default();
    entry.favorite = !entry.favorite;
    let now = entry.favorite;
    save(&store)?;
    Ok(now)
}

// ---------------------------------------------------------------------------
// Presets
// ---------------------------------------------------------------------------

pub fn presets() -> Vec<Preset> {
    load().presets
}

pub fn save_preset(preset: Preset) -> Result<(), String> {
    let mut store = load();
    store.presets.retain(|p| p.name != preset.name);
    store.presets.push(preset);
    save(&store)
}

pub fn delete_preset(name: &str) -> Result<(), String> {
    let mut store = load();
    store.presets.retain(|p| p.name != name);
    save(&store)
}

// ---------------------------------------------------------------------------
// Sprite cache
// ---------------------------------------------------------------------------

/// Downloads a character's sprite and stores it locally. Returns the path or an empty string.
pub async fn cache_sprite(name: &str, sprite_url: &str) -> String {
    if sprite_url.is_empty() || std::fs::create_dir_all(sprites_dir()).is_err() {
        return String::new();
    }

    let safe: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let dest = sprites_dir().join(format!("{safe}.png"));

    let Ok(resp) = reqwest::get(sprite_url).await else {
        return String::new();
    };
    if !resp.status().is_success() {
        return String::new();
    }
    let Ok(bytes) = resp.bytes().await else {
        return String::new();
    };
    if std::fs::write(&dest, &bytes).is_err() {
        return String::new();
    }

    dest.display().to_string()
}

// ---------------------------------------------------------------------------
// Autostart
// ---------------------------------------------------------------------------

pub fn autostart() -> Vec<(String, usize)> {
    load().autostart
}

pub fn set_autostart(list: Vec<(String, usize)>) -> Result<(), String> {
    let mut store = load();
    store.autostart = list;
    save(&store)
}

// ---------------------------------------------------------------------------
// Catalog index cache (for searching the whole catalog)
// ---------------------------------------------------------------------------

pub fn index_path() -> PathBuf {
    data_dir().join("catalog-index.json")
}

pub fn read_index<T: serde::de::DeserializeOwned>() -> Option<T> {
    std::fs::read_to_string(index_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

/// How long ago the saved catalog index was written (None: there is none).
pub fn index_age() -> Option<std::time::Duration> {
    std::fs::metadata(index_path()).ok()?.modified().ok()?.elapsed().ok()
}

pub fn write_index<T: Serialize>(value: &T) -> Result<(), String> {
    std::fs::create_dir_all(data_dir())
        .map_err(|e| format!("could not create the data folder: {e}"))?;
    let json = serde_json::to_string(value)
        .map_err(|e| format!("could not serialize the index: {e}"))?;
    std::fs::write(index_path(), json).map_err(|e| format!("could not save the index: {e}"))
}
