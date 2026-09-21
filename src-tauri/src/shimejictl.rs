// Wrapper around the `shimejictl` CLI (part of wl_shimeji).
//
// WHAT IS CERTAIN AND WHAT IS ASSUMED
//
// Certain (from the wl_shimeji README, checked on a live system):
//   shimejictl convert <archive> -O <dir>      - interactive, asks which to convert
//   shimejictl prototypes import [-f] <files>
//   shimejictl prototypes list
//   shimejictl prototypes info <name>
//   shimejictl prototypes export -i <name> -o <file>
//   shimejictl mascot summon <name> [-x -y | --select]
//   shimejictl mascot dismiss [-a | -i ID | -s]
//   shimejictl mascot set-behavior <behavior>
//   shimejictl config get|set|list
//   shimejictl stop
//
// Notes from testing:
//   • `config list` prints a different format when the overlay is not running
//   • there is no command that deletes a prototype at all
//
// So removal works on disk: it erases the prototype's folder or file and asks
// the overlay to reload. Every step returns an explanation of what did not work,
// so the UI can show the reason.

use serde::Serialize;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// One overlay parameter: the real key, the human label from wl_shimeji itself,
/// and the current value.
#[derive(Serialize, Clone, Debug)]
pub struct ConfigOption {
    pub key: String,
    pub label: String,
    pub value: String,
    /// false means the overlay is not running and the value was read from the config file.
    pub live: bool,
}

/// All shimejictl calls go one at a time. Parallel ones (fast clicks on "Summon random",
/// several `summon` calls at once) crashed `shimeji-overlayd`. Verified: 8 sequential
/// calls work, 12 parallel ones bring the overlay down.
static CTL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    CTL.lock().unwrap_or_else(|e| e.into_inner())
}

/// What to say when a wl_shimeji program cannot even be started because it is not there.
/// "No such file or directory (os error 2)" means nothing to someone who has just
/// installed this app and not the engine that it drives.
/// What a summon says when it was called off. The UI knows this text and stays quiet.
pub const CANCELLED: &str = "Summoning stopped.";

pub const NOT_INSTALLED: &str = "wl_shimeji is not installed (shimejictl was not found), and the app needs it to show characters.";

fn start_error(what: &str, e: &std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::NotFound {
        NOT_INSTALLED.to_string()
    } else {
        format!("could not start {what}: {e}")
    }
}

/// wl_shimeji prints Python log lines when the overlay is gone ("Invalid header in
/// packet (Expected at least 8 bytes, got 0)"). That means "it crashed", and that is
/// what the person should read.
pub fn friendly(err: &str) -> String {
    let e = err.to_lowercase();
    let gone = ["invalid header in packet", "failed to start client", "failed to start overlay", "broken pipe", "connection reset", "connection refused"];
    if gone.iter().any(|m| e.contains(m)) {
        "The overlay is not responding. It has probably crashed.".to_string()
    } else {
        err.to_string()
    }
}

fn run(args: &[&str]) -> Result<String, String> {
    let _guard = lock();
    run_unlocked(args)
}

/// What to tell the person when a `shimejictl` command failed.
fn command_failure(args: &[&str], stderr: &str) -> String {
    let stderr = stderr.trim();
    let friendly = friendly(stderr);
    if friendly == stderr {
        format!("`shimejictl {}` → {}", args.join(" "), friendly)
    } else {
        friendly
    }
}

/// How long an ordinary `shimejictl` call may take before the app gives up on it. The slowest real ones (importing a
/// big character) take seconds.
const CALL_SECONDS: u64 = 60;

/// What a call that was given up on says. It is also how the caller recognises one (`is_no_answer`).
const NO_ANSWER: &str = "The engine did not answer";

/// Whether an error is "the engine went quiet", as opposed to "the engine said no".
pub fn is_no_answer(error: &str) -> bool {
    error.starts_with(NO_ANSWER)
}

fn run_unlocked(args: &[&str]) -> Result<String, String> {
    run_program("shimejictl", args, CALL_SECONDS)
}

/// One call with its own time limit. Takes the lock like every other call.
pub fn run_timeout(args: &[&str], seconds: u64) -> Result<String, String> {
    let _guard = lock();
    run_program("shimejictl", args, seconds)
}

/// Prepares a `Command` with Python environment variables cleaned.
///
/// When Menagerie is run from an AppImage, the AppImage runtime can export
/// `PYTHONHOME` and `PYTHONPATH` pointing into its temporary mount directory
/// (`/tmp/.mount_.../usr`). If child processes inherit these, host `python3`,
/// `shimejictl`, and `shimeji-overlayd` fail immediately during startup with
/// `ModuleNotFoundError: No module named 'encodings'`. Removing them restores
/// normal host Python library resolution.
fn clean_command<S: AsRef<std::ffi::OsStr>>(program: S) -> Command {
    let mut cmd = Command::new(program);
    cmd.env_remove("PYTHONHOME");
    cmd.env_remove("PYTHONPATH");
    cmd
}

/// Runs a program to its end, but not for longer than `seconds`: then it is killed and this says so.
///
/// There used to be no limit, and every call holds the lock the others queue on. One `prototypes export` for a
/// character the engine had half forgotten never got an answer (the process sat in a read, using nothing, for eleven
/// minutes and counting), and with it every summon, every setting and every scene check of the whole app.
fn run_program(program: &str, args: &[&str], seconds: u64) -> Result<String, String> {
    run_program_full(program, args, seconds).map(|(stdout, _)| stdout)
}

/// `run_program` that also hands back what the program printed on stderr. The engine reports what went wrong with
/// each file it was given only there, and still exits with 0.
fn run_program_full(program: &str, args: &[&str], seconds: u64) -> Result<(String, String), String> {
    let mut child = clean_command(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| start_error(program, &e))?;

    // Both pipes are read on threads of their own, or a program that prints a lot would fill one and stall.
    let reader = |pipe: Option<Box<dyn Read + Send>>| {
        pipe.map(|mut p| {
            std::thread::spawn(move || {
                let mut bytes = Vec::new();
                let _ = p.read_to_end(&mut bytes);
                bytes
            })
        })
    };
    let out = reader(child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>));
    let err = reader(child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>));

    let deadline = Instant::now() + Duration::from_secs(seconds);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "{NO_ANSWER} `{program} {}` within {seconds} s, so the app stopped waiting. It is probably stuck on a character that is only half installed.",
                    args.join(" ")
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(15)),
            Err(e) => return Err(format!("could not wait for {program}: {e}")),
        }
    };

    let stdout = out.and_then(|t| t.join().ok()).unwrap_or_default();
    let stderr = err.and_then(|t| t.join().ok()).unwrap_or_default();
    if !status.success() {
        return Err(command_failure(args, &String::from_utf8_lossy(&stderr)));
    }
    Ok((String::from_utf8_lossy(&stdout).to_string(), String::from_utf8_lossy(&stderr).to_string()))
}

/// One call with a time limit that also returns stderr.
pub fn run_timeout_full(args: &[&str], seconds: u64) -> Result<(String, String), String> {
    let _guard = lock();
    run_program_full("shimejictl", args, seconds)
}

// ---------------------------------------------------------------------------
// Two-step conversion (the process waits for the user's choice between steps)
// ---------------------------------------------------------------------------

pub struct PendingConvert {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
    out_dir: PathBuf,
}

/// Step 1: starts convert and reads the list of available prototypes.
pub fn start_convert(
    archive_path: &Path,
    out_dir: &Path,
) -> Result<(PendingConvert, Vec<String>), String> {
    std::fs::create_dir_all(out_dir).map_err(|e| format!("could not create the folder {}: {e}", out_dir.display()))?;

    let mut child = clean_command("shimejictl")
        .arg("convert")
        .arg(archive_path)
        .arg("-O")
        .arg(out_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| start_error("shimejictl convert", &e))?;

    let stdin = child.stdin.take().ok_or("no access to the process stdin")?;
    let stdout = child.stdout.take().ok_or("no access to the process stdout")?;
    let mut reader = BufReader::new(stdout);

    let mut prototypes = Vec::new();
    let mut transcript = String::new();

    // Read byte by byte: the last line is a "> " prompt without a newline, and
    // read_line would block on it forever.
    loop {
        let mut line = Vec::new();
        let mut byte = [0u8; 1];

        loop {
            match reader.read(&mut byte) {
                Ok(0) => break,
                Ok(_) => {
                    if byte[0] == b'\n' {
                        break;
                    }
                    line.push(byte[0]);
                    if byte[0] == b'>' {
                        break;
                    }
                }
                Err(e) => return Err(format!("error reading the convert output: {e}")),
            }
        }

        if line.is_empty() {
            break;
        }

        let text = String::from_utf8_lossy(&line).trim().to_string();
        transcript.push_str(&text);
        transcript.push('\n');

        // A line like "1. Weuron"
        if let Some((num, name)) = text.split_once('.') {
            if !num.trim().is_empty() && num.trim().chars().all(|c| c.is_ascii_digit()) {
                let name = name.trim();
                if !name.is_empty() {
                    prototypes.push(name.to_string());
                    continue;
                }
            }
        }

        if text.ends_with('>') || text.to_lowercase().contains("enter prototypes") {
            break;
        }
    }

    if prototypes.is_empty() {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!(
            "shimejictl did not show a list of characters. Its output:\n{transcript}"
        ));
    }

    Ok((
        PendingConvert {
            child,
            stdin,
            reader,
            out_dir: out_dir.to_path_buf(),
        },
        prototypes,
    ))
}

/// Step 2: passes the choice and runs the conversion to the end.
pub fn finish_convert(
    mut pending: PendingConvert,
    selection: &[String],
) -> Result<Vec<PathBuf>, String> {
    let answer = if selection.is_empty() {
        "A".to_string()
    } else {
        selection.join(",")
    };

    writeln!(pending.stdin, "{answer}")
        .map_err(|e| format!("could not pass the selection: {e}"))?;
    pending.stdin.flush().ok();
    drop(pending.stdin);

    let mut rest = String::new();
    for line in pending.reader.lines().map_while(Result::ok) {
        rest.push_str(&line);
        rest.push('\n');
    }

    let status = pending
        .child
        .wait()
        .map_err(|e| format!("convert did not finish: {e}"))?;

    if !status.success() {
        return Err(format!("convert failed:\n{rest}"));
    }

    let mut result = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&pending.out_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("wlshm") {
                result.push(path);
            }
        }
    }

    if result.is_empty() {
        return Err(format!(
            "the conversion ran but produced no .wlshm files. Output:\n{rest}"
        ));
    }

    Ok(result)
}

pub fn cancel_convert(mut pending: PendingConvert) {
    let _ = pending.child.kill();
    let _ = pending.child.wait();
}

// ---------------------------------------------------------------------------
// Prototypes
// ---------------------------------------------------------------------------

/// Characters that are already in wl_shimeji's own format and only need adding.
///
/// `shimejictl convert` turns a Shimeji-EE folder (`img/` + `conf/`) into `.wlshm`
/// prototypes; it refuses anything else, including wl_shimeji's own `.wlshm` files.
/// So an export of this very app — a zip full of `.wlshm` — could not be installed
/// back. Those go straight to `prototypes import` instead, with no conversion.
pub struct ReadyPrototypes {
    /// Display name → the `.wlshm` file it lives in.
    pub items: Vec<(String, PathBuf)>,
    /// Deleted when this is dropped, when the files were unpacked from a zip.
    _scratch: Option<TempDir>,
}

/// A folder that removes itself.
pub struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A name for the picker: `Shimeji.Hornet.wlshm` reads as `Hornet`.
pub fn wlshm_name(path: &Path) -> String {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("character");
    stem.strip_prefix("Shimeji.").unwrap_or(stem).to_string()
}

/// Looks at a file the person picked and, if it already holds wl_shimeji prototypes,
/// returns them. `None` means "this is not that kind of archive", and the caller
/// converts it as before.
pub fn ready_prototypes(path: &Path) -> Result<Option<ReadyPrototypes>, String> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if ext == "wlshm" {
        return Ok(Some(ReadyPrototypes { items: vec![(wlshm_name(path), path.to_path_buf())], _scratch: None }));
    }
    if ext != "zip" {
        return Ok(None);
    }

    let file = std::fs::File::open(path).map_err(|e| format!("could not open {}: {e}", path.display()))?;
    let mut zip = match zip::ZipArchive::new(file) {
        Ok(z) => z,
        Err(_) => return Ok(None), // not a zip after all: let the converter explain
    };
    let inside: Vec<usize> = (0..zip.len())
        .filter(|i| {
            zip.by_index(*i)
                .map(|e| e.is_file() && e.name().to_lowercase().ends_with(".wlshm"))
                .unwrap_or(false)
        })
        .collect();
    if inside.is_empty() {
        return Ok(None);
    }

    let dir = crate::system::scratch_dir().join(format!("ready-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not create a folder for the archive: {e}"))?;
    let scratch = TempDir(dir.clone());

    let mut items = Vec::new();
    for i in inside {
        let mut entry = zip.by_index(i).map_err(|e| format!("could not read the archive: {e}"))?;
        // Only the file's own name is used, so an entry like "../../x.wlshm" cannot
        // write outside the folder.
        let name = Path::new(entry.name()).file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();
        if name.is_empty() {
            continue;
        }
        let out = dir.join(&name);
        let mut sink = std::fs::File::create(&out).map_err(|e| format!("could not unpack {name}: {e}"))?;
        std::io::copy(&mut entry, &mut sink).map_err(|e| format!("could not unpack {name}: {e}"))?;
        items.push((wlshm_name(&out), out));
    }
    items.sort_by_key(|(name, _)| name.to_lowercase());
    Ok(Some(ReadyPrototypes { items, _scratch: Some(scratch) }))
}

/// What an import came to.
#[derive(Debug, Default, PartialEq)]
pub struct ImportReport {
    pub imported: usize,
    /// One line for each file the engine turned down, or that did not end up in the collection.
    pub problems: Vec<String>,
    /// The overlay had to be restarted first, to make it forget characters that were removed (see `forgotten`).
    pub restarted: bool,
    /// How many of the characters that were on screen were put back after that restart.
    pub restored: usize,
}

