#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod cachomon;
mod catalog;
mod downloads;
mod library;
mod desktop;
mod hyprland;
mod niri;
mod pack;
mod prefs;
mod relocate;
mod shimejictl;
mod system;
mod tray;
mod wayland;

use catalog::{Character, Pack};
use library::{Entry, Preset};
use serde::Serialize;
use std::sync::Mutex;
use tauri::{Emitter, Manager, State};

/// An archive the person picked, waiting for them to choose who to take from it.
///
/// The catalog does not need this: the app builds those packages itself, each holds
/// exactly one character, and the choice is made for it.
enum Pending {
    /// A Shimeji-EE archive: `shimejictl convert` is running and waiting for the choice.
    Convert(shimejictl::PendingConvert),
    /// Characters already in wl_shimeji's format (this app's own export, say): nothing
    /// to convert, they only have to be imported.
    Ready(shimejictl::ReadyPrototypes),
}

#[derive(Default)]
struct AppState {
    pending: Mutex<Option<Pending>>,
    /// How many times each character was summoned this session.
    tally: Mutex<std::collections::HashMap<String, usize>>,
    /// Set by `cancel_install`, read between the steps of a catalog install.
    /// Reset at the start of every install so an old Cancel cannot stop a new one.
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// The same for an export of the collection. Reset when one starts.
    cancel_export: std::sync::atomic::AtomicBool,
}

#[derive(Serialize)]
struct InstalledItem {
    name: String,
    sprite_path: String,
    pack_title: String,
    artist: String,
    favorite: bool,
    /// When it was installed (seconds since the epoch), for the "newest first" order.
    installed_at: u64,
    /// How much disk space it takes (bytes).
    size_bytes: u64,
}

#[derive(Serialize)]
struct InstallOutcome {
    /// One entry per character (not one per alias name).
    installed: Vec<String>,
    /// Already in the collection and not reinstalled.
    skipped: Vec<String>,
    failed: Vec<(String, String)>,
    /// Not installed because the user pressed Cancel.
    cancelled: Vec<String>,
}

// ---------------------------------------------------------------------------
// Catalog
// ---------------------------------------------------------------------------

#[tauri::command]
async fn fetch_packs() -> Result<Vec<Pack>, String> {
    catalog::fetch_packs().await
}

#[tauri::command]
async fn fetch_characters(pack_slug: String) -> Result<Vec<Character>, String> {
    catalog::fetch_characters(&pack_slug).await
}

/// How long a saved catalog index is trusted. It says which characters exist and how many a pack has, and the
/// site gains some every week; a copy that was never renewed (this one was three days old and no button
/// renewed it) made search and the "how much of this pack do I have" counts quietly wrong.
const INDEX_TTL: std::time::Duration = std::time::Duration::from_secs(12 * 3600);

fn index_is_fresh(age: Option<std::time::Duration>) -> bool {
    age.is_some_and(|a| a < INDEX_TTL)
}

/// Index of the whole catalog for global search and the counts on the pack cards.
/// Without `refresh` the saved copy is used while it is fresh; with it (the reload button) the site is asked.
/// A copy that could not be renewed on its own is still better than none (offline); one that was asked for is an error.
#[tauri::command]
async fn catalog_index(refresh: Option<bool>) -> Result<Vec<Character>, String> {
    let explicit = refresh.unwrap_or(false);
    let saved = library::read_index::<Vec<Character>>().filter(|c| !c.is_empty());
    if !explicit && index_is_fresh(library::index_age()) {
        if let Some(saved) = saved {
            return Ok(saved);
        }
    }
    match (catalog::build_index().await, saved) {
        (Ok(all), _) => {
            let _ = library::write_index(&all);
            Ok(all)
        }
        (Err(_), Some(saved)) if !explicit => Ok(saved),
        (Err(e), _) => Err(e),
    }
}

// ---------------------------------------------------------------------------
// Installing
// ---------------------------------------------------------------------------

/// The `install-progress` event for the install indicator in the UI.
///
/// `fraction` is how far this one character has got (0..1); the frontend computes
/// overall progress as (index + fraction) / total.
#[derive(Serialize, Clone)]
struct InstallProgress {
    /// Row key in the indicator: a catalog slug or "local".
    key: String,
    name: String,
    index: usize,
    total: usize,
    /// queued | download | pack | convert | import | sprite | done | skipped | error | cancelled
    stage: &'static str,
    fraction: f32,
    message: String,
}

fn emit_progress(
    app: &tauri::AppHandle,
    key: &str,
    name: &str,
    (index, total): (usize, usize),
    stage: &'static str,
    fraction: f32,
    message: impl Into<String>,
) {
    let _ = app.emit(
        "install-progress",
        InstallProgress {
            key: key.to_string(),
            name: name.to_string(),
            index,
            total,
            stage,
            fraction,
            message: message.into(),
        },
    );
}

/// Installs one character from the catalog.
///
/// Since we build the package ourselves it contains exactly one character, so we
/// answer shimejictl's "which ones to convert" question with "all" automatically,
/// with no separate step in the UI.
async fn install_one(
    app: &tauri::AppHandle,
    cancel: &std::sync::Arc<std::sync::atomic::AtomicBool>,
    character: &Character,
    pos: (usize, usize),
) -> Result<Vec<String>, String> {
    use std::sync::atomic::Ordering;

    let key = character.slug.clone();
    let label = character.name.clone();
    let step = |stage: &'static str, fraction: f32, msg: &str| {
        emit_progress(app, &key, &label, pos, stage, fraction, msg)
    };

    let work_dir = system::scratch_dir();
    let out_dir = work_dir.join(format!("converted-{}", character.slug));
    let _ = std::fs::remove_dir_all(&out_dir);

    // Downloading frames is 0..60% of the character's progress bar.
    step("download", 0.0, "Downloading frames");
    let flag = cancel.clone();
    let archive = pack::build_package(
        &character.slug,
        &character.name,
        &work_dir,
        |done| step("download", 0.6 * done as f32 / 46.0, "Downloading frames"),
        move || flag.load(Ordering::SeqCst),
    )
    .await?;

    // Past this point a step is only worth interrupting before it changes the
    // collection: converting is a few seconds of local work, importing is what
    // makes the character appear. So Cancel is honoured up to the import.
    if cancel.load(Ordering::SeqCst) {
        let _ = std::fs::remove_file(&archive);
        return Err(pack::CANCELLED.to_string());
    }

    step("convert", 0.66, "Converting for wl_shimeji");
    let app_for_thread = app.clone();
    let (k, l) = (key.clone(), label.clone());
    let out_for_thread = out_dir.clone();
    let flag = cancel.clone();
    let installed = tauri::async_runtime::spawn_blocking(move || -> Result<Vec<String>, String> {
        let (convert, _prototypes) = shimejictl::start_convert(&archive, &out_for_thread)?;
        let files = shimejictl::finish_convert(convert, &[])?;
        if flag.load(Ordering::SeqCst) {
            let _ = std::fs::remove_dir_all(&out_for_thread);
            let _ = std::fs::remove_file(&archive);
            return Err(pack::CANCELLED.to_string());
        }
        emit_progress(&app_for_thread, &k, &l, pos, "import", 0.85, "Adding to your collection");
        shimejictl::import_prototypes(&files, |_, _| {})?;
        Ok(files
            .iter()
            .filter_map(|p| p.file_stem().and_then(|s| s.to_str()).map(String::from))
            .collect())
    })
    .await
    .map_err(|e| format!("internal thread error: {e}"))??;

    step("sprite", 0.94, "Saving the picture");

    // Remember the metadata under every known name: the chosen one and the one
    // shimejictl gave (which may be "Shimeji.Eevee").
    let mut names = vec![character.name.clone()];
    for n in &installed {
        if !names.contains(n) {
            names.push(n.clone());
        }
    }

    let artist = catalog::fetch_artist(&character.slug).await.unwrap_or_default();

    for name in &names {
        let mut sprite_path = library::cache_sprite(name, &character.sprite).await;
        if sprite_path.is_empty() {
            // The catalog did not provide a sprite, so take a frame from the prototype we just
            // installed; otherwise the card in the Collection would stay empty.
            let n = name.clone();
            sprite_path = blocking(move || Ok(local_sprite(&n))).await?;
        }
        let _ = library::record(
            name,
            Entry {
                sprite_path,
                slug: character.slug.clone(),
                pack_title: character.pack_title.clone(),
                artist: artist.clone(),
                favorite: false,
            },
        );
    }

    step("done", 1.0, "Done");
    Ok(names)
}

