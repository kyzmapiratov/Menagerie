// Niri integration: launch at login and keybinds.
//
// We write our own file (config.d/55-shimeji.kdl if a config.d folder exists,
// otherwise shimeji.kdl next to config.kdl) and hook it in with ONE `include`
// line appended to config.kdl. That line is added only when the user turns the
// feature on, a backup of config.kdl is kept next to it, the result is checked
// with `niri validate`, and if Niri rejects anything everything is rolled back.
// Turning the feature off removes the line and the file again.
//
// What matters in the commands themselves:
//   • parallel shimejictl calls crash the overlay, so at login we summon
//     characters one by one with pauses (a single spawn-sh-at-startup);
//   • `shimejictl dismiss --all` first waits for a mouse selection, so the
//     "dismiss all" key is `timeout -s INT`, which cancels the selection.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Deserialize, Clone, Debug)]
pub struct Bind {
    pub keys: String,
    /// dismiss_all | stop | summon
    pub action: String,
    /// For summon: the character name.
    #[serde(default)]
    pub arg: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Options {
    pub overlay: bool,
    /// Pause before the first summon, in seconds.
    pub delay: f32,
    pub mascots: Vec<(String, usize)>,
    /// How many characters, chosen at random from the collection, to add at login.
    #[serde(default)]
    pub random: usize,
    pub binds: Vec<Bind>,
}

#[derive(Serialize, Clone, Debug)]
pub struct Status {
    /// Our file exists AND config.kdl includes it: Niri is using it.
    pub active: bool,
    pub file: String,
    /// The line that hooks the file into config.kdl.
    pub include_line: String,
    /// Where the backup of config.kdl goes when the feature is turned on.
    pub backup: String,
    /// `niri validate` of the main config while active ("ok" or Niri's message).
    pub validation: String,
}

/// The locations involved, so tests can point at a temporary folder.
#[derive(Clone, Debug)]
struct Paths {
    dir: PathBuf,
}

impl Paths {
    fn real() -> Self {
        let base = std::env::var("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "~".to_string())).join(".config")
        });
        Self { dir: base.join("niri") }
    }

    fn main(&self) -> PathBuf {
        self.dir.join("config.kdl")
    }

    fn backup(&self) -> PathBuf {
        self.dir.join("config.kdl.shimeji-backup")
    }

    /// (file to write, its path relative to config.kdl for `include`)
    fn target(&self) -> (PathBuf, String) {
        if self.dir.join("config.d").is_dir() {
            (self.dir.join("config.d/55-shimeji.kdl"), "config.d/55-shimeji.kdl".to_string())
        } else {
            (self.dir.join("shimeji.kdl"), "shimeji.kdl".to_string())
        }
    }

    fn include_line(&self) -> String {
        format!("include \"{}\"", self.target().1)
    }
}

/// The comment we put above the include line, so it is clear who added it.
const MARK: &str = "// Menagerie: launch at login and keybinds (remove this line and the next to disable)";

fn is_our_comment(line: &str) -> bool {
    let line = line.trim();
    line == MARK
}