/// How many files go to the engine in one call. A whole collection in one call worked (84 in 1.4 s, on an idle
/// engine) but says nothing until it ends and, when it goes wrong, nothing about where.
const IMPORT_CHUNK: usize = 20;
const IMPORT_SECONDS: u64 = 120;

/// What `shimejictl prototypes import` says about the files it was given.
///
/// It exits with 0 whether they went in or not: a file that could not be imported is only a log line on
/// stderr ("Failed to import prototype referenced by id 123: Invalid file format"), and one that is not a
/// prototype at all is a warning. An app that trusts the exit status reports success for an import that did
/// nothing, which is how a character could be "installed" and not be in the collection.
fn read_import_log(stderr: &str) -> (usize, Vec<String>) {
    let mut imported = 0;
    let mut problems = Vec::new();
    for line in stderr.lines() {
        let line = line.trim();
        if line.contains("Successfully imported prototype") {
            imported += 1;
        } else if let Some((_, why)) = line.split_once("Failed to import prototype") {
            let why = why.rsplit_once(": ").map(|(_, r)| r).unwrap_or(why).trim();
            problems.push(format!("the engine refused a file: {why}"));
        } else if let Some(rest) = line.split_once("File '").map(|(_, r)| r) {
            let (file, what) = rest.split_once('\'').unwrap_or((rest, ""));
            let file = Path::new(file).file_name().and_then(|n| n.to_str()).unwrap_or(file);
            if what.contains("not a valid wl_shimeji prototype") {
                problems.push(format!("{file}: not a wl_shimeji prototype"));
            } else if what.contains("does not exist") {
                problems.push(format!("{file}: the file is gone"));
            }
        }
    }
    (imported, problems)
}

/// Names the engine still holds in memory although their folders are gone: characters the app removed while the
/// overlay was running (there is no delete command, and `reload-all` brings the overlay down).
fn forgotten(listed: &[String], on_disk: &[String]) -> Vec<String> {
    let have: std::collections::HashSet<&str> = on_disk.iter().map(String::as_str).collect();
    listed.iter().filter(|n| !have.contains(n.as_str())).cloned().collect()
}

/// What the engine says about a failed call, without the command line in front of it (that is a wall of file names
/// nobody can read) — or that it said nothing.
fn reason_of(error: &str) -> String {
    let rest = error.split_once("` → ").map(|(_, r)| r).unwrap_or(error).trim();
    // The engine's own log prefixes ("ERROR:__main__:File ... does not exist") are noise.
    let rest = rest.replace("ERROR:__main__:", "").replace("INFO:__main__:", "").replace("WARNING:__main__:", "");
    let rest = rest.trim();
    if rest.is_empty() {
        return "it stopped without saying why; the overlay has probably crashed".to_string();
    }
    let last = rest.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or(rest).trim();
    if last.chars().count() > 160 { format!("{}…", last.chars().take(160).collect::<String>()) } else { last.to_string() }
}

/// Makes a running overlay forget the characters that were removed, by restarting it, and says who was on screen.
///
/// Found on the real engine: after a character is deleted, the running overlay keeps it in memory.
/// Installing it again with `import -f` then answers "Successfully imported" and writes NOTHING (the folder never
/// comes back, the list still shows the old entry), and with such an entry around a whole import can end with an
/// error. An overlay that starts fresh reads only what is on disk and imports normally. `Ok(None)`: nothing to
/// forget, nothing done.
fn forget_removed() -> Result<Option<Vec<(String, usize)>>, String> {
    // `prototypes list` would start an overlay that is not running, for nothing.
    if overlay_pid().is_none() {
        return Ok(None);
    }
    let listed = list_prototypes()?;
    if forgotten(&listed, &installed_names()).is_empty() {
        return Ok(None);
    }
    let installed = installed_names();
    let crowd: Vec<(String, usize)> = on_screen().unwrap_or_default().into_iter().filter(|(n, _)| installed.contains(n)).collect();
    let _ = run_timeout(&["stop"], 15);
    let began = Instant::now();
    while overlay_pid().is_some() && began.elapsed() < Duration::from_secs(6) {
        std::thread::sleep(Duration::from_millis(100));
    }
    if overlay_pid().is_some() {
        return Err("The engine still remembers characters you removed, and would not restart to forget them, so nothing was installed. Use Stop the overlay in Settings, then try again.".to_string());
    }
    Ok(Some(crowd))
}

/// Imports prototypes, a chunk at a time, and reports what really went in. `progress(done, total)` is called
/// before each chunk. Files that were already installed are replaced (the engine's `-f`), so callers keep the
/// ones on screen out of it.
pub fn import_prototypes(paths: &[PathBuf], progress: impl Fn(usize, usize)) -> Result<ImportReport, String> {
    let restart = forget_removed()?;
    let before = installed_names();
    let mut report = ImportReport { restarted: restart.is_some(), ..Default::default() };
    for (i, chunk) in paths.chunks(IMPORT_CHUNK).enumerate() {
        progress(i * IMPORT_CHUNK, paths.len());
        let owned: Vec<String> = chunk.iter().map(|p| p.display().to_string()).collect();
        let mut args: Vec<&str> = vec!["prototypes", "import", "-f"];
        args.extend(owned.iter().map(String::as_str));
        match run_timeout_full(&args, IMPORT_SECONDS) {
            Ok((_, log)) => {
                let (n, problems) = read_import_log(&log);
                report.imported += n;
                report.problems.extend(problems);
            }
            Err(e) if is_no_answer(&e) => return Err(e),
            Err(e) => report.problems.push(format!("the engine failed on {} file{}: {}", chunk.len(), if chunk.len() == 1 { "" } else { "s" }, reason_of(&e))),
        }
    }
    // Put back who was on screen. Where they stood is lost; that is the price of a restart.
    if let Some(crowd) = restart {
        if !crowd.is_empty() {
            match summon_batch(&crowd) {
                Ok(done) => report.restored = done.spawned,
                Err(e) => report.problems.push(format!("the characters that were on screen could not be put back: {}", reason_of(&e))),
            }
        }
    }
    // Trust the collection over the log: something that is not there now did not go in.
    let now = installed_names();
    for path in paths {
        let name = wlshm_name(path);
        if !before.contains(&name) && !now.contains(&name) {
            report.problems.push(format!("{name} was not added"));
        }
    }
    if report.imported == 0 && !report.problems.is_empty() {
        return Err(report.problems.join("\n"));
    }
    Ok(report)
}

/// List of prototypes.
///
/// Real output format (checked on wl_shimeji):
///
///   Available prototypes:
///   0: Shimeji
///   1: Hornet
///   2: .Hornet_Needle
///   5: Hive Queen Vespa
///
/// That is a header line, then "index: name". Names may contain spaces and start
/// with a dot (helper prototypes like .Hornet_Needle).
pub fn list_prototypes() -> Result<Vec<String>, String> {
    let out = run(&["prototypes", "list"])?;
    Ok(out
        .lines()
        .filter_map(|line| {
            let l = line.trim();
            if l.is_empty() {
                return None;
            }
            // The "Available prototypes:" header is skipped.
            let (idx, name) = l.split_once(':')?;
            if !idx.trim().chars().all(|c| c.is_ascii_digit()) || idx.trim().is_empty() {
                return None;
            }
            let name = name.trim();
            if name.is_empty() {
                None
            } else {
                Some(name.to_string())
            }
        })
        .collect())
}

/// Root of the wl_shimeji configuration.
pub fn config_root() -> PathBuf {
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(home).join(".local/share")
        });
    base.join("wl_shimeji")
}

/// The folder wl_shimeji keeps installed characters in, and where new ones go.
pub fn characters_dir() -> PathBuf {
    config_root().join("shimejis")
}

/// Folders where prototypes may live.
///
/// In practice there are two: `prototypes/` and `shimejis/`. One user had 12
/// prototypes but `prototypes/` held only two entries; the rest are stored in
/// `shimejis/`. So we search both.
pub fn prototype_dirs() -> Vec<PathBuf> {
    let root = config_root();
    vec![root.join("prototypes"), root.join("shimejis")]
}

/// A listing of what sits in the prototype folders (for diagnostics).
pub fn prototype_files() -> Vec<String> {
    let mut all = Vec::new();
    for dir in prototype_dirs() {
        let label = dir.display().to_string();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            let mut v: Vec<String> = entries
                .flatten()
                .map(|e| {
                    let n = e.file_name().to_string_lossy().to_string();
                    if e.path().is_dir() {
                        format!("{n}/")
                    } else {
                        n
                    }
                })
                .collect();
            v.sort();
            all.push(format!("{label}:"));
            if v.is_empty() {
                all.push("  (empty)".into());
            } else {
                all.extend(v.into_iter().map(|n| format!("  {n}")));
            }
        }
    }
    all
}

/// Names of installed prototypes from the files on disk (without running shimejictl).
/// `Shimeji.BMO` → `BMO`, `Shimeji..Hornet_Needle` → `.Hornet_Needle`.
pub fn installed_names_on_disk() -> Vec<String> {
    installed_times().into_keys().collect()
}

// ---------------------------------------------------------------------------
// Missing pictures
// ---------------------------------------------------------------------------
//
// A character whose actions refer to a picture that is not in its folder brings the
// overlay down the moment it is summoned ("Action iterator called while behavior is
// NULL", then SIGSEGV). Reproduced on a live overlay: removing just `shime1.qoi` or
// `shime4.qoi` from a working character is enough; removing the rarely used high
// numbers is not. The catalog has characters that lack pictures (Lestrade did, three
// of them), and "Summon all" took the overlay down for whoever had one.
//
// The cure that was verified: give every missing picture a copy of the nearest one
// that exists. The character then works, with fewer distinct poses. Nothing that
// exists is touched, files are only added. The overlay reads a character when it
// starts, so this has to happen while the overlay is not running.

/// Who was repaired while a given overlay was running. That overlay read the old, broken
/// version, so it must not be asked to show them (it would go down); a new overlay reads
/// the repaired files. Keyed by the overlay's process id.
static STALE: std::sync::Mutex<Option<(u32, std::collections::HashSet<String>)>> = std::sync::Mutex::new(None);

fn mark_stale(name: &str) {
    let Some(pid) = overlay_pid() else { return };
    let mut guard = STALE.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_mut() {
        Some((same, names)) if *same == pid => {
            names.insert(name.to_string());
        }
        _ => *guard = Some((pid, std::iter::once(name.to_string()).collect())),
    }
}

/// Everything the running overlay holds is out of date, because the files themselves
/// moved. What it already has in memory still works; new summons wait for its next start.
pub fn forget_stale() {
    if let Some(pid) = overlay_pid() {
        let names = installed_names().into_iter().collect();
        *STALE.lock().unwrap_or_else(|e| e.into_inner()) = Some((pid, names));
    }
}

/// True when the overlay that is running now was started before this character was repaired.
pub fn is_stale(name: &str) -> bool {
    let Some(pid) = overlay_pid() else { return false };
    let guard = STALE.lock().unwrap_or_else(|e| e.into_inner());
    matches!(guard.as_ref(), Some((same, names)) if *same == pid && names.contains(name))
}

/// The unpacked folder of an installed character.
fn prototype_folder(name: &str) -> Option<PathBuf> {
    // A name is one folder's name, never a path: nothing here may reach outside the two folders.
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', '\0']) {
        return None;
    }
    for dir in prototype_dirs() {
        for base in [dir.join(name), dir.join(format!("Shimeji.{name}"))] {
            if base.is_dir() {
                return Some(base);
            }
        }
    }
    None
}

/// The frame numbers (`shime12.qoi` is 12) that a character's actions use.
fn used_frames(actions_json: &str) -> std::collections::BTreeSet<u32> {
    let text = actions_json.to_ascii_lowercase();
    let mut out = std::collections::BTreeSet::new();
    let mut from = 0;
    while let Some(at) = text[from..].find("shime") {
        let digits_at = from + at + "shime".len();
        let digits: String = text[digits_at..].chars().take_while(|c| c.is_ascii_digit()).collect();
        let after = digits_at + digits.len();
        if !digits.is_empty() && (text[after..].starts_with(".qoi") || text[after..].starts_with(".png")) {
            if let Ok(n) = digits.parse() {
                out.insert(n);
            }
        }
        from = digits_at;
    }
    out
}

/// The frames a folder has: number -> file name.
fn present_frames(assets: &Path) -> std::collections::BTreeMap<u32, String> {
    let mut out = std::collections::BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(assets) else { return out };
    for e in entries.flatten() {
        let file = e.file_name().to_string_lossy().to_string();
        let lower = file.to_ascii_lowercase();
        let Some(stem) = lower.strip_suffix(".qoi").or_else(|| lower.strip_suffix(".png")) else { continue };
        if let Some(n) = stem.strip_prefix("shime").and_then(|d| d.parse::<u32>().ok()) {
            out.insert(n, file);
        }
    }
    out
}

/// Frames a character uses but does not have. Empty for a character that is complete,
/// and for one that is not an unpacked folder (nothing can be said about those).
pub fn missing_frames(name: &str) -> Vec<u32> {
    let Some(folder) = prototype_folder(name) else { return Vec::new() };
    let Ok(actions) = std::fs::read_to_string(folder.join("actions.json")) else { return Vec::new() };
    let have = present_frames(&folder.join("assets"));
    used_frames(&actions).into_iter().filter(|n| !have.contains_key(n)).collect()
}

/// Copies the nearest existing frame into every missing place. Returns which frames
/// were added.
pub fn repair_frames(name: &str) -> Result<Vec<u32>, String> {
    let gaps = missing_frames(name);
    if gaps.is_empty() {
        return Ok(gaps);
    }
    let folder = prototype_folder(name).ok_or_else(|| format!("{name} was not found"))?;
    let assets = folder.join("assets");
    let have = present_frames(&assets);
    if have.is_empty() {
        return Err(format!("{name} has no pictures at all"));
    }
    for &n in &gaps {
        let nearest = have.range(..n).next_back().or_else(|| have.range(n..).next()).map(|(_, file)| file.clone());
        let Some(from) = nearest else { continue };
        let ext = Path::new(&from).extension().and_then(|e| e.to_str()).unwrap_or("qoi").to_string();
        std::fs::copy(assets.join(&from), assets.join(format!("shime{n}.{ext}")))
            .map_err(|e| format!("could not repair {name}: {e}"))?;
    }
    mark_stale(name);
    Ok(gaps)
}

