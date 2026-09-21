// Hyprland integration: launch at login and keybinds.
//
// The same idea as niri's (see `niri.rs`), in Hyprland's terms. We write our own file, `menagerie.conf` next to
// `hyprland.conf`, and hook it in with ONE `source =` line appended to `hyprland.conf`, after keeping a backup of
// it. Turning the feature off removes the line and the files again.
//
// What differs from niri:
//   • there is no validator for a config on its own: Hyprland reads the real one when it reloads, and reports what
//     it did not like through `hyprctl configerrors`. So the change is written, Hyprland is asked to reload, and
//     what it says about OUR file decides whether to keep it. Off a Hyprland session there is nothing to ask, and
//     the change is written unchecked (and says so);
//   • the login commands live in a small script (`menagerie-login.sh`) that `exec-once` runs, because a config line
//     is no place for `$`, quotes and pipes (`$name` is a variable there, `#` starts a comment);
//   • keys are spelled `SUPER SHIFT, D`, not `Mod+Shift+D`. The Startup tab captures keys in niri's spelling, and
//     they are translated here; one that has no equivalent is left out and named, never guessed.
//
// Not yet run against a real Hyprland: the behaviour was checked against
// Hyprland's documented config format and against a stand-in for `hyprctl`. Its reports are welcome.

use crate::niri::{Bind, Options};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize, Clone, Debug)]
pub struct Status {
    /// Our file exists AND hyprland.conf sources it: Hyprland is using it.
    pub active: bool,
    pub file: String,
    /// The line that hooks the file into hyprland.conf.
    pub include_line: String,
    pub backup: String,
    /// "ok", "not checked" (no Hyprland running to ask), or what Hyprland said about our file.
    pub validation: String,
    /// Binds that could not be written, one line each, with the reason.
    pub skipped: Vec<String>,
}

#[derive(Clone, Debug)]
struct Paths {
    dir: PathBuf,
    /// The program to ask (a test points this at a stand-in).
    hyprctl: String,
    /// Whether there is a running Hyprland to ask at all.
    running: bool,
}

impl Paths {
    fn real() -> Self {
        let base = std::env::var("XDG_CONFIG_HOME")
            .ok()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "~".to_string())).join(".config"));
        Self {
            dir: base.join("hypr"),
            hyprctl: "hyprctl".to_string(),
            running: std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some_and(|v| !v.is_empty()),
        }
    }

    fn main(&self) -> PathBuf {
        self.dir.join("hyprland.conf")
    }
    fn backup(&self) -> PathBuf {
        self.dir.join("hyprland.conf.menagerie-backup")
    }
    fn conf(&self) -> PathBuf {
        self.dir.join("menagerie.conf")
    }
    fn script(&self) -> PathBuf {
        self.dir.join("menagerie-login.sh")
    }
    /// An absolute path: it works wherever hyprland.conf itself is read from.
    fn source_line(&self) -> String {
        format!("source = {}", self.conf().display())
    }
}

/// The comment we put above the source line, so it is clear who added it.
const MARK: &str = "# Menagerie: launch at login and keybinds (remove this line and the next to disable)";

// ------------------------------------------------------------------ keys

/// Niri's spelling of a key press ("Mod+Shift+D") in Hyprland's: `("SUPER SHIFT", "D")`. None when there is no
/// equivalent this file is sure of.
pub fn convert_keys(niri_keys: &str) -> Option<(String, String)> {
    let mut mods: Vec<&str> = Vec::new();
    let mut key: Option<String> = None;
    for part in niri_keys.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        let m = match part.to_lowercase().as_str() {
            "mod" | "super" | "win" | "mod4" => Some("SUPER"),
            "ctrl" | "control" => Some("CTRL"),
            "alt" | "mod1" => Some("ALT"),
            "shift" => Some("SHIFT"),
            _ => None,
        };
        match m {
            Some(m) => {
                if !mods.contains(&m) {
                    mods.push(m);
                }
            }
            None => {
                if key.is_some() {
                    return None; // two keys in one combination
                }
                key = Some(key_name(part)?);
            }
        }
    }
    Some((mods.join(" "), key?))
}

