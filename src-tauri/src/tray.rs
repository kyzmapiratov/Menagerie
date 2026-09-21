// The tray icon (the system tray: the row of small icons in a panel or bar).
//
// Summoning someone or clearing the screen is two seconds of work that should not need a
// window: open it, find the tab, press the button, put it away again. From the tray it is
// one click. The menu holds only what is worth doing without looking at anything — a
// character, everyone gone, and a way back to the window.
//
// Not every desktop shows one. It needs a StatusNotifier host (KDE and GNOME with an
// extension have one; a bar such as Waybar or Quickshell provides it on wlroots
// compositors), and where there is none this quietly does nothing.
//
// It also needs a library on this side: libayatana-appindicator (or the older libappindicator),
// which the icon code opens when the first icon is made. That library is not something the app
// can count on — a distribution package asks for it, but a binary dropped on a bare system does
// not get it — and the code that opens it panics when it is missing. A missing extra must never
// stop the app from starting, so the panic is caught here and becomes an ordinary error.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Runtime};

pub const ID: &str = "menagerie-tray";

/// Brings the window back and puts it in front.
fn show_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Work that talks to the overlay, off the thread the menu click arrived on: a menu that
/// stays stuck open while a character is summoned would be worse than no menu.
fn in_background(what: impl FnOnce() + Send + 'static) {
    std::thread::spawn(what);
}

/// Set once the library that draws the icon turned out to be missing. The code that opens it
/// keeps its "already failed" state and would panic differently on a second try, so the answer
/// is remembered instead of asking again.
static LIBRARY_MISSING: AtomicBool = AtomicBool::new(false);

const LIBRARY_MISSING_TEXT: &str = "The tray icon needs the libayatana-appindicator library, which is not installed on this system.";

/// Runs the part that opens the tray library, turning a panic into an error.
///
/// The default panic hook is set aside for the moment so the message the library's panic
/// carries does not land on the terminal as if the app had crashed; what it says is used
/// instead. This runs once, on the thread that starts the app, before anything else is going on.
fn guarded(make: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let outcome = catch_unwind(AssertUnwindSafe(make));
    std::panic::set_hook(hook);

    outcome.unwrap_or_else(|payload| {
        let said = payload.downcast_ref::<String>().cloned().or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
        if said.contains("dynamic library") || said.contains("poisoned") {
            LIBRARY_MISSING.store(true, Ordering::SeqCst);
            Err(LIBRARY_MISSING_TEXT.to_string())
        } else {
            Err(format!("The tray library stopped: {}", said.lines().next().unwrap_or("no reason given")))
        }
    })
}

/// Whether an icon can be made here at all (false once it was found that the library is missing).
pub fn available() -> bool {
    !LIBRARY_MISSING.load(Ordering::SeqCst)
}

/// Adds the icon. Safe to call when it is already there.
pub fn install<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    if app.tray_by_id(ID).is_some() {
        return Ok(());
    }
    if !available() {
        return Err(LIBRARY_MISSING_TEXT.to_string());
    }

    let open = MenuItem::with_id(app, "open", "Open Menagerie", true, None::<&str>).map_err(|e| e.to_string())?;
    let random = MenuItem::with_id(app, "random", "Summon someone", true, None::<&str>).map_err(|e| e.to_string())?;
    let dismiss = MenuItem::with_id(app, "dismiss", "Dismiss everyone", true, None::<&str>).map_err(|e| e.to_string())?;
    let sep = PredefinedMenuItem::separator(app).map_err(|e| e.to_string())?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>).map_err(|e| e.to_string())?;
    let menu = Menu::with_items(app, &[&open, &random, &dismiss, &sep, &quit]).map_err(|e| e.to_string())?;

    let mut builder = TrayIconBuilder::with_id(ID)
        .menu(&menu)
        .tooltip("Menagerie")
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_window(app),
            "random" => in_background(|| {
                let _ = crate::shimejictl::summon_random(1);
            }),
            "dismiss" => in_background(|| {
                crate::shimejictl::cancel_summon();
                let _ = crate::shimejictl::dismiss_all();
            }),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // A plain click on the icon brings the window back, as every other app does.
            if let TrayIconEvent::DoubleClick { .. } = event {
                show_window(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    guarded(|| builder.build(app).map(|_| ()).map_err(|e| e.to_string()))
}

/// Takes it away. Safe to call when there is none.
pub fn remove<R: Runtime>(app: &AppHandle<R>) {
    let _ = app.remove_tray_by_id(ID);
}

/// Adds or removes it to match the preference, and remembers the choice.
pub fn apply<R: Runtime>(app: &AppHandle<R>, wanted: bool) -> Result<(), String> {
    // Turning it on is remembered only once it worked; otherwise the switch would come back
    // "on" next time with no icon in the tray.
    if wanted {
        install(app)?;
    } else {
        remove(app);
    }
    crate::prefs::set("tray", Some(if wanted { "1" } else { "0" }))
}

/// What the preference says (on unless it was turned off).
pub fn wanted() -> bool {
    crate::prefs::load().get("tray").map(|v| v != "0").unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// The panic hook and the "library missing" flag are shared by the whole process, so tests
    /// that touch them take turns.
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
    fn turn() -> std::sync::MutexGuard<'static, ()> {
        ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// What the icon crate does on a system without the library: it panics with this text.
    fn panics_like_the_icon_library() -> Result<(), String> {
        panic!("Failed to load ayatana-appindicator3 or appindicator3 dynamic library\n  libayatana-appindicator3.so.1: cannot open shared object file");
    }

    #[test]
    fn a_missing_library_becomes_an_error_and_is_remembered() {
        let _turn = turn();
        LIBRARY_MISSING.store(false, Ordering::SeqCst);
        let error = guarded(panics_like_the_icon_library).unwrap_err();
        assert!(error.contains("libayatana-appindicator"), "{error}");
        assert!(!error.contains("panicked") && !error.contains("cannot open shared object"), "an error is a sentence: {error}");
        assert!(!available(), "the next attempt must not try again");
        LIBRARY_MISSING.store(false, Ordering::SeqCst);
    }

    #[test]
    fn another_panic_is_reported_by_what_it_said_and_not_remembered_as_a_missing_library() {
        let _turn = turn();
        LIBRARY_MISSING.store(false, Ordering::SeqCst);
        let error = guarded(|| panic!("the extern function is not there")).unwrap_err();
        assert!(error.contains("the extern function is not there"), "{error}");
        assert!(available());
    }

    #[test]
    fn a_normal_result_passes_through_untouched() {
        let _turn = turn();
        assert_eq!(guarded(|| Ok(())), Ok(()));
        assert_eq!(guarded(|| Err("no place for icons".into())), Err("no place for icons".to_string()));
    }

    #[test]
    fn the_hook_that_was_there_is_put_back() {
        let _turn = turn();
        // A panic after the guard must still reach the hook that was installed before it.
        let seen = std::sync::Arc::new(AtomicBool::new(false));
        let flag = seen.clone();
        let before = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |_| flag.store(true, Ordering::SeqCst)));

        let _ = guarded(|| panic!("inside"));
        let _ = catch_unwind(|| panic!("after"));

        std::panic::set_hook(before);
        assert!(seen.load(Ordering::SeqCst), "the earlier panic hook was lost");
    }
}