/// Repairs everything installed. Returns who needed it: (name, how many pictures).
pub fn repair_all() -> Vec<(String, usize)> {
    installed_names()
        .into_iter()
        .filter_map(|name| match repair_frames(&name) {
            Ok(added) if !added.is_empty() => Some((name, added.len())),
            _ => None,
        })
        .collect()
}

/// The names of everything installed, read from the prototype folders.
///
/// This used to ask `shimejictl prototypes list`. Every `shimejictl` call connects
/// to the overlay, makes it send its whole state (every prototype and every
/// character on screen), and *starts the overlay if it is not running*. Merely
/// opening the Collection did all that, over and over. The folders hold the same
/// list, so they are read directly.
pub fn installed_names() -> Vec<String> {
    installed_names_in(&prototype_dirs())
}

fn installed_names_in(dirs: &[PathBuf]) -> Vec<String> {
    let mut names = std::collections::BTreeSet::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for e in entries.flatten() {
            let path = e.path();
            let file = e.file_name().to_string_lossy().to_string();
            let is_archive = file.ends_with(".wlshm") && path.is_file();
            if !(path.is_dir() || is_archive) {
                continue;
            }
            let name = file.strip_prefix("Shimeji.").unwrap_or(&file);
            let name = name.strip_suffix(".wlshm").unwrap_or(name);
            if !name.is_empty() {
                names.insert(name.to_string());
            }
        }
    }
    names.into_iter().collect()
}

/// Prototype name → when it was installed (seconds since the epoch, by the folder's
/// creation date, otherwise its modification date). Lets the Collection show the newest first.
pub fn installed_times() -> std::collections::HashMap<String, u64> {
    let mut out = std::collections::HashMap::new();
    for dir in prototype_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let file = e.file_name().to_string_lossy().to_string();
            let name = file.strip_prefix("Shimeji.").unwrap_or(&file);
            let name = name.strip_suffix(".wlshm").unwrap_or(name);
            if name.is_empty() {
                continue;
            }
            let secs = e
                .metadata()
                .ok()
                .and_then(|m| m.created().or_else(|_| m.modified()).ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            out.insert(name.to_string(), secs);
        }
    }
    out
}

/// Size of a folder (bytes), recursively.
pub fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else { return 0 };
    entries
        .flatten()
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => dir_size(&e.path()),
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}

/// Prototype name → how much disk space it takes.
pub fn installed_sizes() -> std::collections::HashMap<String, u64> {
    let mut out = std::collections::HashMap::new();
    for dir in prototype_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let file = e.file_name().to_string_lossy().to_string();
            let name = file.strip_prefix("Shimeji.").unwrap_or(&file);
            let name = name.strip_suffix(".wlshm").unwrap_or(name);
            if !name.is_empty() {
                out.insert(name.to_string(), dir_size(&e.path()));
            }
        }
    }
    out
}

/// Whether an entry in the prototype folders is exactly this prototype.
///
/// Folders are named `Shimeji.{name}` (`Shimeji.BMO`, `Shimeji.Mosscreep (Orange)`,
/// `Shimeji..Hornet_Needle`), so `Path::file_stem()` does not work here: it cuts the
/// name at the last dot. We compare whole names and only exactly, otherwise
/// `Shimeji` would match every folder at once.
fn is_prototype_entry(path: &Path, file: &str, name: &str) -> bool {
    if file == name
        || file == format!("Shimeji.{name}")
        || file == format!("{name}.wlshm")
        || file == format!("Shimeji.{name}.wlshm")
    {
        return true;
    }

    // Fallback: the folder is named differently, but manifest.json holds the same
    // name that `prototypes list` shows.
    if path.is_dir() {
        if let Ok(text) = std::fs::read_to_string(path.join("manifest.json")) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                return v.get("display_name").and_then(|d| d.as_str()) == Some(name);
            }
        }
    }
    false
}

/// Deletes a prototype.
///
/// shimejictl has NO delete command: `prototypes` only supports
/// list / info / reload / reload-all / export / import. So we erase from disk.
/// Only entries found by listing the two prototype folders are removed, so a name
/// can never point anywhere else.
///
/// A prototype may be a file `Shimeji.{name}.wlshm` or a folder `{name}/`, and it
/// may sit in `prototypes/` or in `shimejis/`; we check every variant.
pub fn remove_prototype(name: &str) -> Result<String, String> {
    let dirs = prototype_dirs();
    let mut removed = Vec::new();

    for dir in &dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let file = path.file_name().and_then(|s| s.to_str()).unwrap_or("");

            if !is_prototype_entry(&path, file, name) {
                continue;
            }

            let res = if path.is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            };
            res.map_err(|e| format!("could not delete {}: {e}", path.display()))?;
            removed.push(file.to_string());
        }
    }

    if removed.is_empty() {
        return Err(format!(
            "Prototype file \"{name}\" not found.\n\nLooked in:\n{}\n\n\
             It may be a built-in prototype that ships with wl_shimeji, \
             and those cannot be deleted.",
            prototype_files().join("\n")
        ));
    }

    // This used to end with `shimejictl prototypes reload-all`, to make the overlay
    // forget what was deleted. Reproduced on a live overlay: that command brings the
    // overlay down (SIGABRT) every time it follows a deletion, whether or not anyone
    // of that character is on screen. Without it the overlay simply carries on; it
    // forgets the character the next time it starts, and the app never offers a
    // deleted character to be summoned.
    Ok(format!("deleted {}", removed.join(", ")))
}

// ---------------------------------------------------------------------------
// Characters on screen
// ---------------------------------------------------------------------------

pub fn summon(name: &str) -> Result<(), String> {
    let _guard = lock();
    let args = ["mascot", "summon", name];
    // Its own process group and a note of it: "Dismiss all" pressed while this is running
    // can end it at once instead of waiting for the second it takes.
    let child = clean_command("shimejictl")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|e| start_error("shimejictl", &e))?;
    *SUMMONING.lock().unwrap_or_else(|e| e.into_inner()) = Some(child.id());
    let out = child.wait_with_output().map_err(|e| format!("could not run shimejictl: {e}"))?;
    let stopped = SUMMONING.lock().unwrap_or_else(|e| e.into_inner()).take().is_none();
    if stopped {
        return Err(CANCELLED.to_string());
    }
    // A short pause: the overlay cannot keep up with back-to-back calls.
    std::thread::sleep(std::time::Duration::from_millis(120));
    if out.status.success() {
        Ok(())
    } else {
        Err(command_failure(&args, &String::from_utf8_lossy(&out.stderr)))
    }
}

/// A crowd being summoned, as far as "stop" is concerned.
///
/// A summon of many takes seconds, one connection or one process at a time, and everything
/// else waits for it. Pressing "Dismiss all" in the middle means "I have changed my mind":
/// it must not queue up behind the thing it is meant to undo. So a summon registers here,
/// `cancel_summon` raises the epoch, and every step of every path looks at it before it
/// starts the next character.
static CANCEL_EPOCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static BATCHES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

struct Batch {
    epoch: u64,
}

impl Batch {
    fn begin() -> Self {
        use std::sync::atomic::Ordering::SeqCst;
        BATCHES.fetch_add(1, SeqCst);
        Batch { epoch: CANCEL_EPOCH.load(SeqCst) }
    }

    /// Err(CANCELLED) once somebody has called it off.
    fn check(&self) -> Result<(), String> {
        if CANCEL_EPOCH.load(std::sync::atomic::Ordering::SeqCst) != self.epoch {
            Err(CANCELLED.to_string())
        } else {
            Ok(())
        }
    }
}

impl Drop for Batch {
    fn drop(&mut self) {
        BATCHES.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Removes everyone from the screen (does not delete from disk).
///
/// Removes every character with a single call.
///
/// What code inspection and the live system showed:
///   • without flags `mascot dismiss` waits for a mouse selection and removes nobody;
///   • `--all` also enters selection mode first (the condition in shimejictl is
///     `if arguments.select or arguments.id is None`) and removes everyone only
///     after it ends: a click or a long timeout, hence the 20+ s delay;
///   • the CLI does not expose mascot IDs (`environment info --id` is broken), so
///     the selection cannot be bypassed with `--id`.
/// The way out: in selection mode `KeyboardInterrupt` cancels the selection and
/// proceeds straight to `--all`. So we wait for shimejictl to print its prompt
/// and send it SIGINT; the whole operation takes ~0.3 s.
pub fn dismiss_all() -> Result<String, String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    // Without an overlay there is nobody to dismiss. The command would start one,
    // just to send it away again (and to find out it crashes).
    if !overlay_running() {
        return Ok("Nobody is on screen: the overlay is not running".to_string());
    }

    let _guard = lock();
    let mut child = clean_command("shimejictl")
        .args(["mascot", "dismiss", "--all"])
        // Without this Python buffers stdout in a pipe and the prompt is not visible.
        .env("PYTHONUNBUFFERED", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| start_error("`shimejictl mascot dismiss --all`", &e))?;

    let pid = child.id().to_string();
    let stdout = child.stdout.take().ok_or("no access to the process stdout")?;

    // Safety net: if the prompt never appears (the overlay is not responding),
    // do not hold the thread forever.
    let finished = Arc::new(AtomicBool::new(false));
    {
        let (finished, pid) = (finished.clone(), pid.clone());
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(10));
            if !finished.load(Ordering::SeqCst) {
                let _ = Command::new("kill").args(["-KILL", &pid]).status();
            }
        });
    }

    let mut transcript = String::new();
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        transcript.push_str(&line);
        transcript.push('\n');
        if line.contains("Select mascot to dismiss") {
            // Give Python time to reach dispatch_events, otherwise SIGINT arrives before
            // the try block and the process just dies.
            std::thread::sleep(std::time::Duration::from_millis(80));
            let _ = Command::new("kill").args(["-INT", &pid]).status();
            break;
        }
    }

    let status = child
        .wait()
        .map_err(|e| format!("mascot dismiss did not finish: {e}"))?;
    finished.store(true, Ordering::SeqCst);

    if !status.success() {
        let mut err = String::new();
        if let Some(mut e) = child.stderr.take() {
            let _ = e.read_to_string(&mut err);
        }
        let detail = format!("{transcript}{err}");
        let detail = detail.trim();
        let plain = friendly(detail);
        return Err(if plain == detail {
            format!("`shimejictl mascot dismiss --all` → {status}\n{detail}")
        } else {
            plain
        });
    }

    Ok("mascot dismiss --all".to_string())
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

/// What the overlay can use for window interaction.
///
/// Window interaction, window throwing and global cursor position work only
/// through a compositor-specific plugin (a `.so` in the plugins directory).
/// Upstream ships one official plugin, for KDE KWin; nothing exists for Niri.
#[derive(Serialize, Clone, Debug)]
pub struct PluginStatus {
    pub desktop: String,
    /// File names of the plugins found.
    pub plugins: Vec<String>,
}

pub fn plugin_status() -> PluginStatus {
    let mut dirs: Vec<PathBuf> = Vec::new();

    // A custom location from the overlay config wins.
    if let Ok(text) = std::fs::read_to_string(config_root().join("shimeji-overlayd.conf")) {
        for line in text.lines() {
            if let Some(v) = line.trim().strip_prefix("plugins_location=") {
                dirs.push(PathBuf::from(v.trim()));
            }
        }
    }
    dirs.push(config_root().join("plugins"));
    for d in [
        "/usr/lib/wl_shimeji/plugins",
        "/usr/lib/wl_shimeji",
        "/usr/share/wl_shimeji/plugins",
        "/usr/lib/shimeji/plugins",
    ] {
        dirs.push(PathBuf::from(d));
    }

    let mut plugins: Vec<String> = dirs
        .iter()
        .filter_map(|d| std::fs::read_dir(d).ok())
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".so"))
        .collect();
    plugins.sort();
    plugins.dedup();

    PluginStatus {
        desktop: std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
        plugins,
    }
}

/// Path of the overlay socket.
fn overlay_socket() -> PathBuf {
    PathBuf::from(std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string()))
        .join("shimeji-overlayd.sock")
}

/// The overlay's process id, if it is running.
///
/// The socket file is not a good test: it can outlive a crashed overlay, and the
/// app then said "running" while nothing was there. The process list is the truth.
/// (`comm` is cut to 15 characters, "shimeji-overlay", so the command line decides.)
pub fn overlay_pid() -> Option<u32> {
    for entry in std::fs::read_dir("/proc").ok()?.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else { continue };
        let dir = entry.path();
        match std::fs::read_to_string(dir.join("comm")) {
            Ok(comm) if comm.trim() == "shimeji-overlay" => {}
            _ => continue,
        }
        let Ok(cmdline) = std::fs::read(dir.join("cmdline")) else { continue };
        let first = cmdline.split(|b| *b == 0).next().unwrap_or(&[]);
        let is_it = Path::new(std::str::from_utf8(first).unwrap_or("")).file_name().map(|n| n == "shimeji-overlayd").unwrap_or(false);
        if is_it {
            return Some(pid);
        }
    }
    None
}

/// Whether the overlay is running. `config` never starts it, and without it
/// shimejictl prints the parameters from the file in a different format.
pub fn overlay_running() -> bool {
    overlay_pid().is_some()
}

/// Running and listening: it can be talked to.
pub fn overlay_ready() -> bool {
    overlay_running() && overlay_socket().exists()
}

/// One crash of the overlay that the system has on record.
#[derive(Serialize, Debug, PartialEq)]
pub struct Crash {
    /// Milliseconds since the epoch.
    pub time_ms: u64,
    /// The signal that ended it (11 is a segmentation fault).
    pub signal: i64,
    /// This app ended it, because the desktop had dropped its connection and it was running on
    /// without one (see `look_at_log`).
    pub cut_off: bool,
}

