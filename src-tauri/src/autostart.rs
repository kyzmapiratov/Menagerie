// Starting the characters at login, on any desktop.
//
// Niri gets its own treatment (a config file with key bindings; see `niri.rs`), because
// it can do more. Everywhere else there is one thing every desktop agrees on: a `.desktop`
// file in `~/.config/autostart`, which KDE, GNOME, Xfce, Cinnamon, LXQt and the session
// managers that wlroots compositors use all read. So instead of handing the person a
// script and wishing them luck, the app writes that file and can take it away again.
//
// Only this app's own file is ever written or removed, and it carries a marker saying so.

use std::path::PathBuf;

/// The one file this module owns.
const FILE: &str = "menagerie-characters.desktop";
const MARKER: &str = "X-Menagerie=characters";

#[derive(serde::Serialize, Debug, Default)]
pub struct Status {
    pub active: bool,
    /// Where the file is (or would be).
    pub file: String,
    /// What it runs, for the person to read before turning it on.
    pub command: String,
}

fn dir() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(".config"));
    base.join("autostart")
}

fn path() -> PathBuf {
    dir().join(FILE)
}

/// A shell script as one `Exec=` line: the lines become `;`-separated commands.
///
/// `Exec` is not a shell command line — it is split on spaces by the desktop — so the
/// script is handed to `sh -c` as a single argument. Quotes inside it are doubled the way
/// the desktop entry specification asks for.
pub fn exec_line(script: &str) -> String {
    let body: Vec<&str> = script
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    // "cmd &" starts something in the background and is already a command separator:
    // another ";" after it is a syntax error, and the whole line would do nothing.
    let mut joined = String::new();
    for line in &body {
        if !joined.is_empty() {
            joined.push_str(if joined.ends_with('&') { " " } else { "; " });
        }
        joined.push_str(line);
    }

    // Two layers read this back: the desktop file's own string escapes (\\ means one
    // backslash), and then the shell-style split of the Exec value. So each backslash and
    // quote is escaped for both.
    let escaped = joined.replace('\\', "\\\\\\\\").replace('"', "\\\\\"");
    format!("sh -c \"{escaped}\"")
}

pub fn status() -> Status {
    let file = path();
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    Status {
        active: text.contains(MARKER),
        command: text
            .lines()
            .find_map(|l| l.strip_prefix("Exec="))
            .unwrap_or_default()
            .to_string(),
        file: file.display().to_string(),
    }
}

/// Writes the file (replacing this app's own, never anything else).
pub fn enable(script: &str) -> Result<Status, String> {
    let file = path();
    if file.exists() && !std::fs::read_to_string(&file).unwrap_or_default().contains(MARKER) {
        return Err(format!("{} is somebody else's file, so it was left alone.", file.display()));
    }
    std::fs::create_dir_all(dir()).map_err(|e| format!("could not create {}: {e}", dir().display()))?;

    let body = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Shimeji characters\n\
         Comment=Brings your characters back when you log in\n\
         Exec={}\n\
         Icon=menagerie\n\
         Terminal=false\n\
         NoDisplay=true\n\
         X-GNOME-Autostart-enabled=true\n\
         {MARKER}\n",
        exec_line(script)
    );
    // Written whole and moved into place, so a half-written file never gets run.
    let tmp = file.with_extension("desktop.tmp");
    std::fs::write(&tmp, body).map_err(|e| format!("could not write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &file).map_err(|e| format!("could not save {}: {e}", file.display()))?;
    Ok(status())
}

/// Removes it, if it is ours. A file that is not there is not an error.
pub fn disable() -> Result<Status, String> {
    let file = path();
    match std::fs::read_to_string(&file) {
        Ok(text) if text.contains(MARKER) => {
            std::fs::remove_file(&file).map_err(|e| format!("could not remove {}: {e}", file.display()))?;
        }
        Ok(_) => return Err(format!("{} was not written by this app, so it was left alone.", file.display())),
        Err(_) => {}
    }
    Ok(status())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each test gets its own XDG_CONFIG_HOME, so nothing of the person's is touched.
    fn with_config_home<T>(name: &str, body: impl FnOnce(&PathBuf) -> T) -> T {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let home = std::env::temp_dir().join(format!("shimeji-autostart-test/{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        let saved = std::env::var_os("XDG_CONFIG_HOME");
        std::env::set_var("XDG_CONFIG_HOME", &home);
        let out = body(&home);
        match saved {
            Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
        let _ = std::fs::remove_dir_all(&home);
        out
    }

    #[test]
    fn a_script_becomes_one_exec_line_a_desktop_can_run() {
        let line = exec_line("#!/bin/sh\n# a comment\nshimeji-overlayd &\nsleep 3\nshimejictl summon \"Biscuit Deer\"\n");
        assert!(line.starts_with("sh -c \""), "{line}");
        assert!(line.contains("shimeji-overlayd & sleep 3"), "a background command is its own separator: {line}");
        assert!(!line.contains("&;"), "that would be a shell syntax error: {line}");
        // The quotes around a name with a space survive, escaped for the desktop file.
        assert!(line.contains("\\\\\"Biscuit Deer\\\\\""), "{line}");
        // One line: a newline in Exec would break the file.
        assert!(!line.contains('\n'));
    }

    #[test]
    fn turning_it_on_writes_a_file_and_turning_it_off_takes_it_away() {
        with_config_home("on-off", |home| {
            assert!(!status().active);
            let after = enable("shimeji-overlayd &\nsleep 2\nshimejictl summon 'BMO'").unwrap();
            assert!(after.active);
            let file = home.join("autostart").join(FILE);
            let text = std::fs::read_to_string(&file).unwrap();
            assert!(text.starts_with("[Desktop Entry]"));
            assert!(text.contains("Exec=sh -c \""));
            assert!(text.contains(MARKER));
            assert!(status().active);

            assert!(!disable().unwrap().active);
            assert!(!file.exists());
            // Removing it twice is not an error.
            assert!(disable().is_ok());
        });
    }

    #[test]
    fn somebody_elses_autostart_file_is_never_written_over_or_removed() {
        with_config_home("foreign", |home| {
            let file = home.join("autostart").join(FILE);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, "[Desktop Entry]\nExec=something-else\n").unwrap();

            assert!(enable("shimeji-overlayd &").is_err());
            assert!(disable().is_err());
            assert_eq!(std::fs::read_to_string(&file).unwrap(), "[Desktop Entry]\nExec=something-else\n");
        });
    }
}
