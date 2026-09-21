// Small desktop helpers: show a folder in the file manager, move a file to the trash,
// find programs, and say what kind of system this is.
//
// Nothing here is specific to one desktop or one distribution. The tools are the ones
// every Linux desktop has (D-Bus, `gio`), with fallbacks for the ones that lack them.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Makes sure the usual places for user-installed programs are searched.
///
/// An app started from a launcher does not always get the `PATH` of a terminal: on many
/// systems `~/.local/bin` is only added by the shell's profile. `wl_shimeji` installed
/// there (which is where `install.sh` puts it) would then be "not installed" for this
/// app, and for the overlay it starts, even though it works fine in a terminal. Folders
/// are appended, so anything the user put first still wins.
pub fn extend_path() {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    let mut wanted: Vec<PathBuf> = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        wanted.push(Path::new(&home).join(".local/bin"));
    }
    wanted.extend(["/usr/local/bin", "/usr/bin"].map(PathBuf::from));
    for dir in wanted {
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    if let Ok(joined) = std::env::join_paths(dirs) {
        std::env::set_var("PATH", joined);
    }
}

/// A private place for scratch files (converted archives, exports, probes): `$XDG_RUNTIME_DIR/menagerie`, or
/// `<tmp>/menagerie-<uid>` where there is no runtime folder, made for this user alone (mode 0700).
///
/// It used to be `<tmp>/menagerie`, one name for everybody. On a machine with two users the second one found the
/// first one's folder there, and every install ended in "Permission denied (os error 13)". A fixed, predictable
/// name in a folder everybody may write to is also how someone plants a link where your files are about to go.
pub fn scratch_dir() -> PathBuf {
    scratch_in(std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from), std::env::temp_dir(), our_uid())
}

fn our_uid() -> u32 {
    unsafe extern "C" {
        fn getuid() -> u32;
    }
    unsafe { getuid() }
}

fn scratch_in(runtime: Option<PathBuf>, tmp: PathBuf, uid: u32) -> PathBuf {
    let runtime = runtime.filter(|p| p.is_absolute()).map(|p| p.join("menagerie"));
    for dir in runtime.into_iter().chain([tmp.join(format!("menagerie-{uid}"))]) {
        if make_private(&dir, uid) {
            return dir;
        }
    }
    // Somebody else holds both names: a folder of our own, named for this run.
    let own = tmp.join(format!("menagerie-{uid}-{}", std::process::id()));
    let _ = make_private(&own, uid);
    own
}

/// Makes `dir` if it is missing, and says whether it is a real folder (not a link) that is ours and closed to others.
fn make_private(dir: &Path, uid: u32) -> bool {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    let _ = std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir);
    match std::fs::symlink_metadata(dir) {
        Ok(m) if m.is_dir() && m.uid() == uid => {
            // One that was left open (made by an older build, or by hand) is closed rather than refused.
            m.permissions().mode() & 0o077 == 0 || std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).is_ok()
        }
        _ => false,
    }
}

/// Where a program is, the way a shell would look it up (None: it is not installed).
pub fn which(program: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|p| p.metadata().map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false))
}

/// The packaging family of this system (`arch`, `fedora`, `debian`, `suse`, `nix`), or
/// `unknown`. Read from `/etc/os-release`, which every current distribution has, and
/// used only to choose which installation hint to show.
pub fn distro_family() -> &'static str {
    let text = std::fs::read_to_string("/etc/os-release").or_else(|_| std::fs::read_to_string("/usr/lib/os-release")).unwrap_or_default();
    let field = |key: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(key).and_then(|v| v.strip_prefix('=')))
            .map(|v| v.trim().trim_matches('"').to_lowercase())
            .unwrap_or_default()
    };
    family_of(&field("ID"), &field("ID_LIKE"))
}

/// `ID` and `ID_LIKE` from os-release to a family. Derivatives usually name their parent
/// in `ID_LIKE` (CachyOS, EndeavourOS, Manjaro → arch; Ubuntu, Mint, Pop!_OS → debian),
/// so the ones that do not are listed by name.
pub fn family_of(id: &str, like: &str) -> &'static str {
    let of = |name: &str| match name {
        "arch" | "cachyos" | "endeavouros" | "manjaro" | "garuda" | "artix" | "arcolinux" => Some("arch"),
        "fedora" | "rhel" | "centos" | "rocky" | "almalinux" | "nobara" | "bazzite" => Some("fedora"),
        "debian" | "ubuntu" | "linuxmint" | "pop" | "elementary" | "zorin" | "raspbian" | "kali" | "neon" => Some("debian"),
        "suse" | "sles" | "sled" => Some("suse"),
        n if n.starts_with("opensuse") => Some("suse"),
        "nixos" => Some("nix"),
        _ => None,
    };
    of(id).or_else(|| like.split_whitespace().find_map(of)).unwrap_or("unknown")
}