/// Hyprland's (xkb) name for a key the Startup tab can produce.
fn key_name(k: &str) -> Option<String> {
    if k.len() == 1 && k.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Some(k.to_uppercase());
    }
    if let Some(n) = k.strip_prefix(['F', 'f']) {
        if !n.is_empty() && n.len() <= 2 && n.chars().all(|c| c.is_ascii_digit()) {
            return Some(format!("F{n}"));
        }
    }
    Some(
        match k.to_lowercase().as_str() {
            "return" | "enter" => "Return",
            "space" => "space",
            "tab" => "Tab",
            "up" => "Up",
            "down" => "Down",
            "left" => "Left",
            "right" => "Right",
            "home" => "Home",
            "end" => "End",
            "insert" => "Insert",
            "delete" => "Delete",
            "page_up" | "pageup" => "Prior",
            "page_down" | "pagedown" => "Next",
            _ => return None,
        }
        .to_string(),
    )
}

// ------------------------------------------------------------------ rendering

struct Rendered {
    conf: String,
    /// The login script, when there is anything to run at login.
    script: Option<String>,
    skipped: Vec<String>,
}

/// A bind line for `b`, or why not.
fn bind_line(b: &Bind) -> Result<Option<String>, String> {
    if b.keys.trim().is_empty() {
        return Ok(None);
    }
    let (mods, key) = convert_keys(&b.keys).ok_or_else(|| format!("{}: Hyprland spells that key differently and it was not guessed", b.keys.trim()))?;
    let command = match b.action.as_str() {
        // `dismiss --all` first waits for a mouse selection; the interrupt cancels it (see niri.rs).
        "dismiss_all" => "timeout -s INT 0.7 shimejictl mascot dismiss --all".to_string(),
        "stop" => "shimejictl stop".to_string(),
        "summon" if !b.arg.is_empty() => {
            // A config line has two characters with a meaning of their own.
            if b.arg.contains(['#', '$']) {
                return Err(format!("{}: the name \"{}\" has a character a Hyprland config line cannot carry", b.keys.trim(), b.arg));
            }
            format!("shimejictl summon '{}'", b.arg.replace('\'', "'\\''"))
        }
        _ => return Ok(None),
    };
    Ok(Some(format!("bind = {mods}, {key}, exec, {command}")))
}

fn render(o: &Options, p: &Paths) -> Rendered {
    let mut lines = vec!["# Menagerie: written by the Startup tab. Turn it off there.".to_string()];
    if o.overlay {
        lines.push("exec-once = shimeji-overlayd".to_string());
    }

    let script = crate::niri::login_commands(o).map(|parts| {
        let mut text = "#!/bin/sh\n# Menagerie: bring the characters back at login (run by exec-once in menagerie.conf).\n".to_string();
        text.push_str(&parts.join("\n"));
        text.push('\n');
        text
    });
    if script.is_some() {
        lines.push(format!("exec-once = /bin/sh {}", p.script().display()));
    }

    let mut skipped = Vec::new();
    for b in &o.binds {
        match bind_line(b) {
            Ok(Some(line)) => lines.push(line),
            Ok(None) => {}
            Err(why) => skipped.push(why),
        }
    }
    Rendered { conf: lines.join("\n") + "\n", script, skipped }
}

/// What the Startup tab shows before anything is written.
pub fn preview(o: &Options) -> String {
    let r = render(o, &Paths::real());
    let mut text = r.conf;
    if let Some(script) = r.script {
        text.push_str(&format!("\n# --- menagerie-login.sh ---\n{}", script.lines().map(|l| format!("# {l}")).collect::<Vec<_>>().join("\n")));
        text.push('\n');
    }
    text
}

// ------------------------------------------------------------------ the main config