/// Installs one or several characters in one call.
#[tauri::command]
async fn install_characters(
    app: tauri::AppHandle,
    characters: Vec<Character>,
    overwrite: Option<bool>,
    state: State<'_, AppState>,
) -> Result<InstallOutcome, String> {
    use std::sync::atomic::Ordering;

    let cancel = state.cancel.clone();
    cancel.store(false, Ordering::SeqCst);

    let mut installed = Vec::new();
    let mut skipped = Vec::new();
    let mut failed = Vec::new();
    let mut cancelled = Vec::new();
    let total = characters.len();

    // Leave already installed characters alone unless replacing was explicitly requested.
    let have: std::collections::HashSet<String> = if overwrite.unwrap_or(false) {
        Default::default()
    } else {
        shimejictl::installed_names_on_disk().iter().map(|n| norm(n)).collect()
    };

    // Show the whole queue at once so the indicator knows how long to wait.
    for (i, c) in characters.iter().enumerate() {
        emit_progress(&app, &c.slug, &c.name, (i, total), "queued", 0.0, "Queued");
    }

    for (i, c) in characters.iter().enumerate() {
        // Cancel pressed: everything not finished yet is dropped, and the rows
        // for it say so instead of staying "Queued" forever.
        if cancel.load(Ordering::SeqCst) {
            for (j, rest) in characters.iter().enumerate().skip(i) {
                if have.contains(&norm(&rest.name)) {
                    emit_progress(&app, &rest.slug, &rest.name, (j, total), "skipped", 1.0, "Already installed");
                    skipped.push(rest.name.clone());
                } else {
                    emit_progress(&app, &rest.slug, &rest.name, (j, total), "cancelled", 1.0, "Cancelled");
                    cancelled.push(rest.name.clone());
                }
            }
            break;
        }

        if have.contains(&norm(&c.name)) {
            emit_progress(&app, &c.slug, &c.name, (i, total), "skipped", 1.0, "Already installed");
            skipped.push(c.name.clone());
            continue;
        }
        match install_one(&app, &cancel, c, (i, total)).await {
            Ok(_) => installed.push(c.name.clone()),
            Err(e) if e == pack::CANCELLED => {
                emit_progress(&app, &c.slug, &c.name, (i, total), "cancelled", 1.0, "Cancelled");
                cancelled.push(c.name.clone());
            }
            Err(e) => {
                emit_progress(&app, &c.slug, &c.name, (i, total), "error", 1.0, e.clone());
                failed.push((c.name.clone(), e));
            }
        }
    }

    Ok(InstallOutcome { installed, skipped, failed, cancelled })
}

/// Stops a running catalog install: the current character at its next
/// checkpoint, and everyone still waiting in the queue.
#[tauri::command]
fn cancel_install(state: State<'_, AppState>) {
    // One Cancel button in the progress panel serves both: an install and an export never run for the same panel.
    state.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
    state.cancel_export.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// Local archive: it may contain many characters, so the two-step flow with a
/// choice remains.
#[tauri::command]
fn prepare_local_archive(
    path: String,
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let old = state.pending.lock().unwrap().take();
    if let Some(Pending::Convert(old)) = old {
        shimejictl::cancel_convert(old);
    }

    // An archive that already holds wl_shimeji prototypes (this app's own export, or
    // anything exported by `shimejictl export`) needs no conversion, and the converter
    // refuses it outright — that is the "not a valid Shimeji-EE instance" message.
    let file = std::path::Path::new(&path);
    if let Some(ready) = shimejictl::ready_prototypes(file)? {
        let names: Vec<String> = ready.items.iter().map(|(n, _)| n.clone()).collect();
        if names.is_empty() {
            return Err("There are no characters in that archive.".to_string());
        }
        *state.pending.lock().unwrap() = Some(Pending::Ready(ready));
        return Ok(names);
    }

    let out_dir = system::scratch_dir().join("converted-local");
    let (convert, prototypes) = shimejictl::start_convert(file, &out_dir)?;

    *state.pending.lock().unwrap() = Some(Pending::Convert(convert));
    Ok(prototypes)
}

#[tauri::command]
async fn install_local_selected(
    app: tauri::AppHandle,
    selection: Vec<String>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let pending = {
        let mut guard = state.pending.lock().unwrap();
        guard.take()
    }
    .ok_or("no archive is open")?;

    let label = if selection.is_empty() { "Everything in the archive".to_string() } else { selection.join(", ") };
    let pos = (0, 1);
    emit_progress(&app, "local", &label, pos, "convert", 0.2, "Converting for wl_shimeji");

    let app2 = app.clone();
    let (l2, sel) = (label.clone(), selection.clone());
    let result = blocking(move || {
        // The unpacked files of a ready archive live in a folder that is deleted when `ReadyPrototypes` is dropped. It
        // used to be dropped at the end of the match arm below, before the import ran: every file "does not exist".
        // It has to be held until the import is over.
        let mut hold_until_imported = None;
        let files = match pending {
            Pending::Convert(convert) => shimejictl::finish_convert(convert, &sel)?,
            // Already converted: take the ones that were ticked, or all of them.
            Pending::Ready(ready) => {
                let chosen = ready
                    .items
                    .iter()
                    .filter(|(name, _)| sel.is_empty() || sel.iter().any(|s| s == name))
                    .map(|(_, path)| path.clone())
                    .collect();
                hold_until_imported = Some(ready);
                chosen
            }
        };
        if files.is_empty() {
            return Err("Nothing was chosen from the archive.".to_string());
        }
        emit_progress(&app2, "local", &l2, pos, "import", 0.7, "Adding to your collection");
        let total = files.len();
        let report = shimejictl::import_prototypes(&files, |done, all| {
            let last = (done + 20).min(all);
            emit_progress(&app2, "local", &l2, pos, "import", 0.7 + 0.2 * done as f32 / all.max(1) as f32, format!("Adding to your collection · up to {last} of {all}"));
        })?;

        // Sprite and group for what was just installed, without the network: a frame from
        // the prototype itself, the group from the saved catalogs.
        emit_progress(&app2, "local", &l2, pos, "sprite", 0.92, "Saving the picture");
        // Only what is in the collection now: a name that did not go in must not get a picture and a place of its own.
        let now = shimejictl::installed_names();
        let names: Vec<String> = files
            .iter()
            .map(|p| shimejictl::wlshm_name(p))
            .filter(|n| now.contains(n))
            .collect();
        let xyz: Vec<Character> = library::read_index().unwrap_or_default();
        let cacho = cachomon::cached();
        let ok = names.len();
        for name in names {
            let old = library::all_entries().get(&name).cloned().unwrap_or_default();
            let sprite_path = local_sprite(&name);
            let _ = library::record(
                &name,
                Entry {
                    sprite_path: if sprite_path.is_empty() { old.sprite_path } else { sprite_path },
                    pack_title: if old.pack_title.is_empty() { universe_of(&name, &xyz, &cacho) } else { old.pack_title },
                    slug: old.slug,
                    artist: old.artist,
                    favorite: old.favorite,
                },
            );
        }

        // The truth about it, not "Installed: <what was asked for>": a file the engine turned down is named.
        // `ok` is what is in the collection now, not what the engine claimed: it says "imported" for files it wrote nowhere.
        // Names that start with a dot are helpers a character summons (a spawned egg, a projectile): they are
        // installed with it but are not characters, and "Installed 8 characters" for one download was a puzzle.
        let (helpers, shown): (Vec<String>, Vec<String>) = files.iter().map(|p| shimejictl::wlshm_name(p)).partition(|n| n.starts_with('.'));
        let helper_note = match helpers.len() {
            0 => String::new(),
            1 => " and 1 helper it uses".to_string(),
            n => format!(" and {n} helpers it uses"),
        };
        let mut message = if ok >= total {
            if shown.len() <= 6 { format!("Installed: {}{helper_note}", shown.join(", ")) } else { format!("Installed {} characters{helper_note}", shown.len()) }
        } else if ok == 0 {
            "Nothing was installed".to_string()
        } else {
            format!("Installed {ok} of {total}")
        };
        if report.restarted {
            message.push_str(&format!(
                "\nThe overlay was restarted so it forgets characters you removed{}",
                if report.restored > 0 { format!("; {} on screen were put back where they start", report.restored) } else { String::new() }
            ));
        }
        for problem in report.problems.iter().take(5) {
            message.push_str(&format!("\n{problem}"));
        }
        if report.problems.len() > 5 {
            message.push_str(&format!("\nand {} more", report.problems.len() - 5));
        }
        drop(hold_until_imported);
        Ok(message)
    })
    .await;

    match result {
        Ok(message) => {
            emit_progress(&app, "local", &label, pos, "done", 1.0, message.lines().next().unwrap_or("Done").to_string());
            Ok(message)
        }
        Err(e) => {
            emit_progress(&app, "local", &label, pos, "error", 1.0, e.clone());
            Err(e)
        }
    }
}

#[tauri::command]
fn cancel_local(state: State<'_, AppState>) {
    let pending = {
        let mut guard = state.pending.lock().unwrap();
        guard.take()
    };
    if let Some(Pending::Convert(p)) = pending {
        shimejictl::cancel_convert(p);
    }
}

// ---------------------------------------------------------------------------
// Collection
// ---------------------------------------------------------------------------

#[tauri::command]
async fn list_installed() -> Result<Vec<InstalledItem>, String> {
    blocking(list_installed_inner).await
}

fn list_installed_inner() -> Result<Vec<InstalledItem>, String> {
    // The folders are the truth. The running overlay is not: it keeps a removed character in memory until it
    // restarts (there is no delete command), so asking it "when the folders are empty" showed every character
    // you had just deleted, as a grey letter, and looked like they came back by themselves.
    let names = shimejictl::installed_names();
    let meta = library::all_entries();
    let times = shimejictl::installed_times();
    let sizes = shimejictl::installed_sizes();

    Ok(names
        .into_iter()
        .map(|name| {
            let entry = meta.get(&name).cloned().or_else(|| {
                meta.iter()
                    .find(|(k, _)| name.ends_with(k.as_str()) || k.ends_with(&name))
                    .map(|(_, v)| v.clone())
            });
            let e = entry.unwrap_or_default();
            InstalledItem {
                installed_at: times.get(&name).copied().unwrap_or(0),
                size_bytes: sizes.get(&name).copied().unwrap_or(0),
                name,
                sprite_path: e.sprite_path,
                pack_title: e.pack_title,
                artist: e.artist,
                favorite: e.favorite,
            }
        })
        .collect())
}

/// A frame from the installed prototype itself, saved as a PNG in the sprite cache.
/// An empty string if it could not be extracted.
fn local_sprite(name: &str) -> String {
    let safe: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let dest = library::sprites_dir().join(format!("{safe}.png"));
    if std::fs::create_dir_all(library::sprites_dir()).is_err() {
        return String::new();
    }
    match pack::extract_local_sprite(name, &dest) {
        Ok(()) => dest.display().to_string(),
        Err(_) => String::new(),
    }
}

/// Every shimejictl call is a separate process. Several in a row on the main
/// thread froze the window, so all such commands run on a blocking thread.
async fn blocking<T, F>(f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| format!("internal thread error: {e}"))?
}