/// The crashes of `shimeji-overlayd` since `since_ms`.
///
/// The overlay also quits, normally and by itself, when the screen is empty, and from
/// the outside that looks the same as a crash. Two things tell them apart, and both are
/// used: how the process this app started ended (always available, exact), and the
/// system's crash list (also catches an overlay somebody else started, but only exists
/// where a crash handler is installed — plenty of systems have none). An error means
/// neither knew anything, and the caller can only guess.
pub fn overlay_crashes(since_ms: u64) -> Result<Vec<Crash>, String> {
    let mut found = seen_by_us(last_ending(since_ms), CUT_OFF_AT.load(Ordering::SeqCst), since_ms);
    let ours = !found.is_empty();

    let listed = Command::new("coredumpctl")
        .args(["list", "--no-pager", "--json=short"])
        .stdin(Stdio::null())
        .output()
        .map_err(|_| "there is no crash list on this system".to_string())
        .and_then(|out| crashes_from_json(&String::from_utf8_lossy(&out.stdout), since_ms));

    match listed {
        Ok(list) => {
            // The same crash can be in both. Ours is exact, so anything within a few
            // seconds of it is taken to be the same one.
            for c in list {
                if !found.iter().any(|f| f.time_ms.abs_diff(c.time_ms) < 5000) {
                    found.push(c);
                }
            }
            Ok(found)
        }
        // No crash list on this system: what we saw ourselves is all there is, and when
        // the overlay we started ended cleanly, that is an answer too, not a failure.
        Err(e) => {
            if ours || last_ending(since_ms).is_some() {
                Ok(found)
            } else {
                Err(e)
            }
        }
    }
}

/// The crashes this app knows of first-hand: how the overlay it started ended, and any overlay it
/// ended itself because it had lost the desktop (`cut` is when, 0 for never).
fn seen_by_us(ending: Option<Ending>, cut: u64, since_ms: u64) -> Vec<Crash> {
    let mut found: Vec<Crash> = Vec::new();
    if let Some(Ending { when_ms, signal: Some(signal) }) = ending {
        found.push(Crash { time_ms: when_ms, signal: signal as i64, cut_off: false });
    }
    // The exit status of an overlay we killed says only "a signal ended it". Here the reason is
    // known, and to the person it is a crash all the same.
    if cut != 0 && cut >= since_ms {
        match found.iter_mut().find(|c| c.time_ms.abs_diff(cut) < 5000) {
            Some(c) => c.cut_off = true,
            None => found.push(Crash { time_ms: cut, signal: 9, cut_off: true }),
        }
    }
    found
}

fn crashes_from_json(text: &str, since_ms: u64) -> Result<Vec<Crash>, String> {
    if text.trim().is_empty() {
        return Ok(Vec::new()); // "No coredumps found": nothing has crashed
    }
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("could not read the crash list: {e}"))?;
    Ok(v.as_array()
        .map(|list| {
            list.iter()
                .filter(|e| e["exe"].as_str().map(|x| x.ends_with("shimeji-overlayd")).unwrap_or(false))
                .filter_map(|e| Some(Crash { time_ms: e["time"].as_u64()? / 1000, signal: e["sig"].as_i64().unwrap_or(0), cut_off: false }))
                .filter(|c| c.time_ms >= since_ms)
                .collect()
        })
        .unwrap_or_default())
}

/// How the last overlay this app started came to an end.
///
/// `coredumpctl` knows about crashes only where a crash handler is installed, and many
/// systems have none: the user's machine this was written on reported "No coredumps
/// found" for a segmentation fault we had just watched happen. But the app started that
/// process, so it can simply wait for it and read its exit status, which always says
/// whether a signal killed it.
#[derive(Clone, Copy, Debug)]
pub struct Ending {
    /// Milliseconds since the epoch.
    pub when_ms: u64,
    /// The signal that killed it (11 = segmentation fault), or None for a normal exit.
    pub signal: Option<i32>,
}

static LAST_ENDING: std::sync::Mutex<Option<Ending>> = std::sync::Mutex::new(None);

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn remember_ending(status: std::process::ExitStatus) {
    use std::os::unix::process::ExitStatusExt;
    let ending = Ending { when_ms: now_ms(), signal: status.signal() };
    if let Ok(mut slot) = LAST_ENDING.lock() {
        *slot = Some(ending);
    }
}

/// How the overlay this app started ended, if it ended after `since_ms`.
pub fn last_ending(since_ms: u64) -> Option<Ending> {
    let ending = (*LAST_ENDING.lock().ok()?)?;
    (ending.when_ms >= since_ms).then_some(ending)
}

/// Where what the overlay prints is kept, so a crash can be looked into afterwards.
pub fn overlay_log_path() -> PathBuf {
    crate::library::data_dir().join("overlay.log")
}

/// Text without terminal colour codes (`ESC [ 93 m`), which the overlay prints even into a file.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for d in chars.by_ref() {
                if d.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// How much of the end of the log is ever read. Only the end is of use, and the log can be enormous:
/// an overlay that has lost its desktop wrote two gigabytes of one line in a hundred seconds, and
/// reading all of it for the reason of a crash held the recovery up by minutes.
const LOG_WINDOW: u64 = 1024 * 1024;

/// The end of a file, as text: at most `max` bytes, starting at the beginning of a line.
/// Empty when there is no such file.
fn read_tail(path: &Path, max: u64) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else { return String::new() };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(max);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return String::new();
    }
    let mut bytes = Vec::new();
    if file.take(max).read_to_end(&mut bytes).is_err() {
        return String::new();
    }
    let text = String::from_utf8_lossy(&bytes).into_owned();
    // Starting in the middle of a line would show half of one.
    match (start > 0, text.find('\n')) {
        (true, Some(i)) => text[i + 1..].to_string(),
        (true, None) => String::new(),
        _ => text,
    }
}

/// A log line without its `[hh:mm:ss.mmm]` stamp, which is what tells a repeat from a new line.
fn without_stamp(line: &str) -> &str {
    let plain = line.trim_start_matches('\u{1b}');
    match (plain.starts_with('['), plain.find(']')) {
        (true, Some(i)) => &plain[i + 1..],
        _ => plain,
    }
}

/// The last lines of that log, readable: no colour codes, without the hundreds of harmless
/// "Unknown function" lines every character prints when it is first loaded, and with the same
/// line printed again and again shown once, and how many times.
pub fn overlay_log_tail(lines: usize) -> String {
    log_tail_of(&overlay_log_path(), lines)
}

fn log_tail_of(path: &Path, lines: usize) -> String {
    let text = strip_ansi(&read_tail(path, LOG_WINDOW));
    let mut shown: Vec<(&str, usize)> = Vec::new();
    for line in text.lines().filter(|l| !l.contains("mascot_config_parser") && !l.contains("Unknown function") && !l.trim().is_empty()) {
        match shown.last_mut() {
            Some((last, n)) if without_stamp(last) == without_stamp(line) => *n += 1,
            _ => shown.push((line, 1)),
        }
    }
    let from = shown.len().saturating_sub(lines);
    shown[from..].iter().map(|(l, n)| if *n > 1 { format!("{l}  (repeated {n} times)") } else { (*l).to_string() }).collect::<Vec<_>>().join("\n")
}

/// The character the overlay last complained about (`<Mascot:Shimeji.Lestrade:5> ...
/// behavior is NULL`): the one that was on screen when it went down.
pub fn overlay_culprit() -> Option<String> {
    let text = strip_ansi(&read_tail(&overlay_log_path(), LOG_WINDOW));
    text.lines().rev().find(|l| l.contains("behavior is NULL")).and_then(|l| {
        let start = l.find("<Mascot:")? + "<Mascot:".len();
        let rest = &l[start..];
        let name = &rest[..rest.rfind(':')?.min(rest.find('>')?)];
        Some(name.strip_prefix("Shimeji.").unwrap_or(name).to_string())
    })
}

// ------------------------------------------------ an overlay that has lost the desktop

/// What the overlay prints, over and over, once the desktop has dropped its connection.
const CONNECTION_LOST: &str = "Wayland connection closed";

/// A log this large means something is wrong, and it is emptied rather than left to fill the disk.
const LOG_LIMIT: u64 = 32 * 1024 * 1024;

/// When this app last ended an overlay that had lost its connection (milliseconds since the epoch; 0 for never).
static CUT_OFF_AT: AtomicU64 = AtomicU64::new(0);

/// Whether the end of the log is nothing but that complaint. The overlay does not recover from
/// it, and prints it as fast as it can.
fn connection_lost(tail: &str) -> bool {
    const ENOUGH: usize = 30;
    let mut seen = 0;
    for line in tail.lines().rev().filter(|l| !l.trim().is_empty()).take(ENOUGH) {
        if !line.contains(CONNECTION_LOST) {
            return false;
        }
        seen += 1;
    }
    seen == ENOUGH
}

/// The log with the repeated complaint shown once and counted, and the rest kept (the last 256 KB of it).
fn collapse_complaint(text: &str) -> String {
    let mut kept: Vec<&str> = Vec::new();
    let mut repeats = 0u64;
    for line in text.lines() {
        if line.contains(CONNECTION_LOST) {
            repeats += 1;
            if repeats > 1 {
                continue;
            }
        }
        kept.push(line);
    }
    let (mut from, mut size) = (kept.len(), 0);
    while from > 0 && size + kept[from - 1].len() < 256 * 1024 {
        from -= 1;
        size += kept[from].len() + 1;
    }
    let mut out = kept[from..].join("\n");
    out.push('\n');
    if repeats > 1 {
        out.push_str(&format!("--- \"{CONNECTION_LOST}\" was printed {repeats} times, and was cut down to one line by Menagerie ---\n"));
    }
    out
}

/// Whether the process's standard output is this file. The kernel keeps the answer in `/proc`.
fn writes_to(pid: u32, log: &Path) -> bool {
    match (std::fs::canonicalize(log), std::fs::read_link(format!("/proc/{pid}/fd/1"))) {
        (Ok(mine), Ok(theirs)) => mine == theirs,
        _ => false,
    }
}

/// Ends a process at once. For an overlay that is only spinning there is nothing left to save.
fn end_process(pid: u32) {
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    unsafe {
        kill(pid as i32, 9);
    }
}

/// One look at the overlay's log, for two things.
///
/// **A lost desktop.** The desktop can drop a client it finds fault with — niri did, with
/// "Data too big for buffer (1048576 + 8 > 1048576)", while the overlay drew a crowd on a machine
/// that was very busy. The overlay then neither quits nor reconnects: it prints "Wayland
/// connection closed" as fast as it can (over a hundred thousand lines a second, a whole core and
/// fifteen megabytes a second to the disk) until it happens to fall over, which took it a hundred
/// seconds and two gigabytes. All that time the characters are gone and the process is alive, so
/// nothing looked wrong and nothing was brought back. Here it is ended, so the recovery starts at once.
///
/// **A log that grows for ever** is emptied once it passes `LOG_LIMIT`, whatever it is full of.
///
/// Only an overlay whose output goes to this very file is judged by it.
fn look_at_log(log: &Path) {
    look_at_log_of(log, &overlay_pid);
}

/// `look_at_log`, with the way of finding the overlay's process handed in, so that it can be tried
/// on a stand-in without going anywhere near a real overlay.
fn look_at_log_of(log: &Path, find_overlay: &dyn Fn() -> Option<u32>) {
    let Ok(meta) = std::fs::metadata(log) else { return };
    let writing_now = meta.modified().ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age < Duration::from_secs(3));

    if writing_now && connection_lost(&read_tail(log, 16 * 1024)) {
        if let Some(pid) = find_overlay() {
            if writes_to(pid, log) {
                end_process(pid);
                CUT_OFF_AT.store(now_ms(), Ordering::SeqCst);
                // Nothing may be writing while the log is rewritten.
                let began = Instant::now();
                while find_overlay() == Some(pid) && began.elapsed() < Duration::from_secs(3) {
                    std::thread::sleep(Duration::from_millis(50));
                }
                if let Ok(file) = std::fs::File::open(log) {
                    let mut bytes = Vec::new();
                    // Bounded: at fifteen megabytes a second, even a slow look sees tens of them, not thousands.
                    if file.take(256 * 1024 * 1024).read_to_end(&mut bytes).is_ok() {
                        let _ = std::fs::write(log, collapse_complaint(&String::from_utf8_lossy(&bytes)));
                    }
                }
                return;
            }
        }
    }

    if meta.len() > LOG_LIMIT {
        // The overlay appends, so it goes on writing at the new end without noticing.
        if let Ok(file) = std::fs::OpenOptions::new().write(true).open(log) {
            let _ = file.set_len(0);
        }
        if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(log) {
            let _ = writeln!(file, "--- Menagerie emptied this log: it had passed {} MB ---", LOG_LIMIT / 1024 / 1024);
        }
    }
}

/// Looks at the log every couple of seconds for as long as the app runs. It has a thread of its
/// own so that it goes on when the window is hidden, when nothing on the page is asking.
pub fn watch_log() {
    std::thread::spawn(|| loop {
        std::thread::sleep(Duration::from_secs(2));
        look_at_log(&overlay_log_path());
    });
}

unsafe extern "C" {
    fn socket(domain: i32, kind: i32, protocol: i32) -> i32;
    fn connect(fd: i32, address: *const u8, length: u32) -> i32;
    fn close(fd: i32) -> i32;
}

/// Connects to the overlay's socket and returns the connection, or None when it does not accept yet. The overlay's
/// socket is a SOCK_SEQPACKET one, which the standard library cannot open. Close-on-exec, so no program the app
/// starts later inherits it and keeps the overlay alive by accident.
fn attach(path: &Path) -> Option<i32> {
    use std::os::unix::ffi::OsStrExt;
    let bytes = path.as_os_str().as_bytes();
    if bytes.len() > 107 {
        return None;
    }
    let mut address = [0u8; 110];
    address[..2].copy_from_slice(&1u16.to_ne_bytes()); // AF_UNIX
    address[2..2 + bytes.len()].copy_from_slice(bytes);
    // SOCK_SEQPACKET (5) | SOCK_CLOEXEC (0o2000000)
    let fd = unsafe { socket(1, 5 | 0o2000000, 0) };
    if fd < 0 {
        return None;
    }
    if unsafe { connect(fd, address.as_ptr(), (2 + bytes.len() + 1) as u32) } != 0 {
        unsafe { close(fd) };
        return None;
    }
    Some(fd)
}

fn close_fd(fd: i32) {
    unsafe { close(fd) };
}

