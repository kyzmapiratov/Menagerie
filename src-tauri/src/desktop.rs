// Which desktop this is: niri, Hyprland, KDE Plasma, or something else.
//
// The three the app is built for differ in exactly the things it has to do: where "launch at login" is written
// (niri's `config.kdl`, Hyprland's `hyprland.conf`, an XDG autostart entry on KDE), which keys it can bind, and what
// the engine can do there. `XDG_CURRENT_DESKTOP` alone is a poor guide: plenty of compositors leave it unset, and a
// nested one inherits the outer one's. The compositor's own variables are asked first, the desktop's name after.

use serde::Serialize;

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Compositor {
    Niri,
    Hyprland,
    Kde,
    Sway,
    Gnome,
    Other,
}

impl Compositor {
    pub fn as_str(self) -> &'static str {
        match self {
            Compositor::Niri => "niri",
            Compositor::Hyprland => "hyprland",
            Compositor::Kde => "kde",
            Compositor::Sway => "sway",
            Compositor::Gnome => "gnome",
            Compositor::Other => "other",
        }
    }
}

/// The desktop of this session.
pub fn detect() -> Compositor {
    detect_with(|name| std::env::var(name).ok())
}

/// The same, reading the environment through `get`, so any desktop can be tried without being on it.
pub fn detect_with(get: impl Fn(&str) -> Option<String>) -> Compositor {
    let set = |name: &str| get(name).is_some_and(|v| !v.is_empty());
    let desktop = get("XDG_CURRENT_DESKTOP").unwrap_or_default().to_lowercase();
    let named = |n: &str| desktop.split(':').any(|d| d == n);

    // A compositor's own variables come first: they are set by it, in its session, and by nothing else.
    if set("HYPRLAND_INSTANCE_SIGNATURE") || named("hyprland") {
        Compositor::Hyprland
    } else if set("NIRI_SOCKET") || named("niri") {
        Compositor::Niri
    } else if named("kde") || set("KDE_FULL_SESSION") {
        Compositor::Kde
    } else if set("SWAYSOCK") || named("sway") {
        Compositor::Sway
    } else if named("gnome") || named("gnome-classic") || named("ubuntu") {
        Compositor::Gnome
    } else {
        Compositor::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())
    }

    #[test]
    fn the_three_desktops_the_app_is_built_for_are_told_apart() {
        assert_eq!(detect_with(env(&[("XDG_CURRENT_DESKTOP", "niri"), ("NIRI_SOCKET", "/run/user/1000/niri.sock")])), Compositor::Niri);
        assert_eq!(detect_with(env(&[("XDG_CURRENT_DESKTOP", "Hyprland"), ("HYPRLAND_INSTANCE_SIGNATURE", "abc_123")])), Compositor::Hyprland);
        assert_eq!(detect_with(env(&[("XDG_CURRENT_DESKTOP", "KDE"), ("KDE_FULL_SESSION", "true")])), Compositor::Kde);
    }

    #[test]
    fn the_compositors_own_variable_wins_over_a_desktop_name_that_is_missing_or_wrong() {
        // Hyprland started by a display manager often leaves the desktop name unset.
        assert_eq!(detect_with(env(&[("HYPRLAND_INSTANCE_SIGNATURE", "x")])), Compositor::Hyprland);
        // A niri nested in a KDE session still says KDE in the name.
        assert_eq!(detect_with(env(&[("XDG_CURRENT_DESKTOP", "KDE"), ("NIRI_SOCKET", "/x")])), Compositor::Niri);
        // Names come as lists: "ubuntu:GNOME", "KDE", "sway:wlroots".
        assert_eq!(detect_with(env(&[("XDG_CURRENT_DESKTOP", "ubuntu:GNOME")])), Compositor::Gnome);
        assert_eq!(detect_with(env(&[("XDG_CURRENT_DESKTOP", "sway:wlroots")])), Compositor::Sway);
    }

    #[test]
    fn an_empty_or_unknown_session_is_other_and_never_a_guess() {
        assert_eq!(detect_with(env(&[])), Compositor::Other);
        assert_eq!(detect_with(env(&[("XDG_CURRENT_DESKTOP", ""), ("NIRI_SOCKET", "")])), Compositor::Other);
        assert_eq!(detect_with(env(&[("XDG_CURRENT_DESKTOP", "COSMIC")])), Compositor::Other);
        assert_eq!(Compositor::Kde.as_str(), "kde");
    }
}