#[tauri::command]
async fn summon_mascot(
    name: String,
    count: Option<usize>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let n = count.unwrap_or(1);
    summon_batch(vec![(name, n)], state).await.map(|_| ())
}

/// Summons several characters at once, several copies of each if asked, over a
/// single connection to the overlay (see `shimejictl::summon_batch`). It starts the
/// overlay if it is not running.
#[tauri::command]
async fn summon_batch(
    items: Vec<(String, usize)>,
    state: State<'_, AppState>,
) -> Result<shimejictl::SpawnResult, String> {
    let asked = items.clone();
    let result = blocking(move || shimejictl::summon_batch(&items)).await?;
    let mut tally = state.tally.lock().unwrap();
    for (name, count) in asked {
        if !result.missing.contains(&name) && !name.starts_with('.') {
            *tally.entry(name).or_insert(0) += count.max(1);
        }
    }
    Ok(result)
}

#[tauri::command]
async fn dismiss_all(state: State<'_, AppState>) -> Result<String, String> {
    let msg = blocking(shimejictl::dismiss_all).await?;
    state.tally.lock().unwrap().clear();
    Ok(msg)
}

#[tauri::command]
async fn remove_mascot(name: String) -> Result<String, String> {
    blocking(move || {
        let how = shimejictl::remove_prototype(&name)?;
        let _ = library::forget(&name);
        Ok(how)
    })
    .await
}

#[tauri::command]
async fn remove_many(names: Vec<String>) -> Result<String, String> {
    blocking(move || remove_many_inner(names)).await
}

fn remove_many_inner(names: Vec<String>) -> Result<String, String> {
    let mut ok = 0usize;
    let mut errors = Vec::new();
    for name in &names {
        match shimejictl::remove_prototype(name) {
            Ok(_) => {
                let _ = library::forget(name);
                ok += 1;
            }
            Err(e) => errors.push(format!("{name}: {e}")),
        }
    }
    if errors.is_empty() {
        Ok(format!("Removed {ok}"))
    } else {
        Err(format!(
            "Removed {ok} of {}. Errors:\n{}",
            names.len(),
            errors.join("\n")
        ))
    }
}

#[tauri::command]
fn toggle_favorite(name: String) -> Result<bool, String> {
    library::toggle_favorite(&name)
}

/// Exports every installed character into ONE .zip archive (a `.wlshm` file per
/// prototype), so the whole collection can be backed up or moved in one file.
#[tauri::command]
async fn export_collection(path: String, names: Option<Vec<String>>, app: tauri::AppHandle) -> Result<String, String> {
    blocking(move || {
        let state = app.state::<AppState>();
        state.cancel_export.store(false, std::sync::atomic::Ordering::SeqCst);
        export_collection_inner(Some(&app), &state.cancel_export, std::path::Path::new(&path), names.as_deref())
    })
    .await
}

/// What an export that was called off says. The UI knows this text and stays quiet.
const EXPORT_STOPPED: &str = "Export stopped.";

/// How many characters in a row may go unanswered before the export gives up: the engine has stopped, and waiting
/// for each of the rest would take an hour.
const EXPORT_GIVE_UP_AFTER: usize = 3;

/// File name for a prototype inside the archive: readable, but safe everywhere.
fn archive_name(name: &str, taken: &mut std::collections::HashSet<String>) -> String {
    let mut base: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || " -_().".contains(c) { c } else { '_' })
        .collect();
    // A leading dot would make the file hidden (helper prototypes like .Hornet_Needle).
    if base.starts_with('.') {
        base.replace_range(0..1, "_");
    }
    let mut candidate = format!("{base}.wlshm");
    let mut n = 2;
    while !taken.insert(candidate.to_lowercase()) {
        candidate = format!("{base} ({n}).wlshm");
        n += 1;
    }
    candidate
}

/// A scratch folder that is deleted when dropped, even on an early return or a
/// panic. Folders left behind by an export that was killed mid-way (app closed)
/// are swept up the next time one starts.
struct ScratchDir(std::path::PathBuf);

impl ScratchDir {
    const PREFIX: &'static str = "menagerie-export-";