/// A string in single quotes for sh.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// A string for KDL (in double quotes).
fn kdl(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn secs(x: f32) -> String {
    let s = format!("{:.1}", x.max(0.0));
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// The commands that bring the characters back at login, in order, or None when
/// there is nothing to summon. Shared by the Niri file and the generic snippet.
pub fn login_commands(o: &Options) -> Option<Vec<String>> {
    let list: Vec<&str> = o
        .mascots
        .iter()
        .flat_map(|(n, c)| std::iter::repeat_n(n.as_str(), (*c).max(1)))
        .collect();
    if list.is_empty() && o.random == 0 {
        return None;
    }

    // Summon one by one with a pause; parallel calls crash the overlay.
    let mut parts = vec![format!("sleep {}", secs(o.delay))];
    if o.random > 0 {
        // Chosen anew at every login: the installed names, without the helper
        // prototypes (they start with a dot), `o.random` of them shuffled.
        parts.push(format!(
            "shimejictl prototypes list | sed -n 's/^[0-9]*: //p' | grep -v '^\\.' | shuf -n {} | while IFS= read -r n; do shimejictl summon \"$n\"; sleep 0.4; done",
            o.random.min(60)
        ));
    }
    for (i, name) in list.iter().enumerate() {
        if i > 0 || o.random > 0 {
            parts.push("sleep 0.4".into());
        }
        parts.push(format!("shimejictl summon {}", sh_quote(name)));
    }
    Some(parts)
}

/// The same thing for any other desktop: a plain shell script to put in its
/// autostart, whatever it calls that (Startup Applications, an `exec` line in a
/// compositor's config, a systemd user unit...).
/// Whether this really is a niri session with niri available to run.
///
/// Two things have to be true: the session says it is niri (its own variable, or the
/// desktop name), and the command exists. Either on its own gives the wrong answer —
/// many compositors leave `XDG_CURRENT_DESKTOP` empty, and niri may be installed on a
/// machine that is running something else right now.
pub fn is_niri_session() -> bool {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_lowercase();
    let looks_like_niri = desktop.split(':').any(|d| d == "niri") || std::env::var_os("NIRI_SOCKET").is_some();
    looks_like_niri && niri_command_exists()
}

/// Whether the `niri` command can be run at all.
fn niri_command_exists() -> bool {
    std::process::Command::new("niri")
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn shell_snippet(o: &Options) -> String {
    let mut lines = vec![
        "#!/bin/sh".to_string(),
        "# Menagerie: bring the characters back at login. Add this to your desktop's autostart.".to_string(),
    ];
    if o.overlay {
        lines.push("shimeji-overlayd &".to_string());
    }
    match login_commands(o) {
        Some(parts) => lines.extend(parts),
        None => lines.push("# (nobody is chosen to appear yet)".to_string()),
    }
    lines.join("\n") + "\n"
}

pub fn render(o: &Options) -> String {
    let mut out: Vec<String> = vec!["// Menagerie".into()];

    if o.overlay {
        out.push(r#"spawn-at-startup "shimeji-overlayd""#.into());
    }

    if let Some(parts) = login_commands(o) {
        out.push(format!("spawn-sh-at-startup {}", kdl(&parts.join("; "))));
    }

    let binds: Vec<String> = o
        .binds
        .iter()
        .filter(|b| !b.keys.trim().is_empty())
        .filter_map(|b| match b.action.as_str() {
            "dismiss_all" => Some(format!(
                "    {} {{ spawn-sh {}; }}",
                b.keys.trim(),
                kdl("timeout -s INT 0.7 shimejictl mascot dismiss --all")
            )),
            "stop" => Some(format!(r#"    {} {{ spawn "shimejictl" "stop"; }}"#, b.keys.trim())),
            "summon" if !b.arg.is_empty() => Some(format!(
                r#"    {} {{ spawn "shimejictl" "summon" {}; }}"#,
                b.keys.trim(),
                kdl(&b.arg)
            )),
            _ => None,
        })
        .collect();
    if !binds.is_empty() {
        out.push("binds {".into());
        out.extend(binds);
        out.push("}".into());
    }

    out.join("\n") + "\n"
}

fn include_lines(text: &str, rel: &str) -> Vec<usize> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| {
            let t = l.trim_start();
            !t.starts_with("//") && t.starts_with("include") && l.contains(rel)
        })
        .map(|(i, _)| i)
        .collect()
}

fn is_included(p: &Paths) -> bool {
    let (_, rel) = p.target();
    std::fs::read_to_string(p.main()).map(|t| !include_lines(&t, &rel).is_empty()).unwrap_or(false)
}

fn validate(path: &Path) -> String {
    match std::process::Command::new("niri").arg("validate").arg("-c").arg(path).output() {
        Ok(o) if o.status.success() => "ok".to_string(),
        Ok(o) => {
            let text = String::from_utf8_lossy(&o.stderr).to_string()
                + &String::from_utf8_lossy(&o.stdout);
            // Strip ANSI colors.
            let mut clean = String::new();
            let mut skip = false;
            for c in text.chars() {
                match (skip, c) {
                    (false, '\u{1b}') => skip = true,
                    (true, 'm') => skip = false,
                    (false, c) => clean.push(c),
                    _ => {}
                }
            }
            clean.trim().to_string()
        }
        Err(e) => format!("niri validate failed to start: {e}"),
    }
}

fn status_of(p: &Paths) -> Status {
    let (file, _) = p.target();
    let included = is_included(p);
    let active = included && file.exists();
    Status {
        active,
        file: file.display().to_string(),
        include_line: p.include_line(),
        backup: p.backup().display().to_string(),
        validation: if active { validate(&p.main()) } else { String::new() },
    }
}

/// Syntax check of the generated text on its own, in a scratch folder.
fn check_alone(text: &str) -> Result<(), String> {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = crate::system::scratch_dir().join(format!("niri-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let result = (|| {
        std::fs::write(tmp.join("shimeji.kdl"), text).map_err(|e| e.to_string())?;
        std::fs::write(tmp.join("config.kdl"), "include \"shimeji.kdl\"\n").map_err(|e| e.to_string())?;
        match validate(&tmp.join("config.kdl")) {
            v if v == "ok" => Ok(()),
            v => Err(format!("Niri rejected the config:\n{v}")),
        }
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

fn write_file(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("could not write {}: {e}", path.display()))
}

/// Puts `previous` back (or deletes the file if there was nothing before).
fn restore(path: &Path, previous: &Option<String>) {
    match previous {
        Some(text) => {
            let _ = std::fs::write(path, text);
        }
        None => {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn apply_at(p: &Paths, o: &Options) -> Result<Status, String> {
    let text = render(o);
    check_alone(&text)?;

    let (file, _) = p.target();
    let previous = std::fs::read_to_string(&file).ok();
    write_file(&file, &text)?;

    // The real config may reject what the standalone check accepted, for example
    // a key that one of the user's own binds already uses.
    if is_included(p) {
        let check = validate(&p.main());
        if check != "ok" {
            restore(&file, &previous);
            return Err(format!("Niri rejected the config:\n{check}"));
        }
    }
    Ok(status_of(p))
}

fn enable_at(p: &Paths, o: &Options) -> Result<Status, String> {
    let main = p.main();
    let original = std::fs::read_to_string(&main)
        .map_err(|e| format!("could not read {}: {e}", main.display()))?;

    let text = render(o);
    check_alone(&text)?;

    let (file, rel) = p.target();
    let previous_file = std::fs::read_to_string(&file).ok();
    write_file(&file, &text)?;

    if include_lines(&original, &rel).is_empty() {
        // Keep a copy of the untouched config.kdl first.
        std::fs::write(p.backup(), &original)
            .map_err(|e| format!("could not write the backup {}: {e}", p.backup().display()))?;

        let mut updated = original.clone();
        if !updated.is_empty() && !updated.ends_with('\n') {
            updated.push('\n');
        }
        updated.push_str(&format!("\n{MARK}\n{}\n", p.include_line()));

        if let Err(e) = std::fs::write(&main, &updated) {
            restore(&file, &previous_file);
            return Err(format!("could not update {}: {e}", main.display()));
        }
    }

    let check = validate(&main);
    if check != "ok" {
        let _ = std::fs::write(&main, &original);
        restore(&file, &previous_file);
        return Err(format!("Niri rejected the config, nothing was changed:\n{check}"));
    }
    Ok(status_of(p))
}

fn disable_at(p: &Paths) -> Result<Status, String> {
    let main = p.main();
    let (file, rel) = p.target();

    if let Ok(original) = std::fs::read_to_string(&main) {
        let drop_at = include_lines(&original, &rel);
        if !drop_at.is_empty() {
            let kept: Vec<&str> = original
                .lines()
                .enumerate()
                // Drop the include line and our comment right above it.
                .filter(|(i, l)| !drop_at.contains(i) && !is_our_comment(l))
                .map(|(_, l)| l)
                .collect();
            let mut updated = kept.join("\n");
            // Tidy the blank line we added before the comment.
            while updated.ends_with("\n\n") {
                updated.pop();
            }
            if !updated.ends_with('\n') {
                updated.push('\n');
            }
            std::fs::write(&main, &updated)
                .map_err(|e| format!("could not update {}: {e}", main.display()))?;

            let check = validate(&main);
            if check != "ok" {
                let _ = std::fs::write(&main, &original);
                return Err(format!("Niri rejected the config, nothing was changed:\n{check}"));
            }
        }
    }
    let _ = std::fs::remove_file(&file);
    Ok(status_of(p))
}

/// One of our keys that the user's own Niri config already binds.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Conflict {
    pub keys: String,
    /// File (relative to the Niri config folder) and line of the existing bind.
    pub file: String,
    pub line: usize,
}

/// "Mod+Shift+D" and "shift+mod+d" are the same key: lower-case, modifiers sorted.
fn normalize_key(s: &str) -> String {
    let mut parts: Vec<String> = s.split('+').map(|p| p.trim().to_lowercase()).filter(|p| !p.is_empty()).collect();
    if let Some(last) = parts.pop() {
        parts.sort();
        parts.push(last);
    }
    parts.join("+")
}

fn kdl_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let path = e.path();
        if path.is_dir() {
            kdl_files(&path, out);
        } else if path.extension().map(|x| x == "kdl").unwrap_or(false) {
            out.push(path);
        }
    }
}

/// Niri does NOT reject a key that is bound twice across included files: the
/// later one silently wins. Since our include goes last, a clash would quietly
/// replace one of the user's own binds, so we look for them ourselves.
fn conflicts_at(p: &Paths, keys: &[String]) -> Vec<Conflict> {
    let (ours, _) = p.target();
    let wanted: Vec<(String, String)> = keys
        .iter()
        .filter(|k| !k.trim().is_empty())
        .map(|k| (normalize_key(k), k.trim().to_string()))
        .collect();

    let mut files = Vec::new();
    kdl_files(&p.dir, &mut files);
    files.sort();

    let mut found = Vec::new();
    for file in files.into_iter().filter(|f| *f != ours) {
        let Ok(text) = std::fs::read_to_string(&file) else { continue };
        for (i, line) in text.lines().enumerate() {
            let t = line.trim_start();
            if t.starts_with("//") {
                continue;
            }
            // A bind line is `Keys [property=value ...] { action; }`.
            let Some(head) = t.split(|c: char| c.is_whitespace() || c == '{').next() else { continue };
            let norm = normalize_key(head);
            if let Some((_, original)) = wanted.iter().find(|(n, _)| *n == norm) {
                if t.contains('{') {
                    found.push(Conflict {
                        keys: original.clone(),
                        file: file.strip_prefix(&p.dir).unwrap_or(&file).display().to_string(),
                        line: i + 1,
                    });
                }
            }
        }
    }
    found
}

pub fn conflicts(keys: &[String]) -> Vec<Conflict> {
    conflicts_at(&Paths::real(), keys)
}

pub fn status() -> Status {
    status_of(&Paths::real())
}

/// Rewrites our file with new options (only meaningful while active).
pub fn apply(o: &Options) -> Result<Status, String> {
    apply_at(&Paths::real(), o)
}

/// Turns launch-at-login on: writes our file and adds the include line.
pub fn enable(o: &Options) -> Result<Status, String> {
    enable_at(&Paths::real(), o)
}

/// Turns it off again: removes the include line and our file.
pub fn disable() -> Result<Status, String> {
    disable_at(&Paths::real())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> Options {
        Options {
            overlay: true,
            delay: 3.0,
            mascots: vec![("Bill Cipher".into(), 1), ("O'Neil".into(), 2)],
            random: 0,
            binds: vec![
                Bind { keys: "Mod+Shift+D".into(), action: "dismiss_all".into(), arg: String::new() },
                Bind { keys: "Mod+Shift+S".into(), action: "stop".into(), arg: String::new() },
                Bind { keys: "Mod+Shift+H".into(), action: "summon".into(), arg: "Hornet".into() },
                Bind { keys: String::new(), action: "stop".into(), arg: String::new() },
            ],
        }
    }

    #[test]
    fn renders_sequential_summons_and_binds() {
        let t = render(&opts());
        assert!(t.contains(r#"spawn-at-startup "shimeji-overlayd""#));
        assert!(t.contains("sleep 3; shimejictl summon 'Bill Cipher'; sleep 0.4; shimejictl summon 'O'\\\\''Neil'"));
        assert!(t.contains("timeout -s INT 0.7 shimejictl mascot dismiss --all"));
        // Three keys; the one without a combination is skipped.
        assert_eq!(t.lines().filter(|l| l.starts_with("    Mod+")).count(), 3);
    }

    #[test]
    fn the_generic_snippet_is_a_plain_script_with_the_same_commands() {
        let mut o = opts();
        o.random = 2;
        let t = shell_snippet(&o);
        assert!(t.starts_with("#!/bin/sh\n"), "{t}");
        assert!(t.contains("\nshimeji-overlayd &\n"), "{t}");
        assert!(t.contains("\nsleep 3\n"), "{t}");
        assert!(t.contains("shuf -n 2"), "{t}");
        assert!(t.contains("\nshimejictl summon 'Bill Cipher'\n"), "{t}");
        // No KDL in it.
        assert!(!t.contains("spawn-sh-at-startup"), "{t}");
        // Nobody chosen: a comment, and the script still runs.
        o.mascots.clear();
        o.random = 0;
        assert!(shell_snippet(&o).contains("# (nobody is chosen"));
    }

    #[test]
    fn random_characters_are_picked_at_login_not_when_the_file_is_written() {
        let mut o = opts();
        o.random = 3;
        let t = render(&o);
        // A shell pipeline that shuffles the installed names each time it runs, skipping helper prototypes...
        assert!(t.contains("shimejictl prototypes list | sed -n"), "{t}");
        assert!(t.contains("shuf -n 3"), "{t}");
        assert!(t.contains("grep -v '^\\\\.'"), "{t}");
        // ...that comes before the named ones, with a pause between.
        let at = |needle: &str| t.find(needle).unwrap();
        assert!(at("shuf -n 3") < at("shimejictl summon 'Bill Cipher'"));
        assert!(t.contains("done; sleep 0.4; shimejictl summon 'Bill Cipher'"), "{t}");

        // Random characters alone still make a login file, with no named ones.
        o.mascots.clear();
        assert!(render(&o).contains("shuf -n 3"));
        // And zero means none at all.
        o.random = 0;
        assert!(!render(&o).contains("shuf"));
    }

    /// What we generate must be a valid Niri config (if niri is installed).
    #[test]
    fn generated_config_is_valid_kdl_for_niri() {
        if std::process::Command::new("niri").arg("--help").output().is_err() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("menagerie-niri-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // With the random-characters pipeline too: its quotes live inside a KDL string.
        let mut o = opts();
        o.random = 2;
        std::fs::write(dir.join("shimeji.kdl"), render(&o)).unwrap();
        std::fs::write(dir.join("config.kdl"), "include \"shimeji.kdl\"\n").unwrap();
        let r = validate(&dir.join("config.kdl"));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(r, "ok", "{r}");
    }

    /// A scratch copy of a Niri config folder, deleted on drop.
    struct Sandbox(PathBuf);
    impl Sandbox {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("menagerie-sandbox-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn paths(&self) -> Paths {
            Paths { dir: self.0.clone() }
        }
    }
    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn niri_available() -> bool {
        std::process::Command::new("niri").arg("--help").output().is_ok()
    }

    #[test]
    fn only_our_own_comment_is_recognised_as_ours() {
        assert!(is_our_comment(MARK));
        assert!(is_our_comment(&format!("  {MARK}  ")));
        assert!(!is_our_comment("// my own comment about the shimeji"));
    }

    #[test]
    fn enable_adds_one_include_then_disable_restores_the_config() {
        if !niri_available() {
            return;
        }
        let sb = Sandbox::new("roundtrip");
        std::fs::create_dir_all(sb.0.join("config.d")).unwrap();
        let original = "// my config\nlayout {\n    gaps 8\n}\n";
        std::fs::write(sb.0.join("config.kdl"), original).unwrap();
        let p = sb.paths();

        let s = enable_at(&p, &opts()).expect("enable");
        assert!(s.active, "{s:?}");
        assert_eq!(s.validation, "ok");
        assert_eq!(std::fs::read_to_string(p.backup()).unwrap(), original, "backup must be the untouched config");

        // Turning it on again must not add a second include line.
        enable_at(&p, &opts()).expect("enable twice");
        let text = std::fs::read_to_string(p.main()).unwrap();
        assert_eq!(text.matches("55-shimeji.kdl").count(), 1, "{text}");

        let s = disable_at(&p).expect("disable");
        assert!(!s.active);
        assert!(!p.target().0.exists(), "our file must be removed");
        assert_eq!(std::fs::read_to_string(p.main()).unwrap().trim_end(), original.trim_end());
    }

    #[test]
    fn a_config_niri_rejects_is_left_exactly_as_it_was() {
        if !niri_available() {
            return;
        }
        let sb = Sandbox::new("rollback");
        // Already broken on its own (unclosed block): turning our feature on must not touch it.
        let original = "layout {\n    gaps 8\n";
        std::fs::write(sb.0.join("config.kdl"), original).unwrap();
        let p = sb.paths();

        let err = enable_at(&p, &opts()).expect_err("an invalid config must be rejected");
        assert!(err.contains("rejected"), "{err}");
        assert_eq!(std::fs::read_to_string(p.main()).unwrap(), original, "config.kdl must be untouched");
        assert!(!p.target().0.exists(), "our file must not be left behind");
    }

    #[test]
    fn keys_already_used_in_the_users_config_are_reported() {
        let sb = Sandbox::new("conflicts");
        std::fs::create_dir_all(sb.0.join("config.d")).unwrap();
        std::fs::write(sb.0.join("config.kdl"), "// Mod+Shift+D { in a comment }\n").unwrap();
        std::fs::write(
            sb.0.join("config.d/70-binds.kdl"),
            "binds {\n    Mod+Shift+S { spawn \"x\"; }\n    shift+MOD+h repeat=false { focus-column-left; }\n    Mod+Q { close-window; }\n}\n",
        )
        .unwrap();
        let p = sb.paths();

        let keys = vec!["Mod+Shift+D".to_string(), "Mod+Shift+S".to_string(), "Mod+Shift+H".to_string(), "Mod+Ctrl+D".to_string()];
        let found = conflicts_at(&p, &keys);
        let got: Vec<(String, usize)> = found.iter().map(|c| (c.keys.clone(), c.line)).collect();
        // The commented-out line and the free keys are not conflicts; case and modifier order are ignored.
        assert_eq!(got, vec![("Mod+Shift+S".to_string(), 2), ("Mod+Shift+H".to_string(), 3)], "{found:?}");
        assert_eq!(found[0].file, "config.d/70-binds.kdl");
    }

    #[test]
    fn our_own_file_is_never_reported_as_a_conflict() {
        let sb = Sandbox::new("own");
        std::fs::create_dir_all(sb.0.join("config.d")).unwrap();
        std::fs::write(sb.0.join("config.d/55-shimeji.kdl"), render(&opts())).unwrap();
        assert!(conflicts_at(&sb.paths(), &["Mod+Shift+D".to_string()]).is_empty());
    }

    /// Runs enable/disable against a COPY of the real ~/.config/niri (the real
    /// one is never touched): `cargo test live_niri -- --ignored`.
    #[test]
    #[ignore]
    fn live_niri_config_accepts_our_file() {
        if !niri_available() {
            return;
        }
        let real = Paths::real();
        let sb = Sandbox::new("live");
        fn copy(from: &Path, to: &Path) {
            std::fs::create_dir_all(to).unwrap();
            for e in std::fs::read_dir(from).unwrap().flatten() {
                let (src, dst) = (e.path(), to.join(e.file_name()));
                if src.is_dir() {
                    copy(&src, &dst);
                } else {
                    let _ = std::fs::copy(&src, &dst);
                }
            }
        }
        copy(&real.dir, &sb.0);
        let p = sb.paths();
        let s = enable_at(&p, &opts()).expect("enable against the real config");
        assert!(s.active && s.validation == "ok", "{s:?}");
        disable_at(&p).expect("disable");

        // The keys the app offers by default must be free in this config.
        let defaults = ["Mod+Ctrl+D", "Mod+Ctrl+X", "Mod+Ctrl+H"].map(String::from);
        assert!(conflicts_at(&real, &defaults).is_empty(), "{:?}", conflicts_at(&real, &defaults));
    }
}