/// Lines of hyprland.conf that source our file.
fn source_lines(text: &str, conf: &Path) -> Vec<usize> {
    let name = conf.display().to_string();
    text.lines()
        .enumerate()
        .filter(|(_, l)| {
            let t = l.trim_start();
            !t.starts_with('#') && t.starts_with("source") && (l.contains(&name) || l.contains("menagerie.conf"))
        })
        .map(|(i, _)| i)
        .collect()
}

fn is_sourced(p: &Paths) -> bool {
    std::fs::read_to_string(p.main()).map(|t| !source_lines(&t, &p.conf()).is_empty()).unwrap_or(false)
}

// ------------------------------------------------------------------ asking Hyprland

#[derive(Debug, PartialEq)]
enum Check {
    Ok,
    /// There is no running Hyprland to ask.
    Unchecked,
    /// Hyprland reported errors that name our file.
    Problem(String),
}

fn ask(p: &Paths, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(&p.hyprctl)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    Some(format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)).trim().to_string())
}

/// What Hyprland says about the config it has loaded, as far as it concerns us. Errors in the user's own lines are
/// theirs: not ours to report and certainly not ours to undo.
fn config_errors(p: &Paths) -> Check {
    if !p.running {
        return Check::Unchecked;
    }
    let Some(text) = ask(p, &["configerrors"]) else { return Check::Unchecked };
    let lower = text.to_lowercase();
    if text.is_empty() || lower.contains("no errors") || !lower.contains("menagerie") {
        Check::Ok
    } else {
        Check::Problem(text)
    }
}

/// Asks Hyprland to read the config again, then what it thought of it.
fn reload_and_check(p: &Paths) -> Check {
    if !p.running {
        return Check::Unchecked;
    }
    if ask(p, &["reload"]).is_none() {
        return Check::Unchecked;
    }
    config_errors(p)
}

fn validation_text(check: &Check) -> String {
    match check {
        Check::Ok => "ok".to_string(),
        Check::Unchecked => "not checked".to_string(),
        Check::Problem(text) => text.clone(),
    }
}

fn status_of(p: &Paths, skipped: Vec<String>) -> Status {
    let active = is_sourced(p) && p.conf().exists();
    Status {
        active,
        file: p.conf().display().to_string(),
        include_line: p.source_line(),
        backup: p.backup().display().to_string(),
        validation: if active { validation_text(&config_errors(p)) } else { String::new() },
        skipped,
    }
}

// ------------------------------------------------------------------ writing

fn write_file(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("could not write {}: {e}", path.display()))
}

fn write_script(path: &Path, text: &str) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    write_file(path, text)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).map_err(|e| format!("could not make {} runnable: {e}", path.display()))
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

/// Writes our two files. Returns what was there before, for `undo`.
struct Written {
    conf_before: Option<String>,
    script_before: Option<String>,
}

fn write_ours(p: &Paths, r: &Rendered) -> Result<Written, String> {
    let before = Written { conf_before: std::fs::read_to_string(p.conf()).ok(), script_before: std::fs::read_to_string(p.script()).ok() };
    write_file(&p.conf(), &r.conf)?;
    match &r.script {
        Some(text) => write_script(&p.script(), text)?,
        None => {
            let _ = std::fs::remove_file(p.script());
        }
    }
    Ok(before)
}

fn undo(p: &Paths, before: &Written) {
    restore(&p.conf(), &before.conf_before);
    restore(&p.script(), &before.script_before);
}

fn apply_at(p: &Paths, o: &Options) -> Result<Status, String> {
    let r = render(o, p);
    let before = write_ours(p, &r)?;
    if is_sourced(p) {
        if let Check::Problem(text) = reload_and_check(p) {
            undo(p, &before);
            let _ = reload_and_check(p);
            return Err(format!("Hyprland rejected the config:\n{text}"));
        }
    }
    Ok(status_of(p, r.skipped))
}

