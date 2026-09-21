//! LSP position <-> byte offset conversion.
//!
//! LSP positions are (line, character) pairs. Since LSP 3.17 the character
//! unit is negotiable via `general.positionEncodings`; sigil-lsp prefers
//! UTF-8 code units (i.e. plain byte offsets within the line) and falls back
//! to the LSP default of UTF-16 code units when the client does not offer
//! UTF-8. [`LineIndex`] converts between the two worlds; this module is the
//! single place where positions are computed and is property-tested
//! (`tests/positions.rs`).

use lsp_types::{Position, PositionEncodingKind};

/// The character unit used for LSP positions, negotiated at initialize time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionEncoding {
    /// Positions count UTF-8 bytes (what the parser's byte spans map to
    /// directly).
    Utf8,
    /// Positions count UTF-16 code units (the LSP default).
    Utf16,
}

impl PositionEncoding {
    /// Pick the encoding from the client's advertised list: UTF-8 if
    /// offered, otherwise the LSP-mandated UTF-16 fallback.
    pub fn negotiate(client_offers: Option<&[PositionEncodingKind]>) -> Self {
        let offered = match client_offers {
            Some(list) => list,
            None => return PositionEncoding::Utf16,
        };
        if offered.iter().any(|k| k == &PositionEncodingKind::UTF8) {
            PositionEncoding::Utf8
        } else {
            PositionEncoding::Utf16
        }
    }

    /// The `PositionEncodingKind` to advertise in `InitializeResult`.
    pub fn as_kind(self) -> PositionEncodingKind {
        match self {
            PositionEncoding::Utf8 => PositionEncodingKind::UTF8,
            PositionEncoding::Utf16 => PositionEncodingKind::UTF16,
        }
    }
}

/// Precomputed line-start offsets for a document text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineIndex {
    line_starts: Vec<usize>,
    /// Byte length of the text the index was built for.
    len: usize,
}

impl LineIndex {
    pub fn new(text: &str) -> Self {
        let mut line_starts = vec![0usize];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        LineIndex {
            line_starts,
            len: text.len(),
        }
    }

    pub fn line_count(&self) -> u32 {
        self.line_starts.len() as u32
    }

    fn line_start(&self, line: u32) -> Option<usize> {
        self.line_starts.get(line as usize).copied()
    }

    /// End offset of `line` (the offset of its `\n`, or end of text).
    fn line_end(&self, line: u32) -> usize {
        match self.line_start(line + 1) {
            Some(next) => next - 1, // the '\n' itself
            None => self.len,
        }
    }

    /// Convert an LSP position to a byte offset. Out-of-range lines and
    /// columns are clamped to the nearest valid offset (clients routinely
    /// overshoot at EOF); a column landing inside a multi-byte character is
    /// clamped down to the character start.
    pub fn offset(&self, text: &str, pos: Position, enc: PositionEncoding) -> usize {
        let line = pos.line.min(self.line_count().saturating_sub(1));
        let start = self.line_start(line).unwrap_or(0);
        let end = self.line_end(line);
        let line_text = text.get(start..end).unwrap_or("");
        let col = pos.character;
        let within = match enc {
            PositionEncoding::Utf8 => {
                let mut col = (col as usize).min(line_text.len());
                while col > 0 && !line_text.is_char_boundary(col) {
                    col -= 1;
                }
                col
            }
            PositionEncoding::Utf16 => {
                let mut units = 0u32;
                let mut bytes = 0usize;
                for ch in line_text.chars() {
                    if units + ch.len_utf16() as u32 > col {
                        break;
                    }
                    units += ch.len_utf16() as u32;
                    bytes += ch.len_utf8();
                }
                bytes
            }
        };
        start + within
    }