/// `/home/me/My Music` → `file:///home/me/My%20Music`. Only what a URI may hold
/// unescaped is left as it is, so any name (spaces, `#`, `%`, non-ASCII) survives.
pub fn file_uri(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut out = String::from("file://");
    for &b in path.as_os_str().as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Runs a short command and reports whether it succeeded.
fn ok(cmd: &mut Command) -> bool {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Starts a program that keeps running (a file manager) and does not wait for it.
fn launch(program: &str, arg: &Path) -> bool {
    let mut cmd = Command::new(program);
    cmd.env_remove("PYTHONHOME").env_remove("PYTHONPATH");
    match cmd
        .arg(arg)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(mut child) => {
            // Reap it when it exits, so it does not linger as a zombie.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            true
        }
        Err(_) => false,
    }
}

/// Shows a folder in the file manager.
///
/// `xdg-open` on a folder opens whatever the user's system says is the default
/// program for folders, and on some systems that is a code editor. So the file
/// manager is asked directly, through the standard `org.freedesktop.FileManager1`
/// D-Bus service (Nautilus, Dolphin, Thunar, Nemo and PCManFM-Qt provide it).
/// If that is not available, the well-known file managers are tried by name, and
/// only then `xdg-open`.
pub fn open_folder(path: &Path) -> Result<(), String> {
    if !path.is_dir() {
        return Err(format!("{} is not a folder", path.display()));
    }
    let uri = file_uri(path);

    if ok(Command::new("gdbus").args([
        "call",
        "--session",
        "--dest",
        "org.freedesktop.FileManager1",
        "--object-path",
        "/org/freedesktop/FileManager1",
        "--method",
        "org.freedesktop.FileManager1.ShowFolders",
        &format!("['{uri}']"),
        "",
    ])) {
        return Ok(());
    }
    if ok(Command::new("dbus-send").args([
        "--session",
        "--print-reply",
        "--dest=org.freedesktop.FileManager1",
        "/org/freedesktop/FileManager1",
        "org.freedesktop.FileManager1.ShowFolders",
        &format!("array:string:{uri}"),
        "string:",
    ])) {
        return Ok(());
    }
    for fm in ["nautilus", "dolphin", "thunar", "nemo", "pcmanfm-qt", "pcmanfm", "caja", "krusader"] {
        if launch(fm, path) {
            return Ok(());
        }
    }
    if launch("xdg-open", path) {
        return Ok(());
    }
    Err(format!("no file manager found to open {}", path.display()))
}

/// Moves a file or a folder to the trash (recoverable), never deletes it outright.
pub fn trash(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("{} is not there", path.display()));
    }
    let out = Command::new("gio")
        .arg("trash")
        .arg("--")
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .map_err(|_| "there is no trash available here (`gio` is missing), so the file was kept".to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "could not move it to the trash: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

#[cfg(test)]
mod tests {

    fn scratch_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("menagerie-scratch-test/{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn mode_of(p: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn scratch_lives_in_the_runtime_folder_and_is_closed_to_others() {
        let (runtime, tmp) = (scratch_root("rt"), scratch_root("rt-tmp"));
        let dir = scratch_in(Some(runtime.clone()), tmp, our_uid());
        assert_eq!(dir, runtime.join("menagerie"));
        assert!(dir.is_dir());
        assert_eq!(mode_of(&dir), 0o700);
    }

    #[test]
    fn without_a_runtime_folder_the_scratch_is_named_for_the_user_not_shared() {
        let tmp = scratch_root("nort");
        let dir = scratch_in(None, tmp.clone(), our_uid());
        assert_eq!(dir, tmp.join(format!("menagerie-{}", our_uid())));
        assert_eq!(mode_of(&dir), 0o700);
        // A runtime folder that is not an absolute path is not trusted either.
        assert_eq!(scratch_in(Some(PathBuf::from("relative/run")), tmp.clone(), our_uid()), dir);
    }

    #[test]
    fn a_folder_that_belongs_to_somebody_else_is_never_used() {
        // Pretend to be another user: the folder is ours, so to "uid + 1" it belongs to a stranger.
        let tmp = scratch_root("stranger");
        let mine = our_uid();
        let stranger = mine + 1;
        std::fs::create_dir_all(tmp.join(format!("menagerie-{stranger}"))).unwrap();
        let dir = scratch_in(None, tmp.clone(), stranger);
        assert_ne!(dir, tmp.join(format!("menagerie-{stranger}")), "the stranger's folder must be refused");
        assert_eq!(dir, tmp.join(format!("menagerie-{stranger}-{}", std::process::id())), "and a folder of our own used instead");
    }

    #[test]
    fn a_link_where_the_folder_should_be_is_refused() {
        let tmp = scratch_root("link");
        let elsewhere = tmp.join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::os::unix::fs::symlink(&elsewhere, tmp.join(format!("menagerie-{}", our_uid()))).unwrap();
        let dir = scratch_in(None, tmp.clone(), our_uid());
        assert_ne!(dir, tmp.join(format!("menagerie-{}", our_uid())), "a link could send our files anywhere");
        assert!(std::fs::symlink_metadata(&dir).unwrap().is_dir());
    }

    #[test]
    fn a_folder_left_open_is_closed_and_used() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = scratch_root("open");
        let old = tmp.join(format!("menagerie-{}", our_uid()));
        std::fs::create_dir_all(&old).unwrap();
        std::fs::set_permissions(&old, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(scratch_in(None, tmp, our_uid()), old);
        assert_eq!(mode_of(&old), 0o700);
    }
    use super::*;

    #[test]
    fn uris_escape_everything_a_path_can_hold() {
        assert_eq!(file_uri(Path::new("/home/me/Downloads")), "file:///home/me/Downloads");
        assert_eq!(file_uri(Path::new("/home/me/My Music")), "file:///home/me/My%20Music");
        assert_eq!(file_uri(Path::new("/tmp/a#b%c'd")), "file:///tmp/a%23b%25c%27d");
        // Non-ASCII goes out as UTF-8 bytes.
        assert_eq!(file_uri(Path::new("/tmp/Завантаження")), "file:///tmp/%D0%97%D0%B0%D0%B2%D0%B0%D0%BD%D1%82%D0%B0%D0%B6%D0%B5%D0%BD%D0%BD%D1%8F");
    }

    #[test]
    fn distributions_are_told_apart_by_their_own_name_or_their_parent() {
        assert_eq!(family_of("arch", ""), "arch");
        assert_eq!(family_of("cachyos", "arch"), "arch");
        assert_eq!(family_of("endeavouros", "arch"), "arch");
        assert_eq!(family_of("manjaro", "arch"), "arch");
        assert_eq!(family_of("fedora", ""), "fedora");
        assert_eq!(family_of("nobara", "rhel centos fedora"), "fedora");
        assert_eq!(family_of("rocky", "rhel centos fedora"), "fedora");
        assert_eq!(family_of("debian", ""), "debian");
        assert_eq!(family_of("ubuntu", "debian"), "debian");
        assert_eq!(family_of("linuxmint", "ubuntu debian"), "debian");
        // Unknown name, known parent.
        assert_eq!(family_of("someremix", "ubuntu debian"), "debian");
        assert_eq!(family_of("opensuse-tumbleweed", "opensuse suse"), "suse");
        assert_eq!(family_of("opensuse-leap", "suse opensuse"), "suse");
        assert_eq!(family_of("nixos", ""), "nix");
        assert_eq!(family_of("void", ""), "unknown");
        assert_eq!(family_of("", ""), "unknown");
    }

    #[test]
    fn a_program_is_found_only_when_it_is_really_runnable() {
        assert!(which("sh").is_some());
        assert!(which("definitely-not-a-program-here").is_none());
    }

    #[test]
    fn a_missing_folder_or_file_is_refused_before_anything_runs() {
        assert!(open_folder(Path::new("/definitely/not/here")).is_err());
        assert!(trash(Path::new("/definitely/not/here.zip")).is_err());
        assert!(trash(Path::new("/definitely/not/here/")).is_err());
    }
}