/// Starts the overlay the way the login file does (`shimeji-overlayd`, nobody
/// attached to it) and waits until it is answering. Any `shimejictl` command would
/// start it too, as a side effect, which is how it kept being started (and
/// crashing) without anyone asking; now it happens on purpose, once, and what it
/// prints goes to a log.
///
/// It is given a process group of its own, so closing the terminal the app was
/// started from does not take it down, and it keeps running if the app is closed.
pub fn start_overlay() -> Result<u32, String> {
    if let Some(pid) = overlay_pid() {
        if overlay_socket().exists() {
            return Ok(pid);
        }
    }

    let log_path = overlay_log_path();
    let mut last_problem = String::new();
    for attempt in 0..2 {
        // An overlay that is on its way out (one a `shimejictl` call started by itself quits ~1.4 s after with nobody
        // attached; one that was just stopped takes a moment) still holds the socket. A new one started now sees it
        // and ends at once with status 0: "it stopped right after starting". Wait for the old one to be gone.
        let waited = Instant::now();
        while overlay_pid().is_some() && !overlay_socket().exists() && waited.elapsed() < Duration::from_secs(4) {
            std::thread::sleep(Duration::from_millis(100));
        }
        if let Some(pid) = overlay_pid() {
            if overlay_socket().exists() {
                return Ok(pid);
            }
        }
        // A leftover socket from a crash would make the new one think another is running.
        if overlay_pid().is_none() {
            let _ = std::fs::remove_file(overlay_socket());
        }
        if let Some(dir) = log_path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        // Do not let the log grow for ever.
        if std::fs::metadata(&log_path).map(|m| m.len() > 256 * 1024).unwrap_or(false) {
            let _ = std::fs::remove_file(&log_path);
        }
        let log = std::fs::OpenOptions::new().create(true).append(true).open(&log_path).map_err(|e| format!("could not open {}: {e}", log_path.display()))?;
        let _ = writeln!(&log, "--- started by Menagerie (attempt {}) ---", attempt + 1);
        let err_log = log.try_clone().map_err(|e| format!("could not open the log: {e}"))?;

        let mut child = clean_command("shimeji-overlayd")
            .stdin(Stdio::null())
            .stdout(log)
            .stderr(err_log)
            .process_group(0)
            .spawn()
            .map_err(|e| start_error("shimeji-overlayd", &e))?;
        let pid = child.id();

        let began = Instant::now();
        let mut alive = false;
        while began.elapsed() < Duration::from_secs(6) {
            if let Ok(Some(status)) = child.try_wait() {
                last_problem = format!("it stopped right after starting ({status})");
                break;
            }
            // The moment its socket accepts, someone must be connected: an overlay nobody has connected to ends
            // as soon as it has read the characters. With a big collection that takes seconds and the old
            // "wait, then look" was enough; with a small one it is over in a fifth of a second, and the overlay
            // "would not start" for whoever had few characters. Polled fast for that reason.
            if overlay_socket().exists() {
                if let Some(fd) = attach(&overlay_socket()) {
                    // Still there a moment later? A start that crashes does it within a second.
                    std::thread::sleep(Duration::from_millis(700));
                    match child.try_wait() {
                        Ok(None) => alive = true,
                        _ => last_problem = "it stopped right after starting".to_string(),
                    }
                    // Kept for a while so the caller (a summon, usually) has time to connect itself; then the
                    // overlay may end when nothing is asked of it, as it always did.
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_secs(20));
                        close_fd(fd);
                    });
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        if !alive && last_problem.is_empty() {
            last_problem = "it did not start listening".to_string();
        }

        if alive {
            // Waiting for it reaps it (a crash leaves no zombie behind) and, more useful,
            // tells us exactly how it ended.
            std::thread::spawn(move || {
                if let Ok(status) = child.wait() {
                    remember_ending(status);
                }
            });
            return Ok(pid);
        }
        let _ = child.kill();
        let _ = child.wait();
    }

    let tail = overlay_log_tail(6);
    Err(format!(
        "The overlay would not start: {last_problem}.{}",
        if tail.is_empty() { String::new() } else { format!("\nIts log says:\n{tail}") }
    ))
}

/// Numeric layer values in the config file (wlr-layer-shell).
const LAYERS: [&str; 4] = ["background", "bottom", "top", "overlay"];

/// Overlay settings.
///
/// Two output formats of `shimejictl config list` (checked on a live system):
///
///   overlay running:
///     Config:
///       - Multiplication (BREEDING) = true
///       - Overlay layer (WLR_SHELL_LAYER) = overlay
///
///   overlay stopped (the config file is read, enums are numbers):
///     Config:
///     BREEDING: true
///     WLR_SHELL_LAYER: 3
pub fn config_list() -> Result<Vec<ConfigOption>, String> {
    Ok(parse_config(&run(&["config", "list"])?))
}

fn parse_config(out: &str) -> Vec<ConfigOption> {
    out.lines()
        .filter_map(|line| {
            let l = line.trim();

            // Live format: "- Label (KEY) = value"
            if let Some((left, value)) = l.trim_start_matches('-').trim().split_once('=') {
                let left = left.trim();
                let (open, close) = (left.rfind('(')?, left.rfind(')')?);
                if close > open {
                    let key = left[open + 1..close].trim().to_string();
                    if !key.is_empty() {
                        return Some(ConfigOption {
                            key,
                            label: left[..open].trim().to_string(),
                            value: value.trim().to_string(),
                            live: true,
                        });
                    }
                }
                return None;
            }

            // Local format: "KEY: value"
            let (key, value) = l.split_once(':')?;
            let key = key.trim();
            if key.is_empty() || !key.chars().all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit()) {
                return None;
            }
            let mut value = value.trim().to_string();
            if key == "WLR_SHELL_LAYER" {
                if let Some(name) = value.parse::<usize>().ok().and_then(|n| LAYERS.get(n)) {
                    value = name.to_string();
                }
            }
            Some(ConfigOption { key: key.to_string(), label: key.to_string(), value, live: false })
        })
        .collect()
}

/// What the overlay holds after a setting was written.
#[derive(Serialize, Debug)]
pub struct SetOutcome {
    /// The value the overlay has now. Usually what was asked for.
    pub value: String,
    /// The overlay chose something else: the number was outside the range it accepts
    /// (scale, for one, goes no higher than 2), or it ignores this setting altogether.
    pub adjusted: bool,
}

pub fn config_set(key: &str, value: &str) -> Result<SetOutcome, String> {
    let mut value = value.to_string();

    // Without the overlay `config set` writes the value to the file as is, and there
    // enums are numbers. We do not risk corrupting the config with a string the overlay cannot read.
    if !overlay_running() {
        match key {
            "WLR_SHELL_LAYER" => {
                if let Some(n) = LAYERS.iter().position(|l| *l == value) {
                    value = n.to_string();
                }
            }
            "WINDOW_THROW_POLICY" if value.parse::<i64>().is_err() => {
                return Err("The window throw policy can only be changed while the overlay is running \
                            (summon anyone from the Scene tab)."
                    .to_string());
            }
            _ => {}
        }
    }

    run(&["config", "set", key, &value])?;

    // The overlay answers "done" even when it then keeps something else: it clamps
    // numbers to its own range (scale stops at 2), and it ignores the window throw
    // policy entirely. So a running overlay is asked what it holds now, and that — not
    // what we asked for — is what the app shows.
    if overlay_running() {
        if let Ok(now) = run(&["config", "get", key]) {
            let held = now.lines().find_map(|l| l.split_once('=').map(|(_, v)| v.trim().to_string()));
            if let Some(held) = held {
                let adjusted = !same_value(&value, &held);
                return Ok(SetOutcome { value: held, adjusted });
            }
        }
    }
    Ok(SetOutcome { value, adjusted: false })
}

/// Whether the engine's factory size should be swapped for normal size. wl_shimeji ships with `mascot_scale=2`, which is
/// HALF size (it divides by it), so on a new machine every character came out tiny and the Size slider opened at x0.5.
/// Only on a machine that has none of our characters yet, only while the value is still the factory one, and only once:
/// somebody who chose x0.5 keeps it.
fn wants_normal_size(held: &str, has_characters: bool, already_done: bool) -> bool {
    !already_done && !has_characters && held.trim().parse::<f64>().is_ok_and(|v| (v - 2.0).abs() < 1e-6)
}

/// First start on a new machine: normal size instead of the engine's half size. See `wants_normal_size`.
pub fn first_run_defaults() {
    const DONE: &str = "size-default-applied";
    let done = crate::prefs::load().contains_key(DONE);
    if done {
        return;
    }
    let held = run(&["config", "get", "MASCOT_SCALE"])
        .ok()
        .and_then(|out| out.lines().find_map(|l| l.split_once('=').map(|(_, v)| v.trim().to_string())));
    // Not answered (no config yet, engine missing): try again next start, do not mark it done.
    let Some(held) = held else { return };
    if wants_normal_size(&held, !installed_names().is_empty(), false) {
        let _ = config_set("MASCOT_SCALE", "1");
    }
    let _ = crate::prefs::set(DONE, Some("1"));
}

/// "0.30" and "0.300000" are the same number; "Top" and "top" the same word.
fn same_value(wanted: &str, held: &str) -> bool {
    let (a, b) = (wanted.trim(), held.trim());
    match (a.parse::<f64>(), b.parse::<f64>()) {
        (Ok(x), Ok(y)) => (x - y).abs() < 1e-4,
        _ => a.eq_ignore_ascii_case(b),
    }
}

// Who is on screen, and dismissing one specific character.
//
// The `shimejictl` command line cannot do either: `mascot` only offers summon /
// dismiss / set-behavior, `dismiss` without `--all` waits for a mouse click, and
// `environment info --id N`, which would print the mascots with their IDs,
// compares a string with a number and always answers "Invalid environment id".
// `dismiss --id N` does exist, but nothing hands out the IDs.
//
// The client code inside `shimejictl` itself does know them: when it connects, the
// overlay sends the full state (every mascot with its ID and prototype), and
// `Mascot.dismiss()` removes exactly one. So the helper below loads that script
// as a module (its `main` is guarded, importing runs nothing) and uses its
// `Client` directly. It never starts the overlay.
//
// This leans on the internals of one particular wl_shimeji build (checked on
// 0.0.2.r103). If a future version changes them the helper fails with a clear
// message and the UI falls back to counting what it summoned itself.
const HELPER: &str = r#"
import importlib.machinery, importlib.util, json, os, random, shutil, socket, sys, time

def load():
    path = shutil.which("shimejictl")
    if not path:
        raise SystemExit("shimejictl was not found")
    loader = importlib.machinery.SourceFileLoader("shimejictl_mod", os.path.realpath(path))
    spec = importlib.util.spec_from_loader("shimejictl_mod", loader)
    mod = importlib.util.module_from_spec(spec)
    sys.modules["shimejictl_mod"] = mod   # dataclasses look the module up by name
    loader.exec_module(mod)
    sock = os.path.join(os.environ.get("XDG_RUNTIME_DIR") or "/tmp", "shimeji-overlayd.sock")
    client = mod.Client(sock, {"start": False})
    return mod, client

def name_of(m):
    n = m.prototype.name
    return n[len("Shimeji."):] if n.startswith("Shimeji.") else n

try:
    mod, client = load()
    cmd = sys.argv[1]
    if cmd == "list":
        print(json.dumps({"mascots": [{"id": m.id, "name": name_of(m)} for m in mod.mascots.values()]}))
    elif cmd == "dismiss":
        target, mode = sys.argv[2], sys.argv[3]
        hits = sorted((m for m in mod.mascots.values() if name_of(m) == target), key=lambda m: m.id)
        if mode == "one":
            hits = hits[-1:]          # the most recently summoned one
        for m in hits:
            m.dismiss()
        time.sleep(0.25)              # let the packets go out before we disconnect
        print(json.dumps({"dismissed": len(hits)}))
    elif cmd == "spawn":
        # Everyone in one connection. Each `shimejictl summon` is a new process and a new
        # connection, and each connection makes the overlay send its whole state again;
        # with dozens of characters that is slow and, it seems, what brings the overlay
        # down. Here a character is sent, and the next one only after the overlay
        # confirms the last one appeared, so it is never outrun.
        items = json.loads(sys.stdin.read())          # [[name, count], ...]
        envs = list(mod.environments.values())
        if not envs:
            raise SystemExit("the overlay has no screen to put characters on")
        client.socket.settimeout(1.5)
        confirmations = True
        spawned, missing = 0, []
        for name, count in items:
            proto = None
            for p in mod.prototypes.values():
                if name in (p.name, p.display_name, "Shimeji." + name):
                    proto = p
                    break
            if proto is None:
                proto = mod.prototype_find(name)
            if proto is None:
                missing.append(name)
                continue
            for _ in range(int(count)):
                env = random.choice(envs)
                x = 64 + random.randint(0, max(0, env.width - 128))
                before = len(mod.mascots)
                client.queue_packet(mod.Spawn(proto.id, env.id, x, 128, ""))
                spawned += 1
                if confirmations:
                    try:
                        client.dispatch_events(until=lambda: len(mod.mascots) > before)
                    except (TimeoutError, socket.timeout):
                        # No confirmation seen. Do not wait a second and a half for every one
                        # of the rest: fall back to a short fixed pause.
                        confirmations = False
                else:
                    time.sleep(0.06)
        time.sleep(0.3)                               # let the last packets leave before we disconnect
        print(json.dumps({"spawned": spawned, "missing": missing}))
    else:
        raise SystemExit("unknown command")
except SystemExit as e:
    sys.stderr.write(str(e.code) if e.code not in (None, 0, 1) else "could not talk to the overlay")
    sys.exit(1)
except Exception as e:
    sys.stderr.write(f"{type(e).__name__}: {e}")
    sys.exit(1)
"#;

fn run_helper(args: &[&str]) -> Result<serde_json::Value, String> {
    run_helper_with(args, None, 10)
}

/// The summon that is running right now, so it can be called off. Summoning a crowd takes
/// seconds, and during those seconds every other command waits for the lock — including
/// "Dismiss all", which is exactly what a person presses when they realise they asked for
/// too many. Rather than make them wait for the thing they want stopped, the process group
/// is remembered here and `cancel_summon` ends it.
static SUMMONING: std::sync::Mutex<Option<u32>> = std::sync::Mutex::new(None);

