//! Source file handling, spans, and diagnostics.

use std::fmt;

/// Byte-offset span into a source file (start inclusive, end exclusive).
///
/// This is the fundamental location type; it deliberately has no dependency
/// on any parser library so that diagnostics can be produced by any layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub fn point(at: usize) -> Self {
        Self { start: at, end: at }
    }

    /// Smallest span covering both `self` and `other`.
    pub fn merge(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

impl From<std::ops::Range<usize>> for Span {
    fn from(r: std::ops::Range<usize>) -> Self {
        Span::new(r.start, r.end)
    }
}

impl From<Span> for std::ops::Range<usize> {
    fn from(s: Span) -> Self {
        s.start..s.end
    }
}

/// Convert a byte offset in `text` to a 1-based (line, column) pair.
///
/// Lines are delimited by `\n`. Columns count Unicode scalar values (plain
/// characters) — *not* UTF-16 code units as LSP would, and not bytes — so
/// positions stay human-meaningful for any UTF-8 input.
pub fn line_col_in(text: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(text.len());
    let prefix = &text[..offset];
    let line = prefix.bytes().filter(|&b| b == b'\n').count() + 1;
    let line_start = prefix.rfind('\n').map(|i| i + 1).unwrap_or(0);
    (line, text[line_start..offset].chars().count() + 1)
}

/// A source file: name plus text.
#[derive(Debug, Clone)]
pub struct SourceFile {
    pub name: String,
    pub text: String,
}

impl SourceFile {
    pub fn new(name: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            text: text.into(),
        }
    }

    /// Convert a byte offset to a 1-based (line, column) pair.
    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        line_col_in(&self.text, offset)
    }

    /// The line of text containing `offset`, trimmed of trailing newline.
    pub fn line_text(&self, offset: usize) -> &str {
        let offset = offset.min(self.text.len());
        let start = self.text[..offset].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let end = self.text[offset..]
            .find('\n')
            .map(|i| offset + i)
            .unwrap_or(self.text.len());
        &self.text[start..end]
    }

    pub fn snippet(&self, span: Span) -> String {
        let (line, col) = self.line_col(span.start);
        let src_line = self.line_text(span.start);
        let _width = span.end.saturating_sub(span.start).max(1);
        format!("{}:{}:{}: {}", self.name, line, col, src_line)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warning,
}

impl serde::Serialize for Severity {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A structured diagnostic with a span resolved into human-readable form.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
    pub span: Span,
    pub file: String,
    pub notes: Vec<String>,
}

impl Diagnostic {
    pub fn error(
        code: &'static str,
        message: impl Into<String>,
        file: &SourceFile,
        span: Span,
    ) -> Self {
        Diagnostic {
            severity: Severity::Error,
            code,
            message: message.into(),
            span,
            file: file.name.clone(),
            notes: Vec::new(),
        }
    }

    pub fn warning(
        code: &'static str,
        message: impl Into<String>,
        file: &SourceFile,
        span: Span,
    ) -> Self {
        Diagnostic {
            severity: Severity::Warning,
            code,
            message: message.into(),
            span,
            file: file.name.clone(),
            notes: Vec::new(),
        }
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    pub fn render(&self, files: &[(String, String)]) -> String {
        let source = files
            .iter()
            .find(|(n, _)| *n == self.file)
            .map(|(_, t)| SourceFile::new(self.file.clone(), t.clone()));
        match source {
            Some(file) => {
                let (line, col) = file.line_col(self.span.start);
                let mut out = format!(
                    "{}:{}:{}: {}: {}\n{}",
                    self.file,
                    line,
                    col,
                    self.severity,
                    self.message,
                    file.snippet(self.span)
                );
                let mut caret = String::new();
                for _ in 1..col {
                    caret.push(' ');
                }
                let width = (self.span.end.saturating_sub(self.span.start)).clamp(1, 80);
                for _ in 0..width {
                    caret.push('^');
                }
                out.push_str(&format!("\n        {caret}"));
                let _ = width;
                for note in &self.notes {
                    out.push_str(&format!("\n        note: {note}"));
                }
                out
            }
            None => format!("{}: {}: {}", self.severity, self.file, self.message),
        }
    }
}

/// Accumulates diagnostics across files and phases.
#[derive(Debug, Default, Clone)]
pub struct DiagnosticEngine {
    pub diagnostics: Vec<Diagnostic>,
}

impl DiagnosticEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, d: Diagnostic) {
        self.diagnostics.push(d);
    }

    pub fn extend(&mut self, other: DiagnosticEngine) {
        self.diagnostics.extend(other.diagnostics);
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }

    pub fn error_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_col_basic() {
        let f = SourceFile::new("a.rosetta", "hello\nworld\n");
        assert_eq!(f.line_col(0), (1, 1));
        assert_eq!(f.line_col(3), (1, 4));
        assert_eq!(f.line_col(6), (2, 1));
        assert_eq!(f.line_col(9), (2, 4));
    }

    #[test]
    fn line_col_counts_characters_not_bytes() {
        let f = SourceFile::new("a.rosetta", "héllo\nx");
        let offset = f.text.find('o').unwrap();
        assert_eq!(f.line_col(offset), (1, 5));
    }

    #[test]
    fn snippet_points_at_line() {
        let f = SourceFile::new("a.rosetta", "type Foo:\n    attr int (1..1)\n");
        let s = f.snippet(Span::new(14, 18));
        assert!(s.contains("a.rosetta:2:5"), "{s}");
        assert!(s.contains("attr int"), "{s}");
    }
}