    fn new() -> Result<Self, String> {
        let root = system::scratch_dir();
        if let Ok(entries) = std::fs::read_dir(&root) {
            let hour = std::time::Duration::from_secs(3600);
            for e in entries.flatten() {
                let stale = e.file_name().to_string_lossy().starts_with(Self::PREFIX)
                    && e.metadata()
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.elapsed().ok())
                        .map(|age| age > hour)
                        .unwrap_or(false);
                if stale {
                    let _ = std::fs::remove_dir_all(e.path());
                }
            }
        }
        // One per export, not per process: two at once (or two tests in one process) must not sweep each other's files.
        static COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = root.join(format!("{}{}-{n}", Self::PREFIX, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| format!("could not create a temporary folder: {e}"))?;
        Ok(Self(dir))
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `wanted` is the characters to export, or None for the whole collection.
fn export_collection_inner(app: Option<&tauri::AppHandle>, cancel: &std::sync::atomic::AtomicBool, dest: &std::path::Path, wanted: Option<&[String]>) -> Result<String, String> {
    let installed = shimejictl::installed_names();
    let names: Vec<String> = match wanted {
        // Only what is there: a name that is not installed would be asked of the engine, which does not answer.
        Some(list) => list.iter().filter(|n| installed.contains(n)).cloned().collect(),
        None => installed,
    };
    let label = match wanted {
        Some(_) => format!("Exporting {} character{}", names.len(), if names.len() == 1 { "" } else { "s" }),
        None => "Exporting your collection".to_string(),
    };
    // The progress panel is the one used for installs: a row of its own, filled as the characters go by.
    let say = |stage: &'static str, fraction: f32, message: String| {
        if let Some(app) = app {
            emit_progress(app, "export", &label, (0, 1), stage, fraction, message);
        }
    };
    say("export", 0.0, "Starting…".to_string());

    let result = export_collection_with(
        &names,
        dest,
        cancel,
        |chunk, dir| shimejictl::export_prototypes_to_dir(chunk, dir, EXPORT_BATCH_BASE_SECONDS + chunk.len() as u64),
        |name, file| {
            // The list was made a moment ago, and a character can be gone since (a half-done install is taken back):
            // asking the engine for one it no longer has is what once never got an answer.
            if !shimejictl::installed_names().iter().any(|n| n == name) {
                return Err("it is no longer installed".to_string());
            }
            shimejictl::export_prototype(name, file)
        },
        |fraction, message| say("export", fraction, message),
    );
    match &result {
        Ok(message) => say("done", 1.0, message.lines().next().unwrap_or("Done").to_string()),
        Err(e) if e == EXPORT_STOPPED => say("cancelled", 0.0, "Stopped".to_string()),
        Err(e) => say("error", 0.0, e.clone()),
    }
    result
}

/// How many characters go to the engine in one export call, and how long such a call may take besides a second
/// for each: the whole collection in one call took 3.6 s, so this is generous, and short enough that a call stuck on
/// one character is given up on before anyone thinks the app has hung.
const EXPORT_CHUNK: usize = 20;
const EXPORT_BATCH_BASE_SECONDS: u64 = 15;

/// The export itself, with the engine's part handed in so that it can be tried without one.
///
/// Characters go to the engine a chunk at a time (`export_many`: one call, one connection, about a tenth of a second
/// each). Whatever did not come out of a chunk, because the call failed or one character never got an answer, is then
/// asked for one at a time (`export_one`), which is slow but tells the one that will not answer from the rest.
fn export_collection_with(
    names: &[String],
    dest: &std::path::Path,
    cancel: &std::sync::atomic::AtomicBool,
    mut export_many: impl FnMut(&[String], &std::path::Path) -> Result<(), String>,
    mut export_one: impl FnMut(&str, &std::path::Path) -> Result<(), String>,
    progress: impl Fn(f32, String),
) -> Result<String, String> {
    use std::io::Write;
    use std::sync::atomic::Ordering::SeqCst;

    if names.is_empty() {
        return Err("There is nothing to export: the collection is empty".to_string());
    }

    let scratch = ScratchDir::new()?;
    let tmp = scratch.0.clone();
    let has_content = |p: &std::path::Path| std::fs::metadata(p).map(|m| m.len() > 0).unwrap_or(false);

    let mut taken = std::collections::HashSet::new();
    let mut exported: Vec<(String, std::path::PathBuf)> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut silent_in_a_row = 0;
    let mut gave_up = false;
    let mut done = 0;

    'chunks: for (c, chunk) in names.chunks(EXPORT_CHUNK).enumerate() {
        if cancel.load(SeqCst) {
            return Err(EXPORT_STOPPED.to_string()); // the scratch folder goes with `scratch`
        }
        let last = (done + chunk.len()).min(names.len());
        progress(done as f32 / names.len() as f32, format!("{} · {} of {}", chunk[chunk.len() - 1].replace('_', " "), last, names.len()));

        let dir = tmp.join(format!("batch-{c}"));
        std::fs::create_dir_all(&dir).map_err(|e| format!("could not create a folder for the export: {e}"))?;
        // Whatever came out is good, however the call ended.
        let _ = export_many(chunk, &dir);
        let mut missing: Vec<&String> = Vec::new();
        for name in chunk {
            let file = dir.join(shimejictl::engine_file_name(name));
            if has_content(&file) {
                exported.push((archive_name(name, &mut taken), file));
            } else {
                missing.push(name);
            }
        }

        for name in missing {
            if cancel.load(SeqCst) {
                return Err(EXPORT_STOPPED.to_string());
            }
            let file = tmp.join(format!("one-{}", archive_name(name, &mut taken)));
            match export_one(name, &file) {
                // A file with nothing in it is what a killed export leaves behind.
                Ok(()) if has_content(&file) => {
                    silent_in_a_row = 0;
                    exported.push((archive_name(name, &mut taken), file));
                }
                Ok(()) => {
                    silent_in_a_row = 0;
                    skipped.push(format!("{name}: no file was produced"));
                }
                Err(e) if shimejictl::is_no_answer(&e) => {
                    skipped.push(format!("{name}: the engine did not answer"));
                    silent_in_a_row += 1;
                    if silent_in_a_row >= EXPORT_GIVE_UP_AFTER {
                        gave_up = true;
                        break 'chunks;
                    }
                }
                Err(e) => {
                    silent_in_a_row = 0;
                    skipped.push(format!("{name}: {e}"));
                }
            }
        }
        done = last;
    }
    if cancel.load(SeqCst) {
        return Err(EXPORT_STOPPED.to_string());
    }

    progress(0.99, "Writing the archive…".to_string());
    let result = (|| -> Result<(), String> {
        if exported.is_empty() {
            return Err(format!("Nothing was exported:\n{}", skipped.join("\n")));
        }
        if let Some(dir) = dest.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
        }
        let out = std::fs::File::create(dest).map_err(|e| format!("could not create {}: {e}", dest.display()))?;
        let mut zip = zip::ZipWriter::new(std::io::BufWriter::new(out));
        // The files are already compressed archives of the engine's own: deflating them again buys little and costs time.
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, file) in &exported {
            let bytes = std::fs::read(file).map_err(|e| format!("could not read {name}: {e}"))?;
            zip.start_file(name.as_str(), opts).map_err(|e| format!("zip error: {e}"))?;
            zip.write_all(&bytes).map_err(|e| format!("zip error: {e}"))?;
        }
        zip.finish().map_err(|e| format!("could not finish the archive: {e}"))?;
        Ok(())
    })();
    drop(scratch);
    result?;

    let size = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
    let mut msg = format!(
        "Exported {} character{} to {} ({:.1} MB)",
        exported.len(),
        if exported.len() == 1 { "" } else { "s" },
        dest.display(),
        size as f64 / 1_048_576.0
    );
    if !skipped.is_empty() {
        msg.push_str(&format!("\nSkipped {}:\n{}", skipped.len(), skipped.join("\n")));
    }
    if gave_up {
        msg.push_str(&format!(
            "\nThe engine stopped answering, so the other {} were not tried. Restart the overlay and export again.",
            names.len() - exported.len() - skipped.len()
        ));
    }
    Ok(msg)
}

// ---------------------------------------------------------------------------
// Scene: active characters
// ---------------------------------------------------------------------------