fn enable_at(p: &Paths, o: &Options) -> Result<Status, String> {
    let main = p.main();
    let original = std::fs::read_to_string(&main).map_err(|e| format!("could not read {} (is this where your Hyprland config is?): {e}", main.display()))?;

    let r = render(o, p);
    let before = write_ours(p, &r)?;

    if source_lines(&original, &p.conf()).is_empty() {
        std::fs::write(p.backup(), &original).map_err(|e| format!("could not write the backup {}: {e}", p.backup().display()))?;
        let mut updated = original.clone();
        if !updated.is_empty() && !updated.ends_with('\n') {
            updated.push('\n');
        }
        updated.push_str(&format!("\n{MARK}\n{}\n", p.source_line()));
        if let Err(e) = std::fs::write(&main, &updated) {
            undo(p, &before);
            return Err(format!("could not update {}: {e}", main.display()));
        }
    }

    if let Check::Problem(text) = reload_and_check(p) {
        let _ = std::fs::write(&main, &original);
        undo(p, &before);
        let _ = reload_and_check(p);
        return Err(format!("Hyprland rejected the config, nothing was changed:\n{text}"));
    }
    Ok(status_of(p, r.skipped))
}

fn disable_at(p: &Paths) -> Result<Status, String> {
    let main = p.main();
    if let Ok(original) = std::fs::read_to_string(&main) {
        let drop_at = source_lines(&original, &p.conf());
        if !drop_at.is_empty() {
            let kept: Vec<&str> = original
                .lines()
                .enumerate()
                // Drop the source line and our comment right above it.
                .filter(|(i, l)| !drop_at.contains(i) && l.trim() != MARK)
                .map(|(_, l)| l)
                .collect();
            let mut updated = kept.join("\n");
            while updated.ends_with("\n\n") {
                updated.pop();
            }
            if !updated.ends_with('\n') {
                updated.push('\n');
            }
            std::fs::write(&main, &updated).map_err(|e| format!("could not update {}: {e}", main.display()))?;
            if let Check::Problem(text) = reload_and_check(p) {
                let _ = std::fs::write(&main, &original);
                return Err(format!("Hyprland rejected the config, nothing was changed:\n{text}"));
            }
        }
    }
    let _ = std::fs::remove_file(p.conf());
    let _ = std::fs::remove_file(p.script());
    Ok(status_of(p, Vec::new()))
}

// ------------------------------------------------------------------ conflicts

/// One of our keys that the user's own Hyprland config already binds.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Conflict {
    pub keys: String,
    pub file: String,
    pub line: usize,
}

/// `SUPER_SHIFT`, `$mainMod SHIFT`, `super shift`: the same set of modifiers, in one order.
fn normalise_mods(mods: &str, vars: &std::collections::HashMap<String, String>) -> Vec<String> {
    let mut set: Vec<String> = mods
        .split([' ', '_', '+'])
        .filter(|s| !s.is_empty())
        .flat_map(|m| match m.strip_prefix('$').and_then(|v| vars.get(v)) {
            Some(value) => value.split([' ', '_', '+']).map(str::to_string).collect::<Vec<_>>(),
            None => vec![m.to_string()],
        })
        .map(|m| match m.to_uppercase().as_str() {
            "SUPER" | "WIN" | "MOD4" | "META" => "SUPER".to_string(),
            "CTRL" | "CONTROL" => "CTRL".to_string(),
            "ALT" | "MOD1" => "ALT".to_string(),
            other => other.to_string(),
        })
        .collect();
    set.sort();
    set.dedup();
    set
}