/// Stops a summon that is in flight, whichever way it is being done. Says whether there was one.
pub fn cancel_summon() -> bool {
    use std::sync::atomic::Ordering::SeqCst;
    let batches = BATCHES.load(SeqCst) > 0;
    if batches {
        // The next character of a summon that goes one at a time will not start.
        CANCEL_EPOCH.fetch_add(1, SeqCst);
    }
    // And whatever is running right now ends at once.
    let pid = SUMMONING.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(pid) = pid {
        kill_group(pid); // the group, so the python inside `timeout` goes too
    }
    batches || pid.is_some()
}

/// Ends a whole process group: `timeout` and the python it is watching over.
fn kill_group(pid: u32) {
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    unsafe {
        kill(-(pid as i32), 15);
    }
}

/// The helper with an optional text for its standard input and its own time limit.
fn run_helper_with(args: &[&str], stdin: Option<&str>, seconds: u32) -> Result<serde_json::Value, String> {
    run_helper_inner(args, stdin, seconds, false)
}

fn run_helper_inner(args: &[&str], stdin: Option<&str>, seconds: u32, cancellable: bool) -> Result<serde_json::Value, String> {
    let _guard = lock();
    let mut child = clean_command("timeout")
        .arg(seconds.to_string())
        .args(["python3", "-c", HELPER])
        .args(args)
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|e| format!("could not run python3: {e}"))?;
    if cancellable {
        *SUMMONING.lock().unwrap_or_else(|e| e.into_inner()) = Some(child.id());
    }
    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        let _ = pipe.write_all(text.as_bytes());
    }
    let out = child.wait_with_output().map_err(|e| format!("could not run python3: {e}"))?;
    let was_cancelled = cancellable && SUMMONING.lock().unwrap_or_else(|e| e.into_inner()).take().is_none();
    if was_cancelled {
        return Err(CANCELLED.to_string());
    }
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() { "the overlay did not answer".to_string() } else { friendly(&err) });
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("unexpected answer from the helper: {e}"))
}

/// What a batch summon did.
#[derive(Serialize, Debug, Default)]
pub struct SpawnResult {
    pub spawned: usize,
    /// Names the overlay does not know (deleted since a preset was saved, say).
    pub missing: Vec<String>,
    /// Characters that lacked pictures. They are fixed now, but the overlay that is
    /// running read them before, so they can only appear once it has restarted.
    pub restart_needed: Vec<String>,
    /// Set when the fast way did not work and they had to be summoned one process at a
    /// time (which is slow and visible). It says what went wrong, so it is not a mystery.
    pub slow_reason: Option<String>,
}

/// Summons several characters, as many copies of each as asked, in one go.
///
/// Starts the overlay first if it is not running. If the fast way cannot be used
/// (the helper leans on the internals of one wl_shimeji version) it falls back to
/// one `shimejictl summon` per character, slower but it always worked.
pub fn summon_batch(items: &[(String, usize)]) -> Result<SpawnResult, String> {
    // Registered from the first moment: repairing pictures and starting the overlay take
    // time too, and "stop" has to reach a summon that is still getting ready.
    let batch = Batch::begin();
    let items: Vec<(String, usize)> = items
        .iter()
        .filter(|(name, _)| !name.starts_with('.')) // helper prototypes are never summoned on their own
        .map(|(n, c)| (n.clone(), (*c).max(1)))
        .collect();
    let total: usize = items.iter().map(|(_, c)| *c).sum();
    if total == 0 {
        return Ok(SpawnResult::default());
    }

    // A character that lacks pictures takes the overlay down when it appears (see
    // "Missing pictures" above). Fix it first; and if the overlay is already running
    // it knows the old, broken version, so leave that character for after a restart.
    let running = overlay_running();
    let mut restart_needed = Vec::new();
    let mut items = items;
    items.retain(|(name, _)| {
        if !missing_frames(name).is_empty() {
            let _ = repair_frames(name);
        }
        if running && is_stale(name) {
            restart_needed.push(name.clone());
            return false;
        }
        true
    });
    let total: usize = items.iter().map(|(_, c)| *c).sum();
    if total == 0 {
        return Ok(SpawnResult { restart_needed, ..SpawnResult::default() });
    }

    batch.check()?;
    if !overlay_ready() {
        start_overlay()?;
    }
    batch.check()?;

    let payload = serde_json::to_string(&items).map_err(|e| e.to_string())?;
    let seconds = (10 + total as u32 * 2).min(240);
    match run_helper_inner(&["spawn"], Some(&payload), seconds, true) {
        Ok(v) => Ok(SpawnResult {
            spawned: v["spawned"].as_u64().unwrap_or(0) as usize,
            missing: v["missing"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
            restart_needed,
            slow_reason: None,
        }),
        // The overlay itself is gone: another way of asking will not help. Nor is there
        // anything to retry after a person pressed stop.
        Err(e) if e == CANCELLED || e.starts_with("The overlay is not responding") => Err(e),
        // The fast way failed for some other reason. One process per character still
        // works, so nobody is left un-summoned — but it takes about a second each, which
        // is why they then appear one after another.
        Err(why) => {
            let mut done = 0;
            for (name, count) in &items {
                for _ in 0..(*count).max(1) {
                    // Between every two characters: this is where "stop" is heard.
                    batch.check()?;
                    summon(name)?;
                    done += 1;
                }
            }
            Ok(SpawnResult { spawned: done, missing: Vec::new(), restart_needed, slow_reason: Some(why) })
        }
    }
}

/// Summons `count` characters picked at random from what is installed.
///
/// The Scene tab picks in the front end, where it can show who it chose; the tray has no
/// window to show anything in, so it picks here.
pub fn summon_random(count: usize) -> Result<SpawnResult, String> {
    let names: Vec<String> = installed_names().into_iter().filter(|n| !n.starts_with('.')).collect();
    if names.is_empty() {
        return Err("There are no characters installed yet.".to_string());
    }
    // Enough randomness for picking a character: the clock, which nobody is predicting.
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos() as usize).unwrap_or(0);
    let items: Vec<(String, usize)> = (0..count.max(1))
        .map(|i| (names[(seed.wrapping_add(i.wrapping_mul(2_654_435_761))) % names.len()].clone(), 1))
        .collect();
    summon_batch(&items)
}

/// Everyone who is really on screen right now, as (prototype name, how many),
/// most numerous first. Empty (not an error) when the overlay is not running.
pub fn on_screen() -> Result<Vec<(String, usize)>, String> {
    if !overlay_running() {
        return Ok(Vec::new());
    }
    let v = run_helper(&["list"])?;
    let mut counts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for m in v["mascots"].as_array().ok_or("unexpected answer from the helper")? {
        if let Some(n) = m["name"].as_str() {
            *counts.entry(n.to_string()).or_default() += 1;
        }
    }
    let mut list: Vec<(String, usize)> = counts.into_iter().collect();
    list.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    Ok(list)
}

/// Dismisses one copy (the newest) or every copy of a character. Returns how many went.
pub fn dismiss_character(name: &str, all: bool) -> Result<usize, String> {
    let v = run_helper(&["dismiss", name, if all { "all" } else { "one" }])?;
    Ok(v["dismissed"].as_u64().unwrap_or(0) as usize)
}

/// Exports a prototype to .wlshm.
/// How long one character's export may take. They take about a second and a half, the longest measured five.
const EXPORT_SECONDS: u64 = 40;

/// The file name the engine gives a prototype exported into a folder.
pub fn engine_file_name(name: &str) -> String {
    format!("{name}.wlshm").replace(' ', "_").replace('/', "_")
}

/// Exports several prototypes into a folder in ONE call: one process and one connection to the overlay instead of one
/// of each per character (measured: 84 characters in 3.6 s, against a second and a half each). If one of them never
/// gets an answer the whole call waits, so the time allowed grows with the count but stays modest; the caller then
/// finds out which ones did not come out and asks for those on their own.
pub fn export_prototypes_to_dir(names: &[String], dir: &Path, seconds: u64) -> Result<(), String> {
    let out = dir.display().to_string();
    let mut args: Vec<&str> = vec!["prototypes", "export"];
    for name in names {
        args.push("-i");
        args.push(name);
    }
    args.extend(["-o", &out, "-f"]);
    run_timeout(&args, seconds).map(|_| ())
}