/// How many characters the app summoned this session.
///
/// shimejictl has no command that lists active mascots (verified: `mascot` only
/// supports summon / dismiss / set-behavior), so an exact picture of the screen
/// is not available. We keep our own approximate count and say plainly in the
/// UI that it is approximate.
#[tauri::command]
fn summoned_tally(state: State<'_, AppState>) -> Vec<(String, usize)> {
    let tally = state.tally.lock().unwrap();
    let mut v: Vec<(String, usize)> = tally.iter().map(|(k, c)| (k.clone(), *c)).collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

// ---------------------------------------------------------------------------
// Presets (saved sets of characters)
// ---------------------------------------------------------------------------

#[tauri::command]
fn list_presets() -> Vec<Preset> {
    library::presets()
}

#[tauri::command]
fn save_preset(name: String, members: Vec<(String, usize)>) -> Result<(), String> {
    library::save_preset(Preset { name, members })
}

#[tauri::command]
fn delete_preset(name: String) -> Result<(), String> {
    library::delete_preset(&name)
}

#[tauri::command]
async fn run_preset(name: String, state: State<'_, AppState>) -> Result<String, String> {
    let preset = library::presets()
        .into_iter()
        .find(|p| p.name == name)
        .ok_or("preset not found")?;

    let result = summon_batch(preset.members, state).await?;
    let mut msg = format!("Summoned {}", result.spawned);
    if !result.missing.is_empty() {
        msg.push_str(&format!(" (not installed any more: {})", result.missing.join(", ")));
    }
    Ok(msg)
}

// ---------------------------------------------------------------------------
// Niri
// ---------------------------------------------------------------------------

#[tauri::command]
fn get_autostart() -> Vec<(String, usize)> {
    library::autostart()
}

#[tauri::command]
fn set_autostart(list: Vec<(String, usize)>) -> Result<(), String> {
    library::set_autostart(list)
}

#[tauri::command]
fn niri_render(options: niri::Options) -> String {
    niri::render(&options)
}

/// The same login commands as a plain shell script, for desktops that are not Niri.
#[tauri::command]
fn startup_snippet(options: niri::Options) -> String {
    niri::shell_snippet(&options)
}

/// Whether the login file this app writes for any desktop is in place, and what it runs.
#[tauri::command]
async fn autostart_status() -> autostart::Status {
    blocking(|| Ok(autostart::status())).await.unwrap_or_else(|_: String| autostart::Status::default())
}

/// Turns launch-at-login on for desktops that are not niri, by writing the one file the
/// XDG autostart specification defines. Niri has its own, richer setup (see `niri_*`).
#[tauri::command]
async fn autostart_enable(options: niri::Options) -> Result<autostart::Status, String> {
    blocking(move || autostart::enable(&niri::shell_snippet(&options))).await
}

#[tauri::command]
async fn autostart_disable() -> Result<autostart::Status, String> {
    blocking(autostart::disable).await
}

#[tauri::command]
async fn niri_status() -> niri::Status {
    tauri::async_runtime::spawn_blocking(niri::status).await.unwrap_or(niri::Status {
        active: false,
        file: String::new(),
        include_line: String::new(),
        backup: String::new(),
        validation: String::new(),
    })
}

/// Turns launch-at-login on (writes our file, adds the include line to config.kdl).
#[tauri::command]
async fn niri_enable(options: niri::Options) -> Result<niri::Status, String> {
    blocking(move || niri::enable(&options)).await
}

/// Turns it off again (removes the include line and our file).
#[tauri::command]
async fn niri_disable() -> Result<niri::Status, String> {
    blocking(niri::disable).await
}

/// Which of these keys the user's own Niri config already binds.
#[tauri::command]
async fn niri_conflicts(keys: Vec<String>) -> Vec<niri::Conflict> {
    tauri::async_runtime::spawn_blocking(move || niri::conflicts(&keys)).await.unwrap_or_default()
}

/// Rewrites our file with new options while the feature is on.
#[tauri::command]
async fn niri_apply(options: niri::Options) -> Result<niri::Status, String> {
    blocking(move || niri::apply(&options)).await
}

#[tauri::command]
fn hyprland_render(options: niri::Options) -> String {
    hyprland::preview(&options)
}

#[tauri::command]
async fn hyprland_status() -> hyprland::Status {
    tauri::async_runtime::spawn_blocking(hyprland::status).await.unwrap_or(hyprland::Status {
        active: false,
        file: String::new(),
        include_line: String::new(),
        backup: String::new(),
        validation: String::new(),
        skipped: Vec::new(),
    })
}

/// Turns launch-at-login and keybinds on (writes our file, adds the `source` line to hyprland.conf).
#[tauri::command]
async fn hyprland_enable(options: niri::Options) -> Result<hyprland::Status, String> {
    blocking(move || hyprland::enable(&options)).await
}

#[tauri::command]
async fn hyprland_disable() -> Result<hyprland::Status, String> {
    blocking(hyprland::disable).await
}

/// Which of these keys (in niri's spelling, as the Startup tab captures them) the user's own Hyprland config binds.
#[tauri::command]
async fn hyprland_conflicts(keys: Vec<String>) -> Vec<hyprland::Conflict> {
    tauri::async_runtime::spawn_blocking(move || hyprland::conflicts(&keys)).await.unwrap_or_default()
}

#[tauri::command]
async fn hyprland_apply(options: niri::Options) -> Result<hyprland::Status, String> {
    blocking(move || hyprland::apply(&options)).await
}

#[tauri::command]
fn plugin_status() -> shimejictl::PluginStatus {
    shimejictl::plugin_status()
}

/// Preferences saved on disk (see prefs.rs).
#[tauri::command]
fn prefs_load() -> std::collections::BTreeMap<String, String> {
    prefs::load()
}

#[tauri::command]
fn prefs_set(key: String, value: Option<String>) -> Result<(), String> {
    prefs::set(&key, value.as_deref())
}

/// The animation frames of an installed character, for the hover preview.
#[tauri::command]
async fn character_frames(name: String) -> Vec<String> {
    tauri::async_runtime::spawn_blocking(move || pack::local_frames(&name, 46)).await.unwrap_or_default()
}

/// Who is really on screen: [(character, how many)]. Errors when the overlay
/// cannot be queried; the UI then falls back to `summoned_tally`.
#[tauri::command]
async fn on_screen() -> Result<Vec<(String, usize)>, String> {
    blocking(shimejictl::on_screen).await
}

/// Stops a summon that is under way. Answers whether there was one to stop.
///
/// Summoning a crowd holds the one connection to the overlay for seconds, and the app
/// runs its commands one at a time, so "Dismiss all" pressed during it would sit and wait
/// for the very thing it is meant to undo. It calls this first.
#[tauri::command]
fn cancel_summon() -> bool {
    shimejictl::cancel_summon()
}

/// Dismisses one copy (`all = false`) or every copy of a character.
#[tauri::command]
async fn dismiss_character(name: String, all: bool, state: State<'_, AppState>) -> Result<usize, String> {
    let n = name.clone();
    let gone = blocking(move || shimejictl::dismiss_character(&n, all)).await?;
    let mut tally = state.tally.lock().unwrap();
    if all {
        tally.remove(&name);
    } else if let Some(c) = tally.get_mut(&name) {
        *c = c.saturating_sub(gone);
        if *c == 0 {
            tally.remove(&name);
        }
    }
    Ok(gone)
}

#[derive(Serialize)]
struct OverlayStatus {
    /// The process exists.
    running: bool,
    /// ...and it is listening, so it can be talked to.
    ready: bool,
    pid: Option<u32>,
}

/// Whether the overlay is really alive, judged by the process and not by a socket
/// file that a crash leaves behind.
#[tauri::command]
fn overlay_status() -> OverlayStatus {
    let pid = shimejictl::overlay_pid();
    OverlayStatus { running: pid.is_some(), ready: shimejictl::overlay_ready(), pid }
}

/// What the app runs on, for the first-run notice: is the engine installed at all, and is
/// this a session it can work in. Only facts; the words are the front end's.
#[derive(Serialize)]
struct Environment {
    /// Where `shimejictl` was found (None: wl_shimeji is not installed).
    engine: Option<String>,
    /// `wayland`, `x11`, `tty`, or empty when the session does not say.
    session: String,
    /// `XDG_CURRENT_DESKTOP`, for example `niri`, `KDE` or `GNOME`.
    desktop: String,
    /// `arch`, `fedora`, `debian`, `suse`, `nix` or `unknown`.
    family: String,
    /// What the compositor itself says it can do (None: no Wayland session to ask).
    protocols: Option<wayland::Support>,
    /// This is a niri session and the `niri` command is there, so the app can write its
    /// config. `XDG_CURRENT_DESKTOP` alone is no good: plenty of compositors leave it
    /// unset, and the app would then offer to write a niri config on a machine with no niri.
    niri: bool,
    /// The same for Hyprland: a Hyprland session whose `hyprland.conf` exists, so the app can write to it.
    hyprland: bool,
    /// `niri`, `hyprland`, `kde`, `sway`, `gnome` or `other` (see desktop.rs).
    compositor: &'static str,
}

#[tauri::command]
fn environment_check() -> Environment {
    let var = |name: &str| std::env::var(name).unwrap_or_default();
    let mut session = var("XDG_SESSION_TYPE").to_lowercase();
    if session.is_empty() && !var("WAYLAND_DISPLAY").is_empty() {
        session = "wayland".to_string();
    }
    Environment {
        engine: system::which("shimejictl").map(|p| p.display().to_string()),
        session,
        desktop: var("XDG_CURRENT_DESKTOP"),
        family: system::distro_family().to_string(),
        protocols: wayland::support(),
        niri: niri::is_niri_session(),
        hyprland: desktop::detect() == desktop::Compositor::Hyprland && hyprland::config_exists(),
        compositor: desktop::detect().as_str(),
    }
}

/// Crashes of the overlay on record since a moment (milliseconds since the epoch).
/// An error means the system keeps no such record.
#[tauri::command]
async fn overlay_crashes(since_ms: u64) -> Result<Vec<shimejictl::Crash>, String> {
    blocking(move || shimejictl::overlay_crashes(since_ms)).await
}

#[derive(Serialize)]
struct Repaired {
    name: String,
    /// How many missing pictures were added.
    pictures: usize,
}

/// Gives every character that lacks pictures a copy of the nearest one (files are only
/// added). Such a character brings the overlay down when it is summoned. Returns who
/// was fixed, and whether the running overlay must restart to use them.
#[tauri::command]
async fn repair_characters() -> Vec<Repaired> {
    blocking(|| Ok(shimejictl::repair_all()))
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|(name, pictures)| Repaired { name, pictures })
        .collect()
}

