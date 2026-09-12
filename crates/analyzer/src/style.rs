//! Colour as a parameter: one decision, made by the host, threaded by value
//! through every rendering function.
//!
//! Whether output gets ANSI escapes depends on facts only the host knows —
//! is the destination a terminal, did the user ask for none, is `NO_COLOR`
//! set — so this module makes no such decision. It offers [`Styles::new`]
//! for a host that has decided, and the semantic paint functions the renderer
//! calls. The bytes written are the same modulo the escape sequences, so a
//! diff of a coloured and a plain rendering is empty once the escapes are
//! stripped; that is what makes redirecting to a file safe.

use surrealql_analyzer_diagnostics::Severity;

/// ANSI SGR parameters, spelled once.
mod sgr {
    pub(super) const RESET: &str = "\x1b[0m";
    pub(super) const BOLD: &str = "1";
    pub(super) const RED: &str = "31";
    pub(super) const YELLOW: &str = "33";
    pub(super) const BLUE: &str = "34";
    pub(super) const CYAN: &str = "36";
}

/// Whether rendered text gets ANSI escapes.
///
/// Copied rather than borrowed everywhere: it is one `bool`, and threading it
/// by value keeps every rendering function pure in it — which is what lets the
/// tests render the same finding twice, coloured and plain, and compare.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Styles {
    color: bool,
}

impl Styles {
    /// Escapes on or off, as the host decided.
    pub const fn new(color: bool) -> Self {
        Self { color }
    }

    /// Never emits an escape sequence — for a file, a pipe, a CI log, or a
    /// machine-readable document.
    pub const fn plain() -> Self {
        Self::new(false)
    }

    /// Always emits escape sequences.
    pub const fn colored() -> Self {
        Self::new(true)
    }

    /// Wraps `text` in `codes` (semicolon-joined SGR parameters), or returns it
    /// untouched when colour is off.
    fn paint(self, text: &str, codes: &str) -> String {
        if self.color {
            format!("\x1b[{codes}m{text}{}", sgr::RESET)
        } else {
            text.to_string()
        }
    }

    /// `error` / `warning` / `hint`, in the severity's colour.
    pub fn severity(self, severity: Severity, text: &str) -> String {
        self.paint(text, &format!("{};{}", sgr::BOLD, severity_color(severity)))
    }

    /// The diagnostic message, bold so it wins the line against the code.
    pub fn message(self, text: &str) -> String {
        self.paint(text, sgr::BOLD)
    }

    /// The `-->` and `|` scaffolding around a source excerpt.
    pub fn frame(self, text: &str) -> String {
        self.paint(text, &format!("{};{}", sgr::BOLD, sgr::BLUE))
    }

    /// A file path, so the eye finds "where" without reading the whole line.
    pub fn path(self, text: &str) -> String {
        self.paint(text, sgr::CYAN)
    }

    /// The caret underline, in the colour of whatever it underlines: the
    /// severity for the primary span, blue for a related one.
    pub fn carets(self, text: &str, severity: Option<Severity>) -> String {
        let color = severity.map_or(sgr::BLUE, severity_color);
        self.paint(text, &format!("{};{color}", sgr::BOLD))
    }

    /// The `help:` / `note:` labels.
    pub fn label(self, text: &str) -> String {
        self.paint(text, &format!("{};{}", sgr::BOLD, sgr::CYAN))
    }
}

fn severity_color(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => sgr::RED,
        Severity::Warning => sgr::YELLOW,
        Severity::Hint => sgr::CYAN,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_never_gets_escapes() {
        let styles = Styles::plain();
        assert_eq!(styles.message("hello"), "hello");
        assert_eq!(styles.severity(Severity::Error, "error"), "error");
        assert_eq!(styles.frame("-->"), "-->");
    }

    #[test]
    fn painting_is_the_same_text_plus_escapes() {
        // Colour must never change *what* is written, only how it is dressed —
        // otherwise piped output and terminal output would say different
        // things, and only one of them could be right.
        let plain = Styles::plain();
        let colored = Styles::colored();
        for (painted, bare) in [
            (colored.message("m"), plain.message("m")),
            (colored.path("p"), plain.path("p")),
            (colored.frame("-->"), plain.frame("-->")),
            (colored.label("help:"), plain.label("help:")),
            (colored.carets("^^", None), plain.carets("^^", None)),
        ] {
            assert!(
                painted.starts_with("\x1b["),
                "expected escapes: {painted:?}"
            );
            assert!(
                painted.ends_with("\x1b[0m"),
                "expected a reset: {painted:?}"
            );
            assert!(
                painted.contains(&bare),
                "the text itself must survive: {painted:?} vs {bare:?}"
            );
        }
    }

    #[test]
    fn each_severity_gets_its_own_color() {
        let styles = Styles::colored();
        let error = styles.severity(Severity::Error, "error");
        let warning = styles.severity(Severity::Warning, "warning");
        assert!(error.contains("31"), "errors are red: {error:?}");
        assert!(warning.contains("33"), "warnings are yellow: {warning:?}");
    }
}
