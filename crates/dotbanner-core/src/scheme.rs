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

/// A named palette. It carries either the sixteen base16 slots or an
/// explicit ramp — both describe a set of colours a banner can paint with,
/// and both come from the same kind of file, so the built-in palettes and
/// anything dropped into a scheme directory are the same thing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scheme {
    pub name: String,
    pub source: Source,
    /// Present when the file gave a full base16 palette.
    pub slots: Option<[Rgb; 16]>,
    /// Present when the file gave an explicit `ramp:` list.
    pub explicit: Option<Vec<Rgb>>,
}

/// Where a scheme came from, so a listing can say which are overridable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Shipped in the binary; a file of the same name shadows it.
    BuiltIn,
    /// Found in a scheme directory.
    Installed,
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
        if let Some(ramp) = &self.explicit {
            return ramp.clone();
        }
        match &self.slots {
            Some(slots) => [0x05, 0x0D, 0x0E, 0x08, 0x01]
                .iter()
                .map(|i| slots[*i as usize])
                .collect(),
            None => Vec::new(),
        }
    }

    /// Parse a base16 scheme from YAML or JSON. Both are read the same way —
    /// find every `baseNN` key and take the hex colour after it — which
    /// covers the base16 YAML schemes, their JSON ports and hand-written
    /// files without pulling in a parser for either language.
    /// `name` is the palette's identity — the thing typed after `--colors`.
    /// It comes from the filename rather than any `name:` inside the file,
    /// because a base16 file's own name is a display title ("Gruvbox dark,
    /// hard") and rarely typeable.
    pub fn parse(name: &str, text: &str) -> Option<Self> {
        let mut slots: [Option<Rgb>; 16] = [None; 16];
        // Scan the whole text rather than line by line: YAML puts one key
        // per line, JSON often puts several.
        let mut i = 0;
        while let Some(found) = text[i..].find("base") {
            let at = i + found + 4;
            i = at;
            let idx: String = text[at..].chars().take(2).collect();
            // Both characters must be hex digits: from_str_radix would
            // otherwise accept a sign, so `base+5` would overwrite base05.
            if idx.len() != 2 || !idx.chars().all(|c| c.is_ascii_hexdigit()) {
                continue;
            }
            let Ok(slot) = u8::from_str_radix(&idx, 16) else {
                continue;
            };
            if slot > 0x0F {
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
        if slots.iter().all(|s| s.is_some()) {
            let mut out = [Rgb::new(0, 0, 0); 16];
            for (i, slot) in slots.iter().enumerate() {
                out[i] = slot.expect("all slots checked above");
            }
            return Some(Scheme {
                name: name.to_string(),
                source: Source::Installed,
                slots: Some(out),
                explicit: None,
            });
        }

        // No palette: look for an explicit ramp, a list of hex colours under
        // a `ramp:` key. Curated gradients are ramps rather than palettes,
        // and this lets them live in the same kind of file.
        let ramp = Self::declared_ramp(text)?;
        (!ramp.is_empty()).then(|| Scheme {
            name: name.to_string(),
            source: Source::Installed,
            slots: None,
            explicit: Some(ramp),
        })
    }

    /// The hex colours of the `ramp` key, in order.
    ///
    /// The key is matched in both spellings a file might use — `ramp:` in
    /// YAML and `"ramp":` in JSON — and the list ends at the first line that
    /// starts a new top-level key, so a colour below the ramp is not
    /// swallowed into it.
    fn declared_ramp(text: &str) -> Option<Vec<Rgb>> {
        let key = text
            .find("ramp\"")
            .map(|i| i + "ramp\"".len())
            .or_else(|| text.find("ramp:").map(|i| i + "ramp:".len()))?;
        let mut out = Vec::new();
        let mut started = false;
        for line in text[key..].lines() {
            let raw = line;
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                continue;
            }
            // A line at column zero that is not part of the list ends it.
            let indented = raw.starts_with(char::is_whitespace);
            let is_item = trimmed.starts_with('-') || trimmed.starts_with(['[', '"', '#']);
            if started && !indented && !is_item {
                break;
            }
            // One line can hold several colours: JSON and YAML flow style
            // both write the whole list inline.
            let mut rest = trimmed;
            while let Some(at) = rest.find('#') {
                let token: String = rest[at..]
                    .chars()
                    .take_while(|c| *c == '#' || c.is_ascii_hexdigit())
                    .collect();
                rest = &rest[at + token.len()..];
                if let Ok(rgb) = Rgb::parse(&token) {
                    out.push(rgb);
                    started = true;
                }
            }
            if trimmed.contains(']') && started {
                break;
            }
        }
        (!out.is_empty()).then_some(out)
    }

    /// Read a scheme file, taking its name from the file stem.
    pub fn load(path: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(path).ok()?;
        let name = path.file_stem()?.to_string_lossy().to_string();
        Scheme::parse(&name, &text)
    }
}