pub fn export_prototype(name: &str, out_file: &Path) -> Result<(), String> {
    run_timeout(
        &["prototypes", "export", "-i", name, "-o", &out_file.display().to_string()],
        EXPORT_SECONDS,
    )
    .map(|_| ())
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_a_new_machine_with_the_factory_size_gets_normal_size() {
        assert!(wants_normal_size("2.000000", false, false));
        assert!(!wants_normal_size("2.000000", true, false), "there are characters: someone has been here");
        assert!(!wants_normal_size("2.000000", false, true), "already done once");
        assert!(!wants_normal_size("0.500000", false, false), "a size somebody chose");
        assert!(!wants_normal_size("-1.000000", false, false));
        assert!(!wants_normal_size("garbage", false, false));
    }

    #[test]
    fn a_character_only_the_engine_remembers_is_found() {
        let listed = vec!["Loba".to_string(), "Biscuit Bunny".to_string(), ".Needle".to_string()];
        let disk = vec!["Loba".to_string(), ".Needle".to_string()];
        assert_eq!(forgotten(&listed, &disk), vec!["Biscuit Bunny".to_string()]);
        assert!(forgotten(&listed, &listed).is_empty());
    }

    #[test]
    fn a_failed_import_says_why_and_not_the_whole_command_line() {
        let long = format!("`shimejictl prototypes import -f {}` → Failed to reload prototype 12", "/run/user/1000/x.wlshm ".repeat(20));
        assert_eq!(reason_of(&long), "Failed to reload prototype 12");
        assert!(reason_of("`shimejictl prototypes import -f a.wlshm` → ").contains("crashed"));
        assert!(reason_of(&format!("`x` → {}", "e".repeat(500))).chars().count() <= 161);
        assert_eq!(reason_of("The overlay is not responding. It has probably crashed."), "The overlay is not responding. It has probably crashed.");
    }


    #[test]
    fn the_engines_import_log_says_what_its_exit_status_does_not() {
        let log = "INFO:__main__:Successfully imported prototype referenced by id 116668661\n\
                   INFO:__main__:Successfully imported prototype referenced by id 102795811\n\
                   ERROR:__main__:Failed to import prototype referenced by id 5: Invalid file format\n\
                   WARNING:__main__:File '/run/user/1000/menagerie/ready-1/Broken.wlshm' is not a valid wl_shimeji prototype, please first convert it\n\
                   ERROR:__main__:File '/x/Gone.wlshm' does not exist\n";
        let (n, problems) = read_import_log(log);
        assert_eq!(n, 2);
        assert_eq!(problems, ["the engine refused a file: Invalid file format", "Broken.wlshm: not a wl_shimeji prototype", "Gone.wlshm: the file is gone"]);
        assert_eq!(read_import_log(""), (0, vec![]));
    }

    #[test]
    fn the_engine_names_an_exported_file_by_replacing_spaces_and_slashes() {
        assert_eq!(engine_file_name("Ezio"), "Ezio.wlshm");
        assert_eq!(engine_file_name("Leonardo da Vinci"), "Leonardo_da_Vinci.wlshm");
        assert_eq!(engine_file_name("A/B C"), "A_B_C.wlshm");
        assert_eq!(engine_file_name(".Hornet_Needle"), ".Hornet_Needle.wlshm");
    }

    #[test]
    fn a_program_that_never_answers_is_given_up_on_and_killed() {
        let began = Instant::now();
        let error = run_program("sleep", &["30"], 1).unwrap_err();
        assert!(began.elapsed() < Duration::from_secs(5), "waited {:?}", began.elapsed());
        assert!(is_no_answer(&error), "{error}");
        assert!(error.contains("sleep 30"), "the command is named: {error}");
    }

    #[test]
    fn a_program_that_answers_is_read_whole_and_one_that_fails_says_why() {
        assert_eq!(run_program("echo", &["hello"], 5).unwrap(), "hello\n");
        // Plenty of output, more than a pipe holds, must not stall the wait.
        let big = run_program("sh", &["-c", "head -c 300000 /dev/zero | tr '\\0' x"], 10).unwrap();
        assert_eq!(big.len(), 300_000);
        let failed = run_program("sh", &["-c", "echo nope >&2; exit 3"], 5).unwrap_err();
        assert!(failed.contains("nope") && !is_no_answer(&failed), "{failed}");
        assert!(run_program("no-such-program-menagerie", &[], 1).is_err());
    }
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("menagerie-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// Regression: `file_stem()` broke on names like `Shimeji.X`, `Shimeji.X (Y)`
    /// and `Shimeji..X`. Exactly one folder must match, and `Shimeji` matches none.
    #[test]
    fn installed_names_come_from_the_folders_without_prefix_or_stray_files() {
        let root = tmp("names");
        let a = root.join("shimejis");
        let b = root.join("prototypes");
        for d in ["Shimeji.BMO", "Shimeji..Hornet_Needle", "Shimeji.Mosscreep (Orange)", "PlainName"] {
            std::fs::create_dir_all(a.join(d)).unwrap();
        }
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(b.join("Shimeji.Packed.wlshm"), b"x").unwrap();
        std::fs::write(a.join("notes.txt"), b"not a character").unwrap();
        std::fs::write(a.join("Shimeji.NotAFolder"), b"a file, not a prototype").unwrap();

        let names = installed_names_in(&[b, a, root.join("missing")]);
        assert_eq!(names, vec![".Hornet_Needle", "BMO", "Mosscreep (Orange)", "Packed", "PlainName"]);
    }

    /// Live: starts the overlay, summons a dozen characters in one batch and times it
    /// against one-by-one summons. Dismisses everyone and stops the overlay again if
    /// it was not running before. Touches the live session, so:
    /// `cargo test live_batch -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_the_overlay_starts_with_a_small_collection() {
        // Never disturbs one that is running.
        if overlay_pid().is_some() {
            return;
        }
        // Few characters: the overlay has read them in a fifth of a second and used to be gone before the app looked.
        let pid = start_overlay().expect("the overlay starts");
        assert_eq!(overlay_pid(), Some(pid));
        std::thread::sleep(std::time::Duration::from_secs(2));
        assert!(overlay_pid().is_some(), "still there: someone was connected while the caller got ready");
        let _ = run(&["stop"]);
    }

    #[test]
    #[ignore]
    fn live_batch_summon_is_fast_and_the_overlay_survives() {
        let was_running = overlay_running();
        println!("overlay running before the test: {was_running}");
        let began = Instant::now();
        let pid = start_overlay().expect("the overlay starts");
        println!("overlay ready, pid {pid}, after {:?}", began.elapsed());

        let names: Vec<String> = installed_names().into_iter().filter(|n| !n.starts_with('.')).take(12).collect();
        let items: Vec<(String, usize)> = names.iter().map(|n| (n.clone(), 1)).collect();

        let began = Instant::now();
        let result = summon_batch(&items).expect("the batch summon works");
        let batch = began.elapsed();
        std::thread::sleep(Duration::from_millis(800));
        let seen: usize = on_screen().expect("who is on screen").iter().map(|(_, c)| *c).sum();
        println!("batch: {} spawned in {batch:?}; {seen} on screen; missing {:?}", result.spawned, result.missing);
        assert_eq!(result.spawned, 12);
        assert!(seen >= 12, "only {seen} on screen");
        assert!(overlay_running(), "the overlay died during the batch");

        // Dismiss everyone, then watch whether the overlay stays.
        let _ = dismiss_all();
        let began = Instant::now();
        let mut gone_after = None;
        while began.elapsed() < Duration::from_secs(8) {
            if !overlay_running() {
                gone_after = Some(began.elapsed());
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        println!("after dismiss-all the overlay {}", match gone_after {
            Some(d) => format!("EXITED by itself after {d:?}"),
            None => "stayed running for 8 s".to_string(),
        });
        if !was_running && overlay_running() {
            let _ = run(&["stop"]);
        }
    }

    /// Live: the case that used to bring the overlay down, sixty characters at once,
    /// three times over. Reports the time of each round and whether the overlay lived.
    /// `cargo test live_sixty -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_sixty_characters_three_times() {
        let names: Vec<String> = installed_names().into_iter().filter(|n| !n.starts_with('.')).take(12).collect();
        let items: Vec<(String, usize)> = names.iter().map(|n| (n.clone(), 5)).collect();
        for round in 1..=3 {
            let began = Instant::now();
            let result = summon_batch(&items).expect("the batch summon works");
            let took = began.elapsed();
            std::thread::sleep(Duration::from_millis(1500));
            let seen: usize = on_screen().map(|v| v.iter().map(|(_, c)| *c).sum()).unwrap_or(0);
            println!("round {round}: {} spawned in {took:?}; {seen} on screen; overlay alive: {}", result.spawned, overlay_running());
            assert_eq!(result.spawned, 60);
            assert_eq!(seen, 60, "characters went missing");
            let _ = dismiss_all();
            std::thread::sleep(Duration::from_millis(700));
        }
        if overlay_running() {
            let _ = run(&["stop"]);
        }
    }

    #[test]
    fn crashes_are_read_from_the_systems_list_and_only_the_overlays() {
        let json = r#"[
            {"time": 1789854000000000, "pid": 1, "sig": 11, "exe": "/usr/bin/shimeji-overlayd"},
            {"time": 1789855000000000, "pid": 2, "sig": 6,  "exe": "/usr/bin/shimeji-overlayd"},
            {"time": 1789855500000000, "pid": 3, "sig": 11, "exe": "/usr/bin/firefox"}
        ]"#;
        assert_eq!(crashes_from_json(json, 0).unwrap().len(), 2);
        let recent = crashes_from_json(json, 1_789_854_500_000).unwrap();
        assert_eq!(recent, vec![Crash { time_ms: 1_789_855_000_000, signal: 6, cut_off: false }]);
        assert_eq!(crashes_from_json("", 0).unwrap(), vec![]);
        assert!(crashes_from_json("not json", 0).is_err());
    }

    fn folder_with_frames(root: &Path, name: &str, present: &[u32], used: &[u32]) -> PathBuf {
        let dir = root.join(format!("Shimeji.{name}"));
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        let actions: Vec<String> = used.iter().map(|n| format!(r#"{{"image": "shime{n}.qoi"}}"#)).collect();
        std::fs::write(dir.join("actions.json"), format!("[{}]", actions.join(","))).unwrap();
        for n in present {
            std::fs::write(dir.join("assets").join(format!("shime{n}.qoi")), format!("frame {n}")).unwrap();
        }
        dir
    }

    #[test]
    fn frames_used_by_the_actions_are_found_and_shimeji_the_word_is_not_one() {
        let text = r#"[{"name": "Shimeji.Test", "image": "Shime1.QOI"}, {"image": "shime12.png"}, {"image": "shime7.gif"}, {"image": "shime.qoi"}]"#;
        assert_eq!(used_frames(text).into_iter().collect::<Vec<_>>(), vec![1, 12]);
    }

    #[test]
    fn missing_pictures_are_filled_from_the_nearest_one_and_nothing_is_overwritten() {
        let root = tmp("frames");
        let dir = folder_with_frames(&root, "Lestrade", &[4, 5, 9], &[1, 2, 3, 4, 5, 6, 9, 10]);
        let assets = dir.join("assets");
        let have = present_frames(&assets);
        let gaps: Vec<u32> = used_frames(&std::fs::read_to_string(dir.join("actions.json")).unwrap()).into_iter().filter(|n| !have.contains_key(n)).collect();
        assert_eq!(gaps, vec![1, 2, 3, 6, 10]);

        // Same steps as repair_frames, on this folder.
        for &n in &gaps {
            let from = have.range(..n).next_back().or_else(|| have.range(n..).next()).map(|(_, f)| f.clone()).unwrap();
            std::fs::copy(assets.join(&from), assets.join(format!("shime{n}.qoi"))).unwrap();
        }
        let read = |n: u32| std::fs::read_to_string(assets.join(format!("shime{n}.qoi"))).unwrap();
        assert_eq!(read(1), "frame 4", "below the first one that exists: the first one");
        assert_eq!(read(3), "frame 4");
        assert_eq!(read(6), "frame 5", "otherwise the nearest one below");
        assert_eq!(read(10), "frame 9");
        assert_eq!(read(4), "frame 4", "what existed is untouched");
        assert_eq!(read(9), "frame 9");
    }

    #[test]
    fn the_log_is_readable_and_names_the_character_that_broke_it() {
        assert_eq!(strip_ansi("\u{1b}[93m[01:45][WARN] hello\u{1b}[0m"), "[01:45][WARN] hello");
        let line = "[03:36:38.598][WARN][src/mascot.c:479]: <Mascot:Shimeji.Lestrade:5> Action iterator called while behavior is NULL";
        let name = {
            let start = line.find("<Mascot:").unwrap() + "<Mascot:".len();
            let rest = &line[start..];
            rest[..rest.rfind(':').unwrap().min(rest.find('>').unwrap())].to_string()
        };
        assert_eq!(name, "Shimeji.Lestrade");
    }

    /// Live, on a COPY: repairs the collection under `$SHIMEJI_TEST_XDG` (a copy of
    /// ~/.local/share) and reports who needed it. `SHIMEJI_TEST_XDG=/tmp/x cargo test
    /// live_repair -- --ignored --nocapture`. Never point it at the real one by hand.
    #[test]
    #[ignore]
    fn live_repair_on_a_copy() {
        let Ok(copy) = std::env::var("SHIMEJI_TEST_XDG") else { return };
        std::env::set_var("XDG_DATA_HOME", &copy);
        assert!(config_root().starts_with(&copy), "the copy is not in use");
        println!("Lestrade lacks {:?}", missing_frames("Lestrade"));
        let fixed = repair_all();
        println!("repaired: {fixed:?}");
        assert!(missing_frames("Lestrade").is_empty(), "Lestrade is still incomplete");
        assert!(repair_all().is_empty(), "a second pass finds nothing left to do");
    }

    #[test]
    fn a_value_the_overlay_echoes_back_in_another_format_still_counts_as_taken() {
        assert!(same_value("0.30", "0.300000"));
        assert!(same_value("-1", "-1.000000"));
        assert!(same_value("Top", "top"));
        assert!(same_value("true", "true"));
        assert!(!same_value("bounce", "looping"));
        assert!(!same_value("0.7", "0.3"));
    }

    #[test]
    fn an_already_converted_character_is_recognised_by_its_name() {
        assert_eq!(wlshm_name(Path::new("/tmp/Shimeji.Hornet.wlshm")), "Hornet");
        assert_eq!(wlshm_name(Path::new("/tmp/BMO.wlshm")), "BMO");
        assert_eq!(wlshm_name(Path::new("/tmp/Biscuit Deer (2).wlshm")), "Biscuit Deer (2)");
    }

    #[test]
    fn only_archives_that_really_hold_prototypes_skip_the_converter() {
        let dir = tmp("ready");
        // A single .wlshm file is one character, whatever is inside it.
        let one = dir.join("Shimeji.Test.wlshm");
        std::fs::write(&one, b"not really a prototype, but named like one").unwrap();
        let found = ready_prototypes(&one).unwrap().expect("a .wlshm file is ready to import");
        assert_eq!(found.items.len(), 1);
        assert_eq!(found.items[0].0, "Test");

        // A zip of .wlshm files: this app's own export.
        let zip_path = dir.join("collection.zip");
        {
            let mut w = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
            let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
            for name in ["BMO.wlshm", "Shimeji.Hornet.wlshm"] {
                w.start_file(name, opts).unwrap();
                std::io::Write::write_all(&mut w, b"x").unwrap();
            }
            w.finish().unwrap();
        }
        let found = ready_prototypes(&zip_path).unwrap().expect("a zip of .wlshm is ready to import");
        let names: Vec<&str> = found.items.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["BMO", "Hornet"]);
        assert!(found.items.iter().all(|(_, p)| p.is_file()));

        // A Shimeji-EE zip must NOT take this path: it still needs converting.
        let ee = dir.join("shimeji-ee.zip");
        {
            let mut w = zip::ZipWriter::new(std::fs::File::create(&ee).unwrap());
            let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
            w.start_file("img/shime1.png", opts).unwrap();
            std::io::Write::write_all(&mut w, b"x").unwrap();
            w.start_file("conf/actions.xml", opts).unwrap();
            std::io::Write::write_all(&mut w, b"x").unwrap();
            w.finish().unwrap();
        }
        assert!(ready_prototypes(&ee).unwrap().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_name_is_never_a_path() {
        for bad in ["", ".", "..", "../x", "a/b", "/etc", "a\\b"] {
            assert!(prototype_folder(bad).is_none(), "{bad:?} must not resolve to a folder");
        }
    }

    #[test]
    fn stopping_a_summon_ends_the_whole_group_and_says_when_there_was_none() {
        // Nothing is running: there is nothing to stop, and it says so.
        assert!(!cancel_summon());

        // A group like the one a summon runs in: `timeout` with a child under it.
        let child = Command::new("timeout")
            .args(["30", "sh", "-c", "sleep 30"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .expect("start a group to stop");
        let pid = child.id();
        *SUMMONING.lock().unwrap() = Some(pid);

        assert!(cancel_summon(), "there was one to stop");
        // It is gone within a moment, and asking again finds nothing.
        let mut child = child;
        let began = Instant::now();
        while began.elapsed() < Duration::from_secs(3) {
            if matches!(child.try_wait(), Ok(Some(_))) {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(matches!(child.try_wait(), Ok(Some(_))), "the group should be gone");
        assert!(!cancel_summon());
    }

    #[test]
    fn a_summon_that_goes_one_at_a_time_hears_stop_between_characters() {
        // No summon is running: nothing to stop.
        assert!(!cancel_summon());

        let batch = Batch::begin();
        assert!(batch.check().is_ok());
        // Somebody presses Dismiss all while it is under way.
        assert!(cancel_summon(), "a batch was running");
        assert_eq!(batch.check(), Err(CANCELLED.to_string()));

        // A batch that starts after the stop is not affected by it.
        let next = Batch::begin();
        assert!(next.check().is_ok());
        drop(next);
        drop(batch);
        assert!(!cancel_summon(), "both are finished now");
    }

    #[test]
    fn an_overlay_killed_by_a_signal_is_remembered_as_a_crash() {
        use std::os::unix::process::ExitStatusExt;
        // A clean exit is not a crash, and it is not "no answer" either.
        remember_ending(std::process::ExitStatus::from_raw(0));
        let ending = last_ending(0).expect("a clean ending is still an ending");
        assert_eq!(ending.signal, None);
        // Signal 11 is what a segmentation fault leaves in the wait status.
        remember_ending(std::process::ExitStatus::from_raw(11));
        assert_eq!(last_ending(0).unwrap().signal, Some(11));
        // Asking about a window that ended before it says nothing.
        assert!(last_ending(now_ms() + 10_000).is_none());
    }

    #[test]
    fn a_missing_engine_is_named_and_other_start_failures_keep_their_reason() {
        let gone = std::io::Error::from(std::io::ErrorKind::NotFound);
        assert_eq!(start_error("shimejictl", &gone), NOT_INSTALLED);
        let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert!(start_error("shimejictl", &denied).starts_with("could not start shimejictl: "));
    }

    #[test]
    fn a_crashed_overlay_is_reported_as_that_and_not_as_python_noise() {
        let raw = "ERROR:__main__:Failed to handle packet: Invalid header in packet (Expected at least 8 bytes, got 0)\nERROR:root:Failed to start client";
        assert_eq!(friendly(raw), "The overlay is not responding. It has probably crashed.");
        assert_eq!(friendly("Prototype 'X' not found"), "Prototype 'X' not found");
    }

    #[test]
    fn prototype_entry_matches_only_exact_dir() {
        let root = tmp("proto");
        let dirs = [
            "Shimeji.BMO",
            "Shimeji.Hornet",
            "Shimeji..Hornet_Needle",
            "Shimeji.Mosscreep (Orange)",
            "Shimeji.Mosscreep (White)",
        ];
        for d in dirs {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }

        for (name, expected) in [
            ("BMO", "Shimeji.BMO"),
            ("Hornet", "Shimeji.Hornet"),
            (".Hornet_Needle", "Shimeji..Hornet_Needle"),
            ("Mosscreep (Orange)", "Shimeji.Mosscreep (Orange)"),
        ] {
            let hits: Vec<&str> = dirs
                .iter()
                .copied()
                .filter(|d| is_prototype_entry(&root.join(d), d, name))
                .collect();
            assert_eq!(hits, vec![expected], "name {name:?}");
        }

        for d in dirs {
            assert!(!is_prototype_entry(&root.join(d), d, "Shimeji"), "{d}");
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn parse_config_understands_live_and_local_formats() {
        let live = "Config:\n  - Multiplication (BREEDING) = true\n  - Overlay layer (WLR_SHELL_LAYER) = overlay\n";
        let v = parse_config(live);
        assert_eq!(v.len(), 2);
        assert!(v[0].live && v[0].key == "BREEDING" && v[0].value == "true");

        let local = "Config:\nBREEDING: true\nWLR_SHELL_LAYER: 3\nMASCOT_SCALE: 1.500000\n";
        let v = parse_config(local);
        assert_eq!(v.len(), 3);
        assert!(!v[0].live);
        assert_eq!((v[1].key.as_str(), v[1].value.as_str()), ("WLR_SHELL_LAYER", "overlay"));
        assert_eq!(v[2].value, "1.500000");
    }

    /// Reproduces fast clicks on "Summon random": 12 parallel calls. Without
    /// serialization the overlay went down. Changes the live session: run explicitly:
    /// `cargo test parallel_summons -- --ignored`.
    #[test]
    #[ignore]
    fn parallel_summons_do_not_crash_overlay() {
        let Ok(list) = list_prototypes() else { return };
        let Some(name) = list.first().cloned() else { return };

        let handles: Vec<_> = (0..12)
            .map(|_| {
                let n = name.clone();
                std::thread::spawn(move || summon(&n))
            })
            .collect();
        for h in handles {
            h.join().unwrap().expect("summon");
        }
        std::thread::sleep(std::time::Duration::from_millis(800));

        let env = run(&["environment", "list"]).unwrap();
        let _ = dismiss_all();
        assert!(!env.contains("with 0 mascots"), "the overlay lost its characters: {env}");
    }

    /// Changes the live session (summons and dismisses characters), so it runs only
    /// on request: `cargo test dismiss_all -- --ignored`.
    #[test]
    #[ignore]
    fn dismiss_all_clears_the_screen_quickly() {
        let Ok(list) = list_prototypes() else { return };
        let Some(name) = list.first() else { return };

        summon(name).unwrap();
        summon(name).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(500));

        let t = std::time::Instant::now();
        dismiss_all().expect("dismiss_all");
        let took = t.elapsed();
        assert!(took < std::time::Duration::from_secs(3), "too slow: {took:?}");

        std::thread::sleep(std::time::Duration::from_millis(500));
        let env = run(&["environment", "list"]).unwrap();
        assert!(env.contains("with 0 mascots"), "characters are still on screen: {env}");
    }

    #[test]
    fn only_the_end_of_a_huge_log_is_read() {
        use std::io::{Seek, SeekFrom};
        let dir = tmp("hugelog");
        let path = dir.join("overlay.log");
        // A gigabyte that is mostly a hole, with three lines at its end. Reading all of it would take
        // seconds and a gigabyte of memory, which is what once held a recovery up.
        let mut file = std::fs::File::create(&path).unwrap();
        file.set_len(1 << 30).unwrap();
        file.seek(SeekFrom::End(0)).unwrap();
        writeln!(file, "the third from the end").unwrap();
        writeln!(file, "the one before the last").unwrap();
        writeln!(file, "the last line").unwrap();
        drop(file);

        let began = Instant::now();
        let tail = read_tail(&path, LOG_WINDOW);
        assert!(began.elapsed() < Duration::from_millis(500), "took {:?}", began.elapsed());
        assert!(tail.len() as u64 <= LOG_WINDOW);
        assert!(tail.ends_with("the one before the last\nthe last line\n"), "{:?}", &tail[tail.len().saturating_sub(60)..]);
        assert!(!tail.contains('\0'), "the cut is at a line, so half of one is not shown");
        assert_eq!(read_tail(&dir.join("missing.log"), 100), "");
        std::fs::write(&path, "short\nfile\n").unwrap();
        assert_eq!(read_tail(&path, LOG_WINDOW), "short\nfile\n", "a small file is read whole");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_line_printed_again_and_again_is_shown_once_with_its_count() {
        let dir = tmp("foldlog");
        let path = dir.join("overlay.log");
        let mut text = String::from("[10:00:00.001][INFO][src/a.c:1]: starting\n");
        for i in 0..5000 {
            text.push_str(&format!("\x1b[33m[10:00:01.{:03}][ERROR][src/shimeji-overlay.c:843]: Wayland connection closed\x1b[0m\n", i % 1000));
        }
        std::fs::write(&path, text).unwrap();
        let shown = log_tail_of(&path, 6);
        assert_eq!(shown.lines().count(), 2, "{shown}");
        assert!(shown.contains("Wayland connection closed  (repeated 5000 times)"), "{shown}");
        assert!(!shown.contains('\x1b'), "no colour codes");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_lost_connection_is_recognised_only_by_a_whole_run_of_the_complaint() {
        let complaint = "[10:00:00.100][ERROR][src/shimeji-overlay.c:843]: Wayland connection closed\n";
        assert!(connection_lost(&complaint.repeat(30)));
        assert!(!connection_lost(&complaint.repeat(29)), "a few lines are not a loop");
        assert!(!connection_lost(&format!("{}[10:00:01.000][INFO]: something else\n", complaint.repeat(40))), "the end of the log decides");
        assert!(connection_lost(&format!("[10:00:00.000][INFO]: earlier news\n{}\n\n", complaint.repeat(40))), "blank lines do not count");
        assert!(!connection_lost(""));
    }

    #[test]
    fn a_complaint_that_repeats_is_cut_to_one_line_and_a_count_and_the_rest_stays() {
        let complaint = "[10:00:00.100][ERROR][src/shimeji-overlay.c:843]: Wayland connection closed\n";
        let text = format!("--- started by Menagerie (attempt 1) ---\n[09:59:59.000][WARN]: <Mascot:Shimeji.Rake:5> Applying offset: 1, 2\n{}", complaint.repeat(100_000));
        let out = collapse_complaint(&text);
        assert!(out.len() < 1000, "{} bytes", out.len());
        assert!(out.starts_with("--- started by Menagerie (attempt 1) ---\n"), "{out}");
        assert!(out.contains("Applying offset"), "what came before is kept");
        assert_eq!(out.matches("Wayland connection closed").count(), 2, "the line once, and the note about it: {out}");
        assert!(out.contains("100000 times"), "{out}");
        assert_eq!(collapse_complaint("a\nb\n"), "a\nb\n", "a log without the complaint is left as it is");
    }

    #[test]
    fn an_overlay_ended_for_losing_the_desktop_is_a_crash_with_that_reason() {
        let killed = Some(Ending { when_ms: 1_000_000, signal: Some(9) });
        // Its exit status and our own note are one event: a single crash, and it says why.
        assert_eq!(seen_by_us(killed, 1_000_800, 0), vec![Crash { time_ms: 1_000_000, signal: 9, cut_off: true }]);
        // Without a note it is an ordinary crash.
        assert_eq!(seen_by_us(killed, 0, 0), vec![Crash { time_ms: 1_000_000, signal: 9, cut_off: false }]);
        // An overlay an earlier run of the app started is not ours to wait for, but ending it is still known.
        assert_eq!(seen_by_us(None, 2_000_000, 0), vec![Crash { time_ms: 2_000_000, signal: 9, cut_off: true }]);
        // What happened before the window asked about is not news, and a clean exit is not a crash.
        assert!(seen_by_us(None, 2_000_000, 3_000_000).is_empty());
        assert!(seen_by_us(Some(Ending { when_ms: 5, signal: None }), 0, 0).is_empty());
    }

    #[test]
    fn a_process_is_tied_to_a_log_by_where_its_output_goes() {
        let dir = tmp("writesto");
        let mine = dir.join("overlay.log");
        let other = dir.join("other.log");
        std::fs::write(&other, "").unwrap();
        let mut child = Command::new("sleep").arg("30").stdin(Stdio::null()).stdout(std::fs::File::create(&mine).unwrap()).spawn().unwrap();
        let (writes_mine, writes_other) = (writes_to(child.id(), &mine), writes_to(child.id(), &other));
        let _ = child.kill();
        let _ = child.wait();
        assert!(writes_mine, "it writes to the file it was given");
        assert!(!writes_other, "and not to another one, so a log cannot make us end a process that is not its writer");
        assert!(!writes_to(u32::MAX - 1, &mine), "a process that is not there writes nowhere");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_log_past_the_limit_is_emptied_in_place_and_a_small_one_is_left_alone() {
        let dir = tmp("biglog");
        let path = dir.join("overlay.log");
        std::fs::File::create(&path).unwrap().set_len(LOG_LIMIT + 1).unwrap();
        look_at_log(&path);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("--- Menagerie emptied this log"), "{text}");
        assert!(text.len() < 200);

        std::fs::write(&path, "small\n").unwrap();
        look_at_log(&path);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "small\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_overlay_that_only_complains_is_ended_and_its_log_cut_down() {
        use std::os::unix::process::ExitStatusExt;
        let dir = tmp("spinner");
        let log = dir.join("overlay.log");
        // A stand-in for an overlay that has lost the desktop: it says so as fast as it can, into its log.
        let mut file = std::fs::File::create(&log).unwrap();
        writeln!(file, "--- started by Menagerie (attempt 1) ---").unwrap();
        writeln!(file, "[09:59:59.000][WARN]: <Mascot:Shimeji.Rake:5> Applying offset: 1, 2").unwrap();
        let mut child = Command::new("sh")
            .args(["-c", "while :; do echo '[10:00:00.100][ERROR][src/shimeji-overlay.c:843]: Wayland connection closed'; done"])
            .stdin(Stdio::null())
            .stdout(file)
            .spawn()
            .unwrap();
        let id = child.id();
        // What the real lookup does, for this one process: a zombie is a process that is gone.
        let find = move || {
            let alive = std::fs::read_to_string(format!("/proc/{id}/stat")).map(|s| !s.contains(") Z")).unwrap_or(false);
            alive.then_some(id)
        };

        let began = Instant::now();
        while std::fs::metadata(&log).map(|m| m.len()).unwrap_or(0) < 200_000 && began.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let before = std::fs::metadata(&log).unwrap().len();
        assert!(before >= 200_000, "the stand-in never got going");

        look_at_log_of(&log, &find);

        let status = child.wait().unwrap();
        assert_eq!(status.signal(), Some(9), "it was ended, and not politely: {status:?}");
        let after = std::fs::read_to_string(&log).unwrap();
        assert!(after.len() < 2000, "{} bytes are left of {before}", after.len());
        assert!(after.starts_with("--- started by Menagerie (attempt 1) ---\n"), "{after}");
        assert!(after.contains("Applying offset"), "what came before it is kept");
        assert!(after.contains("was printed") && after.contains("times"), "{after}");
        assert!(CUT_OFF_AT.load(Ordering::SeqCst) > 0, "the ending is remembered, with its reason");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_log_full_of_the_complaint_is_left_alone_when_no_overlay_writes_to_it() {
        // Somebody else's process, or none: the words alone are not a reason to end anything.
        let dir = tmp("notours");
        let log = dir.join("overlay.log");
        let text = "[10:00:00.100][ERROR][src/shimeji-overlay.c:843]: Wayland connection closed\n".repeat(100);
        std::fs::write(&log, &text).unwrap();
        let stranger = Command::new("sleep").arg("30").stdout(Stdio::null()).spawn();
        let mut stranger = stranger.unwrap();
        look_at_log_of(&log, &|| Some(stranger.id()));
        let alive = matches!(stranger.try_wait(), Ok(None));
        let _ = stranger.kill();
        let _ = stranger.wait();
        assert!(alive, "a process that does not write to the log was ended");
        assert_eq!(std::fs::read_to_string(&log).unwrap(), text, "and the log is as it was");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod size_tests {
    use super::*;

    #[test]
    fn dir_size_sums_nested_files() {
        let root = std::env::temp_dir().join(format!("menagerie-size-{}", std::process::id()));
        std::fs::create_dir_all(root.join("a/b")).unwrap();
        std::fs::write(root.join("x"), [0u8; 100]).unwrap();
        std::fs::write(root.join("a/b/y"), [0u8; 50]).unwrap();
        assert_eq!(dir_size(&root), 150);
        let _ = std::fs::remove_dir_all(root);
    }
}

#[cfg(test)]
mod on_screen_tests {
    use super::*;

    /// Read-only: whatever is on screen must come back as a well-formed list.
    /// Skipped when the overlay is not running.
    #[test]
    fn on_screen_lists_real_characters_with_counts() {
        if !overlay_running() {
            return;
        }
        let list = on_screen().expect("on_screen");
        let sorted = list.windows(2).all(|w| w[0].1 >= w[1].1);
        assert!(sorted, "most numerous first: {list:?}");
        assert!(list.iter().all(|(n, c)| !n.is_empty() && !n.starts_with("Shimeji.") && *c > 0), "{list:?}");
        // The list must agree with the overlay's own count.
        let total: usize = list.iter().map(|(_, c)| c).sum();
        let env = run(&["environment", "list"]).unwrap_or_default();
        assert!(env.contains(&format!("with {total} mascots")), "helper says {total}, overlay says: {env}");
    }

    /// Summons two of one character, dismisses ONE, then the rest. Changes the
    /// live session, so it runs only on request: `cargo test dismiss_character -- --ignored`.
    #[test]
    #[ignore]
    fn dismiss_character_removes_exactly_what_was_asked() {
        let Ok(list) = list_prototypes() else { return };
        let Some(name) = list.iter().find(|n| !n.starts_with('.')).cloned() else { return };
        let count = |n: &str| on_screen().unwrap().into_iter().find(|(x, _)| x == n).map(|(_, c)| c).unwrap_or(0);

        let start = count(&name);
        summon(&name).unwrap();
        summon(&name).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(600));
        assert_eq!(count(&name), start + 2);

        assert_eq!(dismiss_character(&name, false).unwrap(), 1);
        std::thread::sleep(std::time::Duration::from_millis(400));
        assert_eq!(count(&name), start + 1, "one dismissed");

        assert_eq!(dismiss_character(&name, true).unwrap(), start + 1);
        std::thread::sleep(std::time::Duration::from_millis(400));
        assert_eq!(count(&name), 0, "all copies dismissed");
    }
}