/// The character the overlay last complained about, if its log says so.
#[tauri::command]
fn overlay_culprit() -> Option<String> {
    shimejictl::overlay_culprit()
}

/// Starts the overlay (and waits until it answers).
#[tauri::command]
async fn start_overlay() -> Result<u32, String> {
    blocking(shimejictl::start_overlay).await
}

/// The last lines the overlay printed, for "what happened when it crashed".
#[tauri::command]
fn overlay_log(lines: Option<usize>) -> String {
    shimejictl::overlay_log_tail(lines.unwrap_or(12))
}

// ---------------------------------------------------------------------------
// Settings and diagnostics
// ---------------------------------------------------------------------------

#[tauri::command]
async fn config_list() -> Result<Vec<shimejictl::ConfigOption>, String> {
    blocking(shimejictl::config_list).await
}

#[tauri::command]
async fn config_set(key: String, value: String) -> Result<shimejictl::SetOutcome, String> {
    blocking(move || shimejictl::config_set(&key, &value)).await
}

/// Name for comparison across sources: ignores case, dots and spaces.
fn norm(s: &str) -> String {
    downloads::norm(s)
}

/// A character's group ("universe") from the saved catalogs, without the network.
/// shimejis.xyz first (pack title), then cachomon.com (franchise).
fn universe_of(name: &str, xyz: &[Character], cacho: &[cachomon::CachoEntry]) -> String {
    let key = norm(name);
    if let Some(c) = xyz.iter().find(|c| norm(&c.name) == key) {
        return c.pack_title.clone();
    }
    cacho
        .iter()
        .find(|c| norm(&c.name) == key && !c.franchise.is_empty())
        .map(|c| c.franchise.clone())
        .unwrap_or_default()
}

/// Fills in the Collection: the picture (a frame from the prototype itself, or from
/// the catalog if that is missing) and the group. Nothing that already exists is
/// overwritten, so a group that an earlier run worked out is not recomputed.
///
/// Runs automatically after the Collection loads; no button to press.
///
#[tauri::command]
async fn match_sprites() -> Result<String, String> {
    let installed = shimejictl::installed_names();
    let meta = library::all_entries();

    let needs = |name: &String| {
        let e = meta.get(name);
        let sprite_ok = e
            .map(|e| !e.sprite_path.is_empty() && std::path::Path::new(&e.sprite_path).exists())
            .unwrap_or(false);
        let title_ok = e.map(|e| !e.pack_title.is_empty()).unwrap_or(false);
        !sprite_ok || !title_ok
    };
    if !installed.iter().any(needs) {
        return Ok("Everything is already filled in".to_string());
    }

    // shimejis.xyz catalog: from the cache only; we build it (slow, needs the network)
    // only when there is no cache. A network failure is no reason to give up: local frames remain.
    let xyz: Vec<Character> = match library::read_index() {
        Some(v) => v,
        None => match catalog::build_index().await {
            Ok(built) => {
                let _ = library::write_index(&built);
                built
            }
            Err(_) => Vec::new(),
        },
    };
    let cacho = match cachomon::cached() {
        v if !v.is_empty() => v,
        _ => cachomon::index(false).await.map(|i| i.entries).unwrap_or_default(),
    };

    let mut filled = 0usize;

    for name in installed.iter().filter(|n| needs(n)) {
        let old = meta.get(name).cloned().unwrap_or_default();
        let key = norm(name);

        // 1) picture: local frame → catalog sprite → cachomon thumbnail
        let mut sprite_path = if !old.sprite_path.is_empty() && std::path::Path::new(&old.sprite_path).exists() {
            old.sprite_path.clone()
        } else {
            let n = name.clone();
            blocking(move || Ok(local_sprite(&n))).await?
        };
        let xyz_hit = xyz.iter().find(|c| norm(&c.name) == key);
        if sprite_path.is_empty() {
            if let Some(c) = xyz_hit {
                sprite_path = library::cache_sprite(name, &c.sprite).await;
            }
        }
        if sprite_path.is_empty() {
            if let Some(c) = cacho.iter().find(|c| norm(&c.name) == key) {
                sprite_path = library::cache_sprite(name, &c.thumb).await;
            }
        }

        // 2) group: only if not set yet
        let pack_title = if old.pack_title.is_empty() {
            universe_of(name, &xyz, &cacho)
        } else {
            old.pack_title.clone()
        };

        if sprite_path == old.sprite_path && pack_title == old.pack_title {
            continue;
        }
        let slug = if old.slug.is_empty() {
            xyz_hit.map(|c| c.slug.clone()).unwrap_or_default()
        } else {
            old.slug.clone()
        };
        let _ = library::record(
            name,
            Entry { sprite_path, slug, pack_title, artist: old.artist, favorite: old.favorite },
        );
        filled += 1;
    }

    Ok(format!("Cards updated: {filled}"))
}

#[tauri::command]
async fn cachomon_index(refresh: bool) -> Result<cachomon::CachoIndex, String> {
    cachomon::index(refresh).await
}

#[tauri::command]
async fn recent_downloads() -> Result<Vec<downloads::DownloadFile>, String> {
    blocking(|| Ok(downloads::recent_archives())).await
}

/// The folder the app watches for archives, so the UI can tell the user the
/// real path instead of guessing "Downloads".
#[tauri::command]
fn downloads_path() -> String {
    downloads::downloads_dir().to_string_lossy().into_owned()
}

/// Opens that folder in the file manager.
#[tauri::command]
fn open_downloads() -> Result<(), String> {
    system::open_folder(&downloads::downloads_dir())
}

/// Shows a folder in the file manager. Only folders that belong to this app, the
/// characters, or the Downloads are allowed: the page cannot ask for any path.
#[tauri::command]
fn open_folder(which: String) -> Result<(), String> {
    let dir = match which.as_str() {
        "downloads" => downloads::downloads_dir(),
        "characters" => shimejictl::characters_dir(),
        "app" => library::data_dir(),
        other => return Err(format!("unknown folder: {other}")),
    };
    system::open_folder(&dir)
}

#[derive(Serialize)]
struct StorageInfo {
    characters_dir: String,
    characters_count: usize,
    characters_bytes: u64,
    /// Set when the characters folder is a link to somewhere else.
    characters_link: Option<String>,
    app_dir: String,
    app_bytes: u64,
    downloads_dir: String,
}

/// Where everything lives, and how much space it takes, for the Settings page.
#[tauri::command]
async fn storage_info() -> StorageInfo {
    blocking(|| {
        let characters = shimejictl::characters_dir();
        let sizes = shimejictl::installed_sizes();
        let app = library::data_dir();
        Ok(StorageInfo {
            characters_dir: characters.display().to_string(),
            characters_count: sizes.len(),
            characters_bytes: sizes.values().sum(),
            characters_link: std::fs::read_link(&characters).ok().map(|p| p.display().to_string()),
            app_dir: app.display().to_string(),
            app_bytes: shimejictl::dir_size(&app),
            downloads_dir: downloads::downloads_dir().display().to_string(),
        })
    })
    .await
    .unwrap_or_else(|_: String| StorageInfo {
        characters_dir: String::new(),
        characters_count: 0,
        characters_bytes: 0,
        characters_link: None,
        app_dir: String::new(),
        app_bytes: 0,
        downloads_dir: String::new(),
    })
}