/// The palettes shipped in the binary, as the same kind of file a user
/// would install. A scheme directory entry of the same name shadows one.
const BUILT_IN: &[(&str, &str)] = &[
    ("omarchy", include_str!("../schemes/omarchy.yaml")),
    ("fire", include_str!("../schemes/fire.yaml")),
    ("synthwave", include_str!("../schemes/synthwave.yaml")),
    ("mint", include_str!("../schemes/mint.yaml")),
    ("ember", include_str!("../schemes/ember.yaml")),
    ("steel", include_str!("../schemes/steel.yaml")),
    ("monokai", include_str!("../schemes/monokai.yaml")),
    ("gruvbox", include_str!("../schemes/gruvbox.yaml")),
    ("nord", include_str!("../schemes/nord.yaml")),
    ("dracula", include_str!("../schemes/dracula.yaml")),
    ("catppuccin", include_str!("../schemes/catppuccin.yaml")),
    ("tokyo-night", include_str!("../schemes/tokyo-night.yaml")),
    ("solarized", include_str!("../schemes/solarized.yaml")),
    ("everforest", include_str!("../schemes/everforest.yaml")),
    ("rose-pine", include_str!("../schemes/rose-pine.yaml")),
    ("kanagawa", include_str!("../schemes/kanagawa.yaml")),
];

/// The shipped palettes, in the order they are declared.
pub fn built_in() -> Vec<Scheme> {
    BUILT_IN
        .iter()
        .filter_map(|(name, text)| {
            Scheme::parse(name, text).map(|mut s| {
                s.source = Source::BuiltIn;
                s
            })
        })
        .collect()
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
    installed_reporting().0
}

/// Every scheme found on disk, plus the files that could not be read and
/// why. A malformed palette is otherwise indistinguishable from a missing
/// one, which makes a typo expensive to find.
pub fn installed_reporting() -> (Vec<Scheme>, Vec<(PathBuf, String)>) {
    let mut found: Vec<Scheme> = Vec::new();
    let mut problems: Vec<(PathBuf, String)> = Vec::new();
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
            match Scheme::load(&path) {
                Some(scheme) => {
                    if !found.iter().any(|s| s.name == scheme.name) {
                        found.push(scheme);
                    }
                }
                None => problems.push((
                    path,
                    "needs either all sixteen base16 slots or a ramp of hex colours".into(),
                )),
            }
        }
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    (found, problems)
}

/// Every palette a name can resolve to: installed files first, so a file
/// shadows a built-in of the same name, then the shipped set.
pub fn all() -> Vec<Scheme> {
    let mut out = installed();
    for scheme in built_in() {
        if !out.iter().any(|s| s.name == scheme.name) {
            out.push(scheme);
        }
    }
    out
}

/// Look one palette up by name.
pub fn find(name: &str) -> Option<Scheme> {
    all().into_iter().find(|s| s.name == name)
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
        let slots = s.slots.expect("a palette");
        assert_eq!(slots[0x00], Rgb::parse("#1d2021").unwrap());
        assert_eq!(slots[0x0F], Rgb::parse("#d65d0e").unwrap());
    }

    #[test]
    fn parses_an_explicit_ramp() {
        let text = "name: fire\nramp:\n  - \"#fff8d8\"\n  - \"#ffd21f\"\n  - \"#8a0f0f\"\n";
        let s = Scheme::parse("fire", text).expect("parses");
        assert_eq!(s.name, "fire", "identity comes from the filename");
        assert!(s.slots.is_none());
        assert_eq!(s.ramp().len(), 3);
    }

    #[test]
    fn every_built_in_parses_and_has_a_ramp() {
        let all = built_in();
        assert_eq!(
            all.len(),
            BUILT_IN.len(),
            "a shipped scheme failed to parse"
        );
        for s in &all {
            assert!(s.ramp().len() >= 3, "{} has too short a ramp", s.name);
            assert_eq!(s.source, Source::BuiltIn);
        }
    }

    #[test]
    fn built_in_names_are_unique() {
        let mut names: Vec<String> = built_in().into_iter().map(|s| s.name).collect();
        let total = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), total);
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
        assert_eq!(s.slots.unwrap()[0x0A], Rgb::parse("#fabd2f").unwrap());
    }

    #[test]
    fn a_json_ramp_loads() {
        let json = r##"{ "name": "x", "ramp": ["#111111", "#222222", "#333333"] }"##;
        let s = Scheme::parse("x", json).expect("json ramps parse");
        assert_eq!(s.ramp().len(), 3);
    }

    #[test]
    fn a_yaml_flow_ramp_loads_every_colour() {
        let text = r##"ramp: ["#111111", "#222222", "#333333"]"##;
        assert_eq!(Scheme::parse("x", text).unwrap().ramp().len(), 3);
    }

    #[test]
    fn a_key_after_the_ramp_is_not_swallowed() {
        let text = "ramp:\n  - \"#111111\"\n  - \"#222222\"\nbackground: \"#333333\"\n";
        assert_eq!(Scheme::parse("x", text).unwrap().ramp().len(), 2);
    }

    #[test]
    fn a_signed_slot_index_is_not_a_slot() {
        // `base+5` must not overwrite base05.
        let text = BASE16_YAML.to_string() + "base+5: \"#00ff00\"\n";
        let s = Scheme::parse("x", &text).unwrap();
        assert_eq!(s.slots.unwrap()[0x05], Rgb::parse("#d5c4a1").unwrap());
    }

    #[test]
    fn an_incomplete_palette_is_rejected() {
        // Neither a full palette nor a ramp.
        assert!(Scheme::parse("x", "base00: \"111111\"\nbase01: \"222222\"").is_none());
    }

    #[test]
    fn the_ramp_runs_foreground_to_background() {
        let s = Scheme::parse("x", BASE16_YAML).unwrap();
        let ramp = s.ramp();
        let slots = s.slots.expect("a palette");
        assert_eq!(ramp.len(), 5);
        assert_eq!(ramp[0], slots[0x05]);
        assert_eq!(ramp[4], slots[0x01]);
    }
}