    /// Convert a byte offset to an LSP position. Offsets beyond the end of
    /// the text are clamped to the end; offsets inside a multi-byte
    /// character are clamped down to the character start.
    pub fn position(&self, text: &str, offset: usize, enc: PositionEncoding) -> Position {
        let offset = offset.min(text.len());
        let offset = if text.is_char_boundary(offset) {
            offset
        } else {
            let mut o = offset;
            while o > 0 && !text.is_char_boundary(o) {
                o -= 1;
            }
            o
        };
        let line = self
            .line_starts
            .partition_point(|&start| start <= offset)
            .saturating_sub(1) as u32;
        let start = self.line_start(line).unwrap_or(0);
        // NB: `\r` is treated as an ordinary character (no trimming before
        // `\n`) so that offset -> position -> offset is a strict bijection.
        let line_text = &text[start..offset];
        let character = match enc {
            PositionEncoding::Utf8 => (offset - start) as u32,
            PositionEncoding::Utf16 => line_text.chars().map(|c| c.len_utf16() as u32).sum(),
        };
        Position { line, character }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "type Foo:\n    attr int (1..1)\n";

    #[test]
    fn utf8_round_trip_and_values() {
        let idx = LineIndex::new(TEXT);
        assert_eq!(idx.line_count(), 3);
        let p = idx.position(TEXT, 7, PositionEncoding::Utf8);
        assert_eq!(
            p,
            Position {
                line: 0,
                character: 7
            }
        );
        assert_eq!(idx.offset(TEXT, p, PositionEncoding::Utf8), 7);
        // start of line 1 (after "type Foo:\n" = 10 bytes)
        let p = idx.position(TEXT, 10, PositionEncoding::Utf8);
        assert_eq!(
            p,
            Position {
                line: 1,
                character: 0
            }
        );
    }

    #[test]
    fn utf16_counts_code_units() {
        // `€` is one char, 3 UTF-8 bytes, 1 UTF-16 unit. `𐐷` is 4 bytes,
        // 2 UTF-16 units.
        let text = "a€𐐷b";
        let idx = LineIndex::new(text);
        assert_eq!(
            idx.position(text, 1, PositionEncoding::Utf16),
            Position {
                line: 0,
                character: 1
            }
        );
        assert_eq!(
            idx.position(text, 4, PositionEncoding::Utf16),
            Position {
                line: 0,
                character: 2
            }
        );
        assert_eq!(
            idx.position(text, 8, PositionEncoding::Utf16),
            Position {
                line: 0,
                character: 4
            }
        );
        // And back again.
        assert_eq!(
            idx.offset(text, Position::new(0, 2), PositionEncoding::Utf16),
            4
        );
        assert_eq!(
            idx.offset(text, Position::new(0, 4), PositionEncoding::Utf16),
            8
        );
    }

    #[test]
    fn clamps_overshooting_positions() {
        let idx = LineIndex::new(TEXT);
        // One past the last line.
        let p = Position {
            line: 99,
            character: 0,
        };
        assert_eq!(idx.offset(TEXT, p, PositionEncoding::Utf8), TEXT.len());
        // Column past end of line 0 ("type Foo:" is 9 bytes).
        let p = Position {
            line: 0,
            character: 999,
        };
        assert_eq!(idx.offset(TEXT, p, PositionEncoding::Utf8), 9);
        // Offset past end of text.
        let p = idx.position(TEXT, 9999, PositionEncoding::Utf8);
        assert_eq!(p.line, 2);
        assert_eq!(p.character, 0);
    }

    #[test]
    fn offset_inside_multibyte_char_clamps_down() {
        let text = "a€b";
        let idx = LineIndex::new(text);
        // Middle of the `€` (UTF-8 mode): clamp to the character start.
        assert_eq!(
            idx.offset(text, Position::new(0, 2), PositionEncoding::Utf8),
            1
        );
    }

    #[test]
    fn empty_text_has_one_line() {
        let idx = LineIndex::new("");
        assert_eq!(idx.line_count(), 1);
        let p = idx.position("", 0, PositionEncoding::Utf16);
        assert_eq!(
            p,
            Position {
                line: 0,
                character: 0
            }
        );
    }
}
