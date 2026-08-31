//! Semantic styling for the CLI's own output.
//!
//! Two kinds of colour leave this program and they follow opposite rules.
//! Colour inside a rendered banner is *content*: it is the thing the user
//! asked for, so it is always emitted and survives a pipe into a file.
//! Colour in the tool's own chrome — headings, hints, errors — is
//! *presentation*, so it is dropped when stdout is not a terminal or when
//! `NO_COLOR` is set.
//!
//! Four roles carry every meaning the chrome needs:
//!
//! | Role | Looks like | Means |
//! |------|-----------|-------|
//! | [`heading`] | bold | a section or topic |
//! | [`name`] | bold | a value you can type: a font, style or gradient |
//! | [`cmd`] | cyan | a command you can run |
//! | [`hint`] | dim | supporting context, counts, next steps |
//! | [`bad`] | red | something went wrong |

use std::io::IsTerminal;
use std::sync::OnceLock;

/// Whether chrome should carry colour. Honours `NO_COLOR` (any value) and
/// falls back to plain text whenever stdout is redirected.
pub fn colored() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal())
}

fn wrap(code: &str, text: &str) -> String {
    if colored() {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

/// A section or topic title.
pub fn heading(text: &str) -> String {
    wrap("1", text)
}

/// A value the reader can type back: a font family, style or gradient name.
pub fn name(text: &str) -> String {
    wrap("1", text)
}

/// A command the reader can run.
pub fn cmd(text: &str) -> String {
    wrap("36", text)
}

/// Supporting context: counts, metadata, the next step.
pub fn hint(text: &str) -> String {
    wrap("2", text)
}

/// A failure.
pub fn bad(text: &str) -> String {
    wrap("31", text)
}

/// Wrap a family in quotes when it contains spaces, so a suggestion can be
/// pasted straight back onto the command line.
pub fn quoted(name: &str) -> String {
    if name.contains(' ') {
        format!("\"{name}\"")
    } else {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_only_when_needed() {
        assert_eq!(quoted("Metal Mania"), "\"Metal Mania\"");
        assert_eq!(quoted("Monoid"), "Monoid");
    }

    #[test]
    fn chrome_is_plain_when_not_a_terminal() {
        // The test harness captures stdout, so colour is off here and every
        // role returns its text unchanged.
        assert!(!colored());
        assert_eq!(heading("Topics"), "Topics");
        assert_eq!(cmd("dotbanner show"), "dotbanner show");
        assert_eq!(hint("357 families"), "357 families");
        assert_eq!(bad("no font matched"), "no font matched");
    }
}
