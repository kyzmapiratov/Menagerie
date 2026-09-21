// Asking the compositor what it can do, instead of guessing from its name.
//
// Whether characters can appear at all, and whether some of the overlay's settings do
// anything, depends on the Wayland protocols the compositor offers — not on whether it
// is called GNOME or niri. One short conversation with the compositor answers it: connect,
// ask for the registry, read the list of globals, stop. No library is needed for that; the
// wire format is four fields and a string.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

/// The protocols this app cares about, and whether the compositor has them.
#[derive(serde::Serialize, Debug, Default, Clone)]
pub struct Support {
    /// The layer the characters are drawn on. Without it nothing can appear (GNOME).
    pub layer_shell: bool,
    /// Every character is a subsurface of the layer.
    pub subcompositor: bool,
    /// What the Opacity setting needs. Many compositors, niri among them, do not have it.
    pub alpha_modifier: bool,
    /// Used for smooth scaling; without it characters are scaled in whole pixels.
    pub viewporter: bool,
    /// Every global the compositor announced, for the troubleshooting details.
    pub all: Vec<String>,
}

/// Where the compositor is listening (None: this is not a Wayland session).
fn socket_path() -> Option<PathBuf> {
    let display = std::env::var_os("WAYLAND_DISPLAY")?;
    let path = PathBuf::from(&display);
    if path.is_absolute() {
        return Some(path);
    }
    Some(PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR")?).join(path))
}

/// A Wayland message: object id, opcode, and the body. The length counts the header.
fn message(object: u32, opcode: u16, body: &[u8]) -> Vec<u8> {
    let len = (8 + body.len()) as u16;
    let mut out = Vec::with_capacity(len as usize);
    out.extend_from_slice(&object.to_ne_bytes());
    out.extend_from_slice(&opcode.to_ne_bytes());
    out.extend_from_slice(&len.to_ne_bytes());
    out.extend_from_slice(body);
    out
}

/// Reads the list of globals the compositor announces when a client connects.
///
/// The conversation is: `wl_display.get_registry` (the compositor then sends one
/// `global` event per protocol), then `wl_display.sync`, whose `done` says the list is
/// complete. Anything unexpected ends it; this only ever reads.
pub fn globals() -> Result<Vec<String>, String> {
    let path = socket_path().ok_or("not a Wayland session")?;
    let mut sock = UnixStream::connect(&path).map_err(|e| format!("could not reach the compositor at {}: {e}", path.display()))?;
    sock.set_read_timeout(Some(Duration::from_millis(600))).ok();
    sock.set_write_timeout(Some(Duration::from_millis(600))).ok();

    const DISPLAY: u32 = 1;
    const REGISTRY: u32 = 2;
    const CALLBACK: u32 = 3;

    // get_registry(new_id = 2), then sync(new_id = 3).
    let mut hello = message(DISPLAY, 1, &REGISTRY.to_ne_bytes());
    hello.extend_from_slice(&message(DISPLAY, 0, &CALLBACK.to_ne_bytes()));
    sock.write_all(&hello).map_err(|e| format!("could not ask the compositor: {e}"))?;

    let mut names = Vec::new();
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];

    loop {
        let read = sock.read(&mut chunk).map_err(|e| format!("the compositor stopped answering: {e}"))?;
        if read == 0 {
            break; // the compositor closed the connection: whatever we have is what there is
        }
        buf.extend_from_slice(&chunk[..read]);

        let mut at = 0;
        while buf.len() >= at + 8 {
            let object = u32::from_ne_bytes(buf[at..at + 4].try_into().unwrap());
            let opcode = u16::from_ne_bytes(buf[at + 4..at + 6].try_into().unwrap());
            let size = u16::from_ne_bytes(buf[at + 6..at + 8].try_into().unwrap()) as usize;
            if size < 8 || buf.len() < at + size {
                break; // the rest of this message has not arrived yet
            }
            let body = &buf[at + 8..at + size];

            // registry.global(name: u32, interface: string, version: u32)
            if object == REGISTRY && opcode == 0 && body.len() >= 8 {
                let len = u32::from_ne_bytes(body[4..8].try_into().unwrap()) as usize;
                // The string carries a trailing zero byte and is padded to four bytes.
                if len >= 1 && body.len() >= 8 + len {
                    let text = String::from_utf8_lossy(&body[8..8 + len - 1]).to_string();
                    names.push(text);
                }
            }
            // callback.done: the compositor has announced everything it has.
            if object == CALLBACK && opcode == 0 {
                return Ok(names);
            }
            at += size;
        }
        buf.drain(..at);
    }
    Ok(names)
}

/// What the compositor supports, or `None` when there is no Wayland session to ask.
pub fn support() -> Option<Support> {
    let all = globals().ok()?;
    let has = |name: &str| all.iter().any(|g| g == name);
    Some(Support {
        layer_shell: has("zwlr_layer_shell_v1"),
        subcompositor: has("wl_subcompositor"),
        alpha_modifier: has("wp_alpha_modifier_v1"),
        viewporter: has("wp_viewporter"),
        all,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_has_the_header_the_protocol_asks_for() {
        let m = message(1, 1, &2u32.to_ne_bytes());
        assert_eq!(m.len(), 12);
        assert_eq!(u32::from_ne_bytes(m[0..4].try_into().unwrap()), 1);
        assert_eq!(u16::from_ne_bytes(m[4..6].try_into().unwrap()), 1);
        assert_eq!(u16::from_ne_bytes(m[6..8].try_into().unwrap()), 12);
    }

    #[test]
    fn without_a_wayland_session_it_says_so_instead_of_waiting() {
        let saved = std::env::var_os("WAYLAND_DISPLAY");
        std::env::remove_var("WAYLAND_DISPLAY");
        assert!(globals().is_err());
        assert!(support().is_none());
        if let Some(v) = saved {
            std::env::set_var("WAYLAND_DISPLAY", v);
        }
    }

    /// On a real session this must find the basics. It is skipped where there is none.
    #[test]
    fn a_live_compositor_answers_with_its_protocols() {
        let Some(s) = support() else { return };
        assert!(s.all.iter().any(|g| g == "wl_compositor"), "every compositor has wl_compositor: {:?}", s.all);
        assert!(s.all.len() > 3);
    }
}
