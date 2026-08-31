//! Colour schemes in the base16 interchange format.
//!
//! base16 is the portable way people already share terminal and editor
//! themes: sixteen slots, `base00`–`base0F`, with fixed semantics — `base00`
//! the background, `base05` the default foreground, `base08`–`base0F` the
//! accents. Konsole colour schemes, editor themes and the base16 library all
//! express the same sixteen colours, so accepting the format means a banner
//! can match whatever the rest of a setup already runs.
//!
//! A banner needs an ordered ramp rather than a palette, so one is derived
//! by a fixed rule (see [`Scheme::ramp`]). The rule is documented because a
//! predictable result matters more than a clever one: a reader should be
//! able to look at a scheme file and know what the banner will do.

use std::path::{Path, PathBuf};

use crate::color::Rgb;

/// A base16 palette: sixteen slots in their canonical order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scheme {
    pub name: String,
    pub slots: [Rgb; 16],
}

impl Scheme {
    /// The ramp a banner paints with, read top to bottom:
    ///
    /// `base05` (foreground) → `base0D` (blue) → `base0E` (violet) →
    /// `base08` (red) → `base01` (dark background shade)
    ///
    /// Starting light and ending dark suits a banner lit from above, and the
    /// three accents in between are the slots themes vary most, so schemes
    /// stay recognisable.
    pub fn ramp(&self) -> Vec<Rgb> {
        [0x05, 0x0D, 0x0E, 0x08, 0x01]
            .iter()
            .map(|i| self.slots[*i as usize])
            .collect()
    }

    /// Parse a base16 scheme from YAML or JSON. Both are read the same way —
    /// find every `baseNN` key and take the hex colour after it — which
    /// covers the base16 YAML schemes, their JSON ports and hand-written
    /// files without pulling in a parser for either language.
    pub fn parse(name: &str, text: &str) -> Option<Self> {
        let mut slots: [Option<Rgb>; 16] = [None; 16];
        // Scan the whole text rather than line by line: YAML puts one key
        // per line, JSON often puts several.
        let mut i = 0;
        while let Some(found) = text[i..].find("base") {
            let at = i + found + 4;
            i = at;
            let idx: String = text[at..].chars().take(2).collect();
            let Ok(slot) = u8::from_str_radix(&idx, 16) else {
                continue;
            };
            if slot > 0x0F || idx.len() != 2 {
                continue;
            }
            // Take the first hex-looking token after the key, stopping at
            // the value's closing quote or a separator.
            let after = at + idx.len();
            let value: String = text[after..]
                .chars()
                .skip_while(|c| !c.is_ascii_hexdigit() && *c != '#')
                .take_while(|c| c.is_ascii_hexdigit() || *c == '#')
                .collect();
            if let Ok(rgb) = Rgb::parse(&value) {
                slots[slot as usize] = Some(rgb);
            }
        }
        if slots.iter().any(|s| s.is_none()) {
            return None;
        }
        let mut out = [Rgb::new(0, 0, 0); 16];
        for (i, slot) in slots.iter().enumerate() {
            out[i] = slot.expect("all slots checked above");
        }
        Some(Scheme {
            name: name.to_string(),
            slots: out,
        })
    }

    /// Read a scheme file, taking its name from the file stem.
    pub fn load(path: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(path).ok()?;
        let name = path.file_stem()?.to_string_lossy().to_string();
        Scheme::parse(&name, &text)
    }
}

/// Where scheme files are looked for, in order: the user's config, then
/// shared data. Both are optional.
pub fn scheme_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(config) = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    {
        dirs.push(config.join("dotbanner/schemes"));
    }
    if let Some(data) = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
    {
        dirs.push(data.join("dotbanner/schemes"));
    }
    dirs
}

/// Every scheme found on disk, sorted by name. A name found in an earlier
/// directory wins, so a user's copy shadows a shared one.
pub fn installed() -> Vec<Scheme> {
    let mut found: Vec<Scheme> = Vec::new();
    for dir in scheme_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !matches!(ext, "yaml" | "yml" | "json") {
                continue;
            }
            if let Some(scheme) = Scheme::load(&path) {
                if !found.iter().any(|s| s.name == scheme.name) {
                    found.push(scheme);
                }
            }
        }
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}

/// Look one scheme up by name.
pub fn find(name: &str) -> Option<Scheme> {
    installed().into_iter().find(|s| s.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE16_YAML: &str = r#"
scheme: "Test Scheme"
author: "someone"
base00: "1d2021"
base01: "282828"
base02: "3c3836"
base03: "504945"
base04: "bdae93"
base05: "d5c4a1"
base06: "ebdbb2"
base07: "fbf1c7"
base08: "fb4934"
base09: "fe8019"
base0A: "fabd2f"
base0B: "b8bb26"
base0C: "8ec07c"
base0D: "83a598"
base0E: "d3869b"
base0F: "d65d0e"
"#;

    #[test]
    fn parses_base16_yaml() {
        let s = Scheme::parse("gruvbox-hard", BASE16_YAML).expect("parses");
        assert_eq!(s.slots[0x00], Rgb::parse("#1d2021").unwrap());
        assert_eq!(s.slots[0x0F], Rgb::parse("#d65d0e").unwrap());
    }

    #[test]
    fn parses_json_with_hashes() {
        let json = r##"{ "base00": "#1d2021", "base01": "#282828", "base02": "#3c3836",
            "base03": "#504945", "base04": "#bdae93", "base05": "#d5c4a1",
            "base06": "#ebdbb2", "base07": "#fbf1c7", "base08": "#fb4934",
            "base09": "#fe8019", "base0A": "#fabd2f", "base0B": "#b8bb26",
            "base0C": "#8ec07c", "base0D": "#83a598", "base0E": "#d3869b",
            "base0F": "#d65d0e" }"##;
        let s = Scheme::parse("x", json).expect("parses");
        assert_eq!(s.slots[0x0A], Rgb::parse("#fabd2f").unwrap());
    }

    #[test]
    fn an_incomplete_palette_is_rejected() {
        assert!(Scheme::parse("x", "base00: \"111111\"\nbase01: \"222222\"").is_none());
    }

    #[test]
    fn the_ramp_runs_foreground_to_background() {
        let s = Scheme::parse("x", BASE16_YAML).unwrap();
        let ramp = s.ramp();
        assert_eq!(ramp.len(), 5);
        assert_eq!(ramp[0], s.slots[0x05]);
        assert_eq!(ramp[4], s.slots[0x01]);
    }
}