/// Checks a folder the person is about to choose for the characters, without changing
/// anything: the UI asks before it acts. `true` means it already holds characters.
#[tauri::command]
async fn check_characters_folder(path: String) -> Result<bool, String> {
    blocking(move || relocate::check_target(&shimejictl::characters_dir(), std::path::Path::new(&path))).await
}

/// Moves the characters to another folder and leaves a link behind, because wl_shimeji
/// has no setting for where they live.
#[tauri::command]
async fn move_characters_folder(path: String) -> Result<relocate::Moved, String> {
    let target = std::path::PathBuf::from(&path);
    blocking(move || {
        let moved = relocate::move_characters(&shimejictl::characters_dir(), &target)?;
        // The overlay reads a character once, when it loads it. After the files have
        // moved, anything it has open is best re-read at its next start.
        shimejictl::forget_stale();
        Ok(moved)
    })
    .await
}

/// Where to watch for downloaded archives. An empty path goes back to the system's own
/// Downloads folder.
#[tauri::command]
async fn set_downloads_dir(path: String) -> Result<String, String> {
    blocking(move || {
        let trimmed = path.trim().to_string();
        if trimmed.is_empty() {
            prefs::set("downloads-dir", None)?;
        } else {
            let dir = std::path::PathBuf::from(&trimmed);
            if !dir.is_dir() {
                return Err(format!("{trimmed} is not a folder"));
            }
            prefs::set("downloads-dir", Some(&trimmed))?;
        }
        Ok(downloads::downloads_dir().display().to_string())
    })
    .await
}

/// Whether the tray icon is wanted, and turning it on or off without a restart.
#[tauri::command]
fn tray_enabled() -> bool {
    // "On" means there is an icon, so a system that cannot show one reads as off.
    tray::wanted() && tray::available()
}

#[tauri::command]
fn set_tray_enabled(app: tauri::AppHandle, on: bool) -> Result<(), String> {
    tray::apply(&app, on)
}

/// The last lines the overlay printed, for the log view in Settings.
#[tauri::command]
async fn overlay_log_full(lines: Option<usize>) -> String {
    let n = lines.unwrap_or(400);
    blocking(move || Ok(shimejictl::overlay_log_tail(n))).await.unwrap_or_default()
}

/// Moves an archive that was installed from the Downloads folder to the trash (so
/// it can be got back). Refuses anything that is not a `.zip` inside Downloads, so
/// this cannot be turned into "delete any file".
#[tauri::command]
fn trash_archive(path: String) -> Result<(), String> {
    let file = std::path::PathBuf::from(&path)
        .canonicalize()
        .map_err(|e| format!("{path}: {e}"))?;
    let downloads = downloads::downloads_dir()
        .canonicalize()
        .map_err(|e| format!("the Downloads folder is not there: {e}"))?;
    let is_zip = file.extension().map(|x| x.eq_ignore_ascii_case("zip")).unwrap_or(false);
    if !file.is_file() || !is_zip || !file.starts_with(&downloads) {
        return Err("Only .zip files in the Downloads folder are moved to the trash".to_string());
    }
    // An archive of ready `.wlshm` files is an export: a backup or a move to another computer, not a download to
    // clear away. It was trashed after "installing" it back, and to whoever made it that looked like it had vanished.
    if holds_exports(&file) {
        return Err("Only .zip files in the Downloads folder are moved to the trash, and not an export of your own characters".to_string());
    }
    system::trash(&file)
}

/// Whether a `.zip` holds `.wlshm` files, which is what this app's own export writes.
fn holds_exports(zip: &std::path::Path) -> bool {
    let Ok(file) = std::fs::File::open(zip) else { return false };
    let Ok(archive) = zip::ZipArchive::new(file) else { return false };
    let found = archive.file_names().any(|n| n.to_lowercase().ends_with(".wlshm"));
    found
}