/// Every bind in `file` and the files it sources: (mods, key, file shown, line).
fn binds_in(file: &Path, dir: &Path, ours: &Path, depth: usize, vars: &mut std::collections::HashMap<String, String>, out: &mut Vec<(String, String, String, usize)>) {
    if depth > 4 || file == ours {
        return;
    }
    let Ok(text) = std::fs::read_to_string(file) else { return };
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix('$') {
            if let Some((name, value)) = rest.split_once('=') {
                vars.insert(name.trim().to_string(), value.split('#').next().unwrap_or("").trim().to_string());
            }
        } else if let Some((head, value)) = line.split_once('=') {
            let head = head.trim();
            if head == "source" {
                let target = value.trim();
                let target = match target.strip_prefix("~/") {
                    Some(rest) => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(rest),
                    None if Path::new(target).is_absolute() => PathBuf::from(target),
                    None => dir.join(target),
                };
                binds_in(&target, dir, ours, depth + 1, vars, out);
            } else if head.starts_with("bind") {
                let mut parts = value.splitn(3, ',');
                let (mods, key) = (parts.next().unwrap_or("").trim(), parts.next().unwrap_or("").trim());
                if !key.is_empty() {
                    let shown = file.strip_prefix(dir).unwrap_or(file).display().to_string();
                    out.push((normalise_mods(mods, vars).join(" "), key.to_lowercase(), shown, i + 1));
                }
            }
        }
    }
}

fn conflicts_at(p: &Paths, keys: &[String]) -> Vec<Conflict> {
    let mut binds = Vec::new();
    binds_in(&p.main(), &p.dir, &p.conf(), 0, &mut std::collections::HashMap::new(), &mut binds);
    keys.iter()
        .filter_map(|k| {
            let (mods, key) = convert_keys(k)?;
            let mods = normalise_mods(&mods, &std::collections::HashMap::new()).join(" ");
            let key = key.to_lowercase();
            binds.iter().find(|(m, kk, _, _)| *m == mods && *kk == key).map(|(_, _, file, line)| Conflict { keys: k.clone(), file: file.clone(), line: *line })
        })
        .collect()
}

// ------------------------------------------------------------------ what the app calls

pub fn conflicts(keys: &[String]) -> Vec<Conflict> {
    conflicts_at(&Paths::real(), keys)
}

pub fn status() -> Status {
    status_of(&Paths::real(), Vec::new())
}

pub fn apply(o: &Options) -> Result<Status, String> {
    apply_at(&Paths::real(), o)
}

pub fn enable(o: &Options) -> Result<Status, String> {
    enable_at(&Paths::real(), o)
}

pub fn disable() -> Result<Status, String> {
    disable_at(&Paths::real())
}

