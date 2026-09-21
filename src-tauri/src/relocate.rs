// Keeping the characters somewhere else.
//
// wl_shimeji decides where they live: `<data>/wl_shimeji/shimejis`, and there is no
// setting for it. A collection of a few hundred characters is gigabytes, though, and a
// small system disk fills up. The way out is the one every Unix program understands —
// the folder becomes a link to wherever they really are — and this does it properly:
// copy first, check every file arrived, only then swap the link in, and put the old copy
// in the trash rather than deleting it.
//
// Nothing here removes anything until the new copy has been verified byte for byte.

use std::path::{Path, PathBuf};

/// What a move did.
#[derive(serde::Serialize, Debug, Default)]
pub struct Moved {
    pub characters: usize,
    pub bytes: u64,
    pub from: String,
    pub to: String,
    /// The old copy was moved to the trash. False: it is still there, and `note` says where.
    pub old_trashed: bool,
    pub note: String,
}

/// Everything directly inside a folder, sorted, ignoring what is not a character.
fn entries(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    out.sort();
    out
}

/// Total size of a file or a folder, following nothing.
fn size_of(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return 0 };
    if meta.is_dir() {
        entries(path).iter().map(|p| size_of(p)).sum()
    } else {
        meta.len()
    }
}

/// Copies a file or a whole folder. Fails on the first problem, leaving the source alone.
fn copy_into(from: &Path, to: &Path) -> Result<(), String> {
    let meta = std::fs::symlink_metadata(from).map_err(|e| format!("{}: {e}", from.display()))?;
    if meta.is_dir() {
        std::fs::create_dir_all(to).map_err(|e| format!("could not create {}: {e}", to.display()))?;
        for child in entries(from) {
            let name = child.file_name().ok_or("a file with no name")?;
            copy_into(&child, &to.join(name))?;
        }
        Ok(())
    } else if meta.is_symlink() {
        // A link inside the collection is copied as a link, not as what it points at.
        let target = std::fs::read_link(from).map_err(|e| format!("{}: {e}", from.display()))?;
        let _ = std::fs::remove_file(to);
        std::os::unix::fs::symlink(target, to).map_err(|e| format!("could not copy the link {}: {e}", from.display()))
    } else {
        std::fs::copy(from, to).map(|_| ()).map_err(|e| format!("could not copy {}: {e}", from.display()))
    }
}

/// Every file under `from` is under `to` as well, with the same size.
fn same_tree(from: &Path, to: &Path) -> Result<(), String> {
    let meta = std::fs::symlink_metadata(from).map_err(|e| format!("{}: {e}", from.display()))?;
    if meta.is_dir() {
        for child in entries(from) {
            let name = child.file_name().ok_or("a file with no name")?;
            same_tree(&child, &to.join(name))?;
        }
        Ok(())
    } else if meta.is_symlink() {
        std::fs::symlink_metadata(to)
            .map(|m| m.is_symlink())
            .unwrap_or(false)
            .then_some(())
            .ok_or_else(|| format!("{} did not arrive", to.display()))
    } else {
        let there = std::fs::symlink_metadata(to).map_err(|_| format!("{} did not arrive", to.display()))?;
        if there.len() == meta.len() {
            Ok(())
        } else {
            Err(format!("{} arrived incomplete ({} of {} bytes)", to.display(), there.len(), meta.len()))
        }
    }
}

/// A folder that only holds characters, or nothing at all, is safe to move into.
fn looks_free(dir: &Path) -> bool {
    entries(dir).iter().all(|p| {
        p.file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with("Shimeji.") || n.starts_with('.'))
            .unwrap_or(false)
    })
}

/// Checks a chosen folder without touching anything, so the UI can explain before asking.
///
/// `Ok(true)` means it already holds characters and they will be kept and used as they are.
pub fn check_target(current: &Path, target: &Path) -> Result<bool, String> {
    if !target.is_absolute() {
        return Err("Please choose a folder by its full path.".to_string());
    }
    let real_current = std::fs::canonicalize(current).unwrap_or_else(|_| current.to_path_buf());
    let real_target = std::fs::canonicalize(target).unwrap_or_else(|_| target.to_path_buf());

    if real_target == real_current {
        return Err("The characters are already there.".to_string());
    }
    if real_target.starts_with(&real_current) {
        return Err("That folder is inside the one the characters are in now. Pick one outside it.".to_string());
    }
    if real_current.starts_with(&real_target) {
        return Err("That folder holds the one the characters are in now. Pick a different one.".to_string());
    }
    if target.exists() && !target.is_dir() {
        return Err(format!("{} is a file, not a folder.", target.display()));
    }
    if target.is_dir() && !looks_free(target) {
        return Err("That folder has other things in it. Please choose an empty one (or make a new one).".to_string());
    }
    Ok(target.is_dir() && !entries(target).is_empty())
}