fn main() {
    system::extend_path();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(AppState::default())
        .setup(|app| {
            let scope = app.asset_protocol_scope();
            let _ = scope.allow_directory(library::sprites_dir(), true);
            library::reset_hand_set_groups_once();
            // Keeps the overlay's log from growing without limit, and ends an overlay that has lost the desktop.
            shimejictl::watch_log();
            // A new machine starts at normal size, not the engine's half size (off the start-up path: it asks the engine).
            std::thread::spawn(shimejictl::first_run_defaults);
            // The tray icon, unless it was turned off. Desktops without a place
            // to put one simply do not show it.
            if tray::wanted() {
                if let Err(e) = tray::install(app.handle()) {
                    eprintln!("no tray icon: {e}");
                }
            }
            // A character that lacks pictures crashes the overlay when it appears.
            // Fixing that only adds files, so it is done at every start, in the background.
            std::thread::spawn(|| {
                let _ = shimejictl::repair_all();
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            fetch_packs,
            fetch_characters,
            cachomon_index,
            recent_downloads,
            downloads_path,
            open_downloads,
            catalog_index,
            install_characters,
            cancel_install,
            prepare_local_archive,
            install_local_selected,
            cancel_local,
            list_installed,
            summon_mascot,
            dismiss_all,
            cancel_summon,
            remove_mascot,
            remove_many,
            toggle_favorite,
            export_collection,
            summoned_tally,
            list_presets,
            save_preset,
            delete_preset,
            run_preset,
            niri_render,
            niri_status,
            autostart_status,
            autostart_enable,
            autostart_disable,
            startup_snippet,
            niri_enable,
            niri_disable,
            niri_apply,
            niri_conflicts,
            hyprland_render,
            hyprland_status,
            hyprland_enable,
            hyprland_disable,
            hyprland_apply,
            hyprland_conflicts,
            overlay_status,
            environment_check,
            start_overlay,
            overlay_crashes,
            repair_characters,
            overlay_culprit,
            overlay_log,
            summon_batch,
            open_folder,
            storage_info,
            set_downloads_dir,
            tray_enabled,
            set_tray_enabled,
            overlay_log_full,
            check_characters_folder,
            move_characters_folder,
            trash_archive,
            on_screen,
            dismiss_character,
            prefs_load,
            prefs_set,
            character_frames,
            plugin_status,
            get_autostart,
            set_autostart,
            config_list,
            config_set,
            match_sprites
        ])
        .run(tauri::generate_context!())
        .expect("error while running the application");
}

#[cfg(test)]
mod index_tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_saved_index_is_trusted_for_half_a_day_and_not_when_it_is_missing_or_old() {
        assert!(index_is_fresh(Some(Duration::from_secs(60))));
        assert!(index_is_fresh(Some(Duration::from_secs(12 * 3600 - 1))));
        assert!(!index_is_fresh(Some(Duration::from_secs(12 * 3600))), "half a day is where trust ends");
        assert!(!index_is_fresh(Some(Duration::from_secs(3 * 24 * 3600))), "the one that was three days old");
        assert!(!index_is_fresh(None), "no file, no trust");
    }
}

#[cfg(test)]
mod archive_tests {
    use super::*;

    #[test]
    fn an_export_is_recognised_and_a_download_is_not() {
        let dir = std::env::temp_dir().join(format!("menagerie-archive-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let write = |name: &str, inside: &str| {
            let path = dir.join(name);
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
            zip.start_file(inside, zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut zip, b"x").unwrap();
            zip.finish().unwrap();
            path
        };
        assert!(holds_exports(&write("export.zip", "Blob.wlshm")));
        assert!(!holds_exports(&write("download.zip", "Blob/actions.xml")));
        assert!(!holds_exports(&dir.join("missing.zip")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod export_tests {
    use super::*;

    /// An engine that cannot export a folder of them: everything falls back to one at a time.
    fn no_batch(_: &[String], _: &std::path::Path) -> Result<(), String> {
        Err("batching is off".to_string())
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// An exporter that writes a small file, except for the names it is told to fail on.
    fn exporter<'a>(fail: &'a [(&'a str, &'a str)]) -> impl FnMut(&str, &std::path::Path) -> Result<(), String> + 'a {
        move |name, file| match fail.iter().find(|(n, _)| *n == name) {
            Some((_, "silent")) => Err(format!("{} `shimejictl prototypes export -i {name}` within 40 s", "The engine did not answer")),
            Some((_, why)) => Err(why.to_string()),
            None => std::fs::write(file, format!("archive of {name}")).map_err(|e| e.to_string()),
        }
    }

    fn dest(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("menagerie-export-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("out.zip")
    }

    #[test]
    fn one_character_that_never_answers_is_skipped_and_the_rest_are_exported() {
        let out = dest("skip");
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let msg = export_collection_with(&names(&["Ampharos", "Ezio", "Yusuf"]), &out, &cancel, no_batch, exporter(&[("Yusuf", "silent")]), |_, _| {}).unwrap();
        assert!(msg.starts_with("Exported 2 characters"), "{msg}");
        assert!(msg.contains("Yusuf: the engine did not answer"), "{msg}");
        let zip = zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).unwrap();
        assert_eq!(zip.len(), 2);
    }

    #[test]
    fn an_engine_that_has_stopped_is_given_up_on_after_three_silent_ones_in_a_row() {
        let out = dest("giveup");
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let list = names(&["A", "B", "C", "D", "E", "F", "G"]);
        let mut tried = Vec::new();
        let fail = [("C", "silent"), ("D", "silent"), ("E", "silent")];
        let mut inner = exporter(&fail);
        let msg = export_collection_with(&list, &out, &cancel, no_batch, |n, f| { tried.push(n.to_string()); inner(n, f) }, |_, _| {}).unwrap();
        assert_eq!(tried, ["A", "B", "C", "D", "E"], "F and G must not be tried: each would wait 40 s");
        assert!(msg.starts_with("Exported 2 characters") && msg.contains("The engine stopped answering, so the other 2 were not tried"), "{msg}");
    }

    #[test]
    fn a_silent_one_between_answers_does_not_count_towards_giving_up() {
        let out = dest("scattered");
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let fail = [("B", "silent"), ("D", "silent"), ("F", "silent")];
        let msg = export_collection_with(&names(&["A", "B", "C", "D", "E", "F", "G"]), &out, &cancel, no_batch, exporter(&fail), |_, _| {}).unwrap();
        assert!(msg.starts_with("Exported 4 characters") && !msg.contains("stopped answering"), "{msg}");
    }

    #[test]
    fn a_file_with_nothing_in_it_is_not_a_success() {
        let out = dest("empty");
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let msg = export_collection_with(&names(&["A", "B"]), &out, &cancel, no_batch, |name, file| {
            std::fs::write(file, if name == "A" { "" } else { "something" }).map_err(|e| e.to_string())
        }, |_, _| {}).unwrap();
        assert!(msg.starts_with("Exported 1 character ") && msg.contains("A: no file was produced"), "{msg}");
    }

    #[test]
    fn cancelling_stops_the_export_writes_no_archive_and_says_so() {
        let out = dest("cancel");
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let mut seen = 0;
        let result = export_collection_with(&names(&["A", "B", "C", "D"]), &out, &cancel, no_batch, |n, f| {
            seen += 1;
            if n == "B" { cancel.store(true, std::sync::atomic::Ordering::SeqCst); }
            std::fs::write(f, "x").map_err(|e| e.to_string())
        }, |_, _| {});
        assert_eq!(result.unwrap_err(), EXPORT_STOPPED);
        assert_eq!(seen, 2, "nothing after the one being done when Cancel was pressed");
        assert!(!out.exists(), "a cancelled export leaves no archive");
    }

    #[test]
    fn progress_climbs_chunk_by_chunk_and_ends_on_the_archive() {
        let out = dest("progress");
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let seen = std::cell::RefCell::new(Vec::new());
        let list: Vec<String> = (1..=45).map(|i| format!("Char_{i}")).collect();
        export_collection_with(&list, &out, &cancel, batch_writing_everything(), exporter(&[]), |f, m| seen.borrow_mut().push((f, m))).unwrap();
        let seen = seen.into_inner();
        assert_eq!(seen[0], (0.0, "Char 20 · 20 of 45".to_string()), "underscores read as spaces");
        assert_eq!(seen[1].1, "Char 40 · 40 of 45");
        assert_eq!(seen[2].1, "Char 45 · 45 of 45");
        assert_eq!(seen.last().unwrap().1, "Writing the archive…");
        assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0), "never goes back");
    }

    #[test]
    fn archive_names_are_safe_unique_and_not_hidden() {
        let mut taken = std::collections::HashSet::new();
        assert_eq!(archive_name("Mosscreep (Orange)", &mut taken), "Mosscreep (Orange).wlshm");
        assert_eq!(archive_name(".Hornet_Needle", &mut taken), "_Hornet_Needle.wlshm");
        assert_eq!(archive_name("a/b", &mut taken), "a_b.wlshm");
        // Two names that map to the same file name must not overwrite each other.
        assert_eq!(archive_name("a?b", &mut taken), "a_b (2).wlshm");
    }

    #[test]
    fn scratch_dir_is_removed_on_drop() {
        let dir = {
            let scratch = ScratchDir::new().unwrap();
            std::fs::write(scratch.0.join("x.wlshm"), b"x").unwrap();
            assert!(scratch.0.exists());
            scratch.0.clone()
        };
        assert!(!dir.exists(), "scratch folder survived the drop");
    }

    /// Exports the real collection into a temporary zip and checks its contents.
    /// Read-only for the collection: `cargo test live_export -- --ignored`.
    #[test]
    #[ignore]
    fn live_export_writes_a_zip_with_every_prototype() {
        let names = shimejictl::installed_names();
        if names.is_empty() {
            return;
        }
        let dest = std::env::temp_dir().join(format!("menagerie-live-export-{}.zip", std::process::id()));
        let msg = export_collection_inner(None, &std::sync::atomic::AtomicBool::new(false), &dest, None).expect("export");
        let zip = zip::ZipArchive::new(std::fs::File::open(&dest).unwrap()).unwrap();
        let count = zip.len();
        let _ = std::fs::remove_file(&dest);
        assert_eq!(count, names.len(), "{msg}");
    }

    /// An engine that exports a folder of them in one go, writing each file under the name the engine would give it.
    fn batch_writing_everything() -> impl FnMut(&[String], &std::path::Path) -> Result<(), String> {
        |chunk, dir| {
            for name in chunk {
                std::fs::write(dir.join(shimejictl::engine_file_name(name)), format!("archive of {name}")).map_err(|e| e.to_string())?;
            }
            Ok(())
        }
    }

    #[test]
    fn a_whole_collection_goes_through_one_call_per_chunk_and_never_one_by_one() {
        let out = dest("batch");
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let list: Vec<String> = (1..=45).map(|i| format!("Char {i}")).collect();
        let (mut calls, mut singles) = (Vec::new(), 0);
        let mut inner = batch_writing_everything();
        let msg = export_collection_with(
            &list, &out, &cancel,
            |chunk, dir| { calls.push(chunk.len()); inner(chunk, dir) },
            |_, _| { singles += 1; Ok(()) },
            |_, _| {},
        ).unwrap();
        assert_eq!(calls, [20, 20, 5], "45 characters, three calls");
        assert_eq!(singles, 0, "nothing needed asking for on its own");
        assert!(msg.starts_with("Exported 45 characters"), "{msg}");
        let zip = zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).unwrap();
        assert_eq!(zip.len(), 45);
        assert!(zip.file_names().any(|n| n == "Char 7.wlshm"), "readable names, spaces kept: {:?}", zip.file_names().take(3).collect::<Vec<_>>());
    }

    #[test]
    fn a_chunk_that_stalls_keeps_what_came_out_and_asks_for_the_rest_one_at_a_time() {
        let out = dest("stall");
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let list = names(&["A", "B", "C", "D"]);
        // The engine wrote A and B, then stopped answering on C (and never got to D).
        let many = |chunk: &[String], dir: &std::path::Path| {
            for name in &chunk[..2] {
                std::fs::write(dir.join(shimejictl::engine_file_name(name)), "x").unwrap();
            }
            Err("The engine did not answer `shimejictl prototypes export …` within 19 s".to_string())
        };
        let mut asked = Vec::new();
        let fail = [("C", "silent")];
        let mut inner = exporter(&fail);
        let msg = export_collection_with(&list, &out, &cancel, many, |n, f| { asked.push(n.to_string()); inner(n, f) }, |_, _| {}).unwrap();
        assert_eq!(asked, ["C", "D"], "only what did not come out is asked for again");
        assert!(msg.starts_with("Exported 3 characters") && msg.contains("C: the engine did not answer"), "{msg}");
    }
}