/// Whether the app can write Hyprland's config here: the file is where it is expected.
pub fn config_exists() -> bool {
    Paths::real().main().is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("menagerie-hypr-test/{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A stand-in for `hyprctl` that answers `configerrors` with `errors` and everything else with nothing.
    fn fake_hyprctl(dir: &Path, errors: &str) -> String {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("hyprctl-fake");
        std::fs::write(&path, format!("#!/bin/sh\nif [ \"$1\" = configerrors ]; then printf '%s' '{errors}'; fi\necho \"$@\" >> \"{}/calls\"\n", dir.display())).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.display().to_string()
    }

    fn paths(name: &str, errors: Option<&str>) -> Paths {
        let dir = scratch(name);
        std::fs::create_dir_all(dir.join("hypr")).unwrap();
        std::fs::write(dir.join("hypr/hyprland.conf"), "monitor=,preferred,auto,1\n$mainMod = SUPER\nbind = $mainMod, Q, killactive\nbind = $mainMod SHIFT, D, exec, foot\n").unwrap();
        match errors {
            Some(e) => Paths { dir: dir.join("hypr"), hyprctl: fake_hyprctl(&dir, e), running: true },
            None => Paths { dir: dir.join("hypr"), hyprctl: "hyprctl-does-not-exist".to_string(), running: false },
        }
    }

    fn options() -> Options {
        Options {
            overlay: true,
            delay: 3.0,
            mascots: vec![("Ezio".to_string(), 1), ("Leonardo da Vinci".to_string(), 2)],
            random: 2,
            binds: vec![
                Bind { keys: "Mod+Ctrl+D".into(), action: "dismiss_all".into(), arg: String::new() },
                Bind { keys: "Mod+Shift+E".into(), action: "summon".into(), arg: "Ezio".into() },
            ],
        }
    }

    #[test]
    fn niris_key_spelling_becomes_hyprlands() {
        assert_eq!(convert_keys("Mod+Ctrl+D"), Some(("SUPER CTRL".into(), "D".into())));
        assert_eq!(convert_keys("Mod+Shift+F5"), Some(("SUPER SHIFT".into(), "F5".into())));
        assert_eq!(convert_keys("Alt+Return"), Some(("ALT".into(), "Return".into())));
        assert_eq!(convert_keys("Mod+Space"), Some(("SUPER".into(), "space".into())));
        assert_eq!(convert_keys("Mod+Page_Up"), Some(("SUPER".into(), "Prior".into())));
        assert_eq!(convert_keys("Mod+Left"), Some(("SUPER".into(), "Left".into())));
        assert_eq!(convert_keys("F10"), Some((String::new(), "F10".into())), "a key with no modifier");
    }

    #[test]
    fn a_key_with_no_certain_equivalent_is_refused_and_not_guessed() {
        assert_eq!(convert_keys("Mod+Ctrl"), None, "no key at all");
        assert_eq!(convert_keys("Mod+D+E"), None, "two keys");
        assert_eq!(convert_keys("Mod+XF86AudioPlay"), None);
        let bind = Bind { keys: "Mod+XF86AudioPlay".into(), action: "stop".into(), arg: String::new() };
        assert!(bind_line(&bind).unwrap_err().contains("not guessed"));
    }

    #[test]
    fn the_config_carries_the_overlay_the_login_script_and_the_binds() {
        let p = paths("render", None);
        let r = render(&options(), &p);
        assert!(r.conf.contains("exec-once = shimeji-overlayd\n"), "{}", r.conf);
        assert!(r.conf.contains(&format!("exec-once = /bin/sh {}", p.script().display())), "{}", r.conf);
        assert!(r.conf.contains("bind = SUPER CTRL, D, exec, timeout -s INT 0.7 shimejictl mascot dismiss --all"), "{}", r.conf);
        assert!(r.conf.contains("bind = SUPER SHIFT, E, exec, shimejictl summon 'Ezio'"), "{}", r.conf);
        // The pipes, `$n` and quotes are in the script, where they mean what they say.
        let script = r.script.unwrap();
        assert!(script.starts_with("#!/bin/sh\n") && script.contains("shuf -n 2") && script.contains("shimejictl summon 'Leonardo da Vinci'"), "{script}");
        assert!(!r.conf.contains('$'), "nothing in the config line itself may look like a Hyprland variable");
    }

    #[test]
    fn a_name_with_a_character_a_config_line_cannot_carry_is_left_out_and_named() {
        let mut o = options();
        o.binds = vec![Bind { keys: "Mod+X".into(), action: "summon".into(), arg: "Mr #1".into() }];
        let r = render(&o, &paths("skip", None));
        assert!(!r.conf.contains("bind ="));
        assert_eq!(r.skipped.len(), 1);
        assert!(r.skipped[0].contains("Mr #1"), "{:?}", r.skipped);
    }

    #[test]
    fn turning_it_on_adds_one_source_line_with_a_backup_and_turning_it_off_restores_the_file_exactly() {
        let p = paths("roundtrip", Some(""));
        let original = std::fs::read_to_string(p.main()).unwrap();

        let status = enable_at(&p, &options()).unwrap();
        assert!(status.active && status.validation == "ok", "{status:?}");
        let now = std::fs::read_to_string(p.main()).unwrap();
        assert_eq!(now.matches("source = ").count(), 1, "exactly one line added: {now}");
        assert!(now.contains(MARK) && now.contains(&p.source_line()));
        assert_eq!(std::fs::read_to_string(p.backup()).unwrap(), original, "the backup is the untouched file");
        assert!(p.conf().exists() && p.script().exists());
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(p.script()).unwrap().permissions().mode() & 0o111, 0o111, "the script can be run");
        }
        let calls = std::fs::read_to_string(p.dir.parent().unwrap().join("calls")).unwrap();
        assert!(calls.contains("reload"), "Hyprland was asked to reload: {calls}");

        // Turning it on again writes nothing twice.
        enable_at(&p, &options()).unwrap();
        assert_eq!(std::fs::read_to_string(p.main()).unwrap().matches("source = ").count(), 1);

        let off = disable_at(&p).unwrap();
        assert!(!off.active);
        assert_eq!(std::fs::read_to_string(p.main()).unwrap(), original, "the config is as it was, byte for byte");
        assert!(!p.conf().exists() && !p.script().exists(), "and our files are gone");
    }

    #[test]
    fn a_config_hyprland_complains_about_is_rolled_back_and_nothing_stays() {
        let p = paths("rollback", Some("Config error in file /x/hypr/menagerie.conf at line 3: invalid dispatcher"));
        let original = std::fs::read_to_string(p.main()).unwrap();
        let error = enable_at(&p, &options()).unwrap_err();
        assert!(error.contains("Hyprland rejected the config") && error.contains("invalid dispatcher"), "{error}");
        assert_eq!(std::fs::read_to_string(p.main()).unwrap(), original, "hyprland.conf is untouched");
        assert!(!p.conf().exists() && !p.script().exists(), "and so is the folder");
    }

    #[test]
    fn an_error_in_the_users_own_lines_is_not_ours_to_report_or_undo() {
        let p = paths("theirs", Some("Config error in file /home/x/.config/hypr/hyprland.conf at line 9: unknown variable"));
        let status = enable_at(&p, &options()).unwrap();
        assert!(status.active, "it stays on: the complaint is about their file, not ours");
    }

    #[test]
    fn without_a_running_hyprland_it_is_written_unchecked_and_says_so() {
        let p = paths("norun", None);
        let status = enable_at(&p, &options()).unwrap();
        assert!(status.active);
        assert_eq!(status.validation, "not checked");
        assert!(disable_at(&p).is_ok());
    }

    #[test]
    fn a_missing_config_is_an_error_that_says_where_it_looked() {
        let dir = scratch("noconf");
        let p = Paths { dir: dir.join("hypr"), hyprctl: "x".into(), running: false };
        let error = enable_at(&p, &options()).unwrap_err();
        assert!(error.contains("hyprland.conf") && error.contains("could not read"), "{error}");
        assert!(!p.conf().exists());
    }

    #[test]
    fn a_key_the_users_config_already_binds_is_found_whatever_way_they_spelled_it() {
        let p = paths("conflict", None);
        // hyprland.conf binds `$mainMod SHIFT, D` (mainMod = SUPER) and `$mainMod, Q`.
        let found = conflicts_at(&p, &["Mod+Shift+D".to_string(), "Mod+Q".to_string(), "Mod+Ctrl+D".to_string()]);
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!((found[0].keys.as_str(), found[0].line), ("Mod+Shift+D", 4));
        assert_eq!((found[1].keys.as_str(), found[1].line), ("Mod+Q", 3));
    }

    #[test]
    fn binds_in_sourced_files_count_and_our_own_file_does_not() {
        let p = paths("sourced", None);
        std::fs::write(p.dir.join("binds.conf"), "bind = SUPER_CTRL, D, exec, foot\n").unwrap();
        let mut main = std::fs::read_to_string(p.main()).unwrap();
        main.push_str("source = binds.conf\n");
        std::fs::write(p.main(), main).unwrap();
        std::fs::write(p.conf(), "bind = SUPER SHIFT, E, exec, ours\n").unwrap();
        let mut text = std::fs::read_to_string(p.main()).unwrap();
        text.push_str(&format!("{}\n", p.source_line()));
        std::fs::write(p.main(), text).unwrap();

        let found = conflicts_at(&p, &["Mod+Ctrl+D".to_string(), "Mod+Shift+E".to_string()]);
        assert_eq!(found.len(), 1, "the one in the sourced file, not the one we wrote ourselves: {found:?}");
        assert_eq!(found[0].file, "binds.conf");
    }
}