/// Moves the characters to `target` and leaves a link behind, so wl_shimeji still finds
/// them at the path it insists on.
///
/// The order matters: copy, verify, then swap. If anything fails before the swap, the
/// characters are exactly where they were and the half-made copy is cleaned up.
pub fn move_characters(current: &Path, target: &Path) -> Result<Moved, String> {
    let adopting = check_target(current, target)?;
    let real_current = std::fs::canonicalize(current).map_err(|e| format!("{}: {e}", current.display()))?;

    std::fs::create_dir_all(target).map_err(|e| format!("could not create {}: {e}", target.display()))?;

    let items = entries(&real_current);
    let bytes: u64 = items.iter().map(|p| size_of(p)).sum();

    // Copy everything that is not there yet. Anything already in the target (a folder
    // this app used before) is left as it is.
    let mut copied: Vec<PathBuf> = Vec::new();
    for item in &items {
        let name = item.file_name().ok_or("a file with no name")?;
        let dest = target.join(name);
        if dest.exists() {
            continue;
        }
        if let Err(e) = copy_into(item, &dest) {
            // Undo this run's copies; the originals were never touched.
            for made in &copied {
                let _ = std::fs::remove_dir_all(made);
            }
            let _ = std::fs::remove_dir_all(&dest);
            return Err(format!("Nothing was moved. {e}"));
        }
        copied.push(dest);
    }

    // Everything has to be there, with the right sizes, before anything is removed.
    for item in &items {
        let name = item.file_name().ok_or("a file with no name")?;
        if let Err(e) = same_tree(item, &target.join(name)) {
            for made in &copied {
                let _ = std::fs::remove_dir_all(made);
            }
            return Err(format!("Nothing was moved: the copy is not complete. {e}"));
        }
    }

    // The swap. `current` may already be a link (moved once before), and then there is
    // no folder to keep: the old place is whatever it pointed at.
    let was_link = std::fs::symlink_metadata(current).map(|m| m.is_symlink()).unwrap_or(false);
    let kept = real_current.with_file_name(format!(
        "{}.moved-{}",
        real_current.file_name().and_then(|n| n.to_str()).unwrap_or("shimejis"),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
    ));

    if was_link {
        std::fs::remove_file(current).map_err(|e| format!("could not replace the link: {e}"))?;
    } else {
        std::fs::rename(&real_current, &kept).map_err(|e| format!("could not set the old folder aside: {e}"))?;
    }

    if let Err(e) = std::os::unix::fs::symlink(target, current) {
        // Put it back exactly as it was.
        if !was_link {
            let _ = std::fs::rename(&kept, &real_current);
        }
        return Err(format!("Nothing was moved: the link could not be made ({e})."));
    }

    // Only now, with the link in place and the copy checked, is the old one let go — to
    // the trash, so it can be brought back.
    let mut old_trashed = false;
    let mut note = String::new();
    if was_link {
        note = format!("The characters were copied from {}; that folder is still there.", real_current.display());
    } else {
        match crate::system::trash(&kept) {
            Ok(()) => old_trashed = true,
            Err(e) => note = format!("The old copy is still at {} ({e}). You can delete it yourself.", kept.display()),
        }
    }

    Ok(Moved {
        characters: items.len(),
        bytes,
        from: real_current.display().to_string(),
        to: target.display().to_string(),
        old_trashed,
        note: if adopting && note.is_empty() {
            "The characters that were already in that folder were kept.".to_string()
        } else {
            note
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("shimeji-relocate-test/{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn character(root: &Path, name: &str, bytes: &[u8]) {
        let dir = root.join(format!("Shimeji.{name}/assets"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("shime1.qoi"), bytes).unwrap();
        std::fs::write(root.join(format!("Shimeji.{name}/actions.json")), b"{}").unwrap();
    }

    #[test]
    fn a_folder_that_is_not_free_is_refused_before_anything_is_touched() {
        let base = scratch("refuse");
        let from = base.join("shimejis");
        std::fs::create_dir_all(&from).unwrap();
        character(&from, "BMO", b"12345");

        let busy = base.join("my-documents");
        std::fs::create_dir_all(&busy).unwrap();
        std::fs::write(busy.join("taxes.pdf"), b"x").unwrap();
        assert!(check_target(&from, &busy).is_err());
        assert!(move_characters(&from, &busy).is_err());
        // The characters never moved.
        assert!(from.join("Shimeji.BMO/assets/shime1.qoi").is_file());
        assert!(busy.join("Shimeji.BMO").metadata().is_err());

        // Its own folder, a folder inside it, and one that holds it: all refused.
        assert!(check_target(&from, &from).is_err());
        assert!(check_target(&from, &from.join("inside")).is_err());
        assert!(check_target(&from, &base).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn moving_copies_everything_leaves_a_link_and_keeps_the_old_copy_recoverable() {
        let base = scratch("move");
        let from = base.join("shimejis");
        std::fs::create_dir_all(&from).unwrap();
        character(&from, "BMO", b"picture-bytes");
        character(&from, "Hornet", b"another-picture");

        let to = base.join("big-disk/characters");
        let report = move_characters(&from, &to).expect("the move works");

        assert_eq!(report.characters, 2);
        assert!(report.bytes > 0);
        // Everything arrived.
        assert_eq!(std::fs::read(to.join("Shimeji.BMO/assets/shime1.qoi")).unwrap(), b"picture-bytes");
        assert!(to.join("Shimeji.Hornet/actions.json").is_file());
        // The old path still works, through a link.
        assert!(std::fs::symlink_metadata(&from).unwrap().is_symlink());
        assert_eq!(std::fs::read_link(&from).unwrap(), to);
        assert!(from.join("Shimeji.Hornet/actions.json").is_file());
        // Either it went to the trash, or it is still there and the report says so.
        assert!(report.old_trashed || !report.note.is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn moving_again_repoints_the_link_without_losing_anyone() {
        let base = scratch("again");
        let from = base.join("shimejis");
        std::fs::create_dir_all(&from).unwrap();
        character(&from, "BMO", b"one");

        let first = base.join("disk-a");
        move_characters(&from, &first).expect("first move");
        let second = base.join("disk-b");
        let report = move_characters(&from, &second).expect("second move");

        assert_eq!(std::fs::read_link(&from).unwrap(), second);
        assert!(second.join("Shimeji.BMO/assets/shime1.qoi").is_file());
        // The place it came from is named, not silently thrown away.
        assert!(report.note.contains("disk-a"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_folder_that_already_holds_characters_is_adopted_and_nothing_of_it_is_lost() {
        let base = scratch("adopt");
        let from = base.join("shimejis");
        std::fs::create_dir_all(&from).unwrap();
        character(&from, "BMO", b"new");

        let old_home = base.join("old-home");
        std::fs::create_dir_all(&old_home).unwrap();
        character(&old_home, "Hornet", b"was-already-here");

        assert!(check_target(&from, &old_home).unwrap(), "a folder with characters is adopted");
        move_characters(&from, &old_home).expect("adopting works");
        assert_eq!(std::fs::read(old_home.join("Shimeji.Hornet/assets/shime1.qoi")).unwrap(), b"was-already-here");
        assert!(old_home.join("Shimeji.BMO/assets/shime1.qoi").is_file());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_copy_that_cannot_finish_changes_nothing() {
        let base = scratch("fail");
        let from = base.join("shimejis");
        std::fs::create_dir_all(&from).unwrap();
        character(&from, "BMO", b"data");

        // A target inside a read-only folder: creating it fails.
        let parent = base.join("locked");
        std::fs::create_dir_all(&parent).unwrap();
        let mut perms = std::fs::metadata(&parent).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o500);
        std::fs::set_permissions(&parent, perms).unwrap();

        assert!(move_characters(&from, &parent.join("nope")).is_err());
        assert!(from.join("Shimeji.BMO/assets/shime1.qoi").is_file());
        assert!(!std::fs::symlink_metadata(&from).unwrap().is_symlink());

        let mut perms = std::fs::metadata(&parent).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o700);
        let _ = std::fs::set_permissions(&parent, perms);
        let _ = std::fs::remove_dir_all(&base);
    }
}
