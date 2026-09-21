//! Open documents and incremental text synchronization.

use lsp_types::{Range, TextDocumentContentChangeEvent};

use crate::position::{LineIndex, PositionEncoding};

/// One open `.rosetta` document plus its precomputed line index.
#[derive(Debug, Clone)]
pub struct Document {
    pub text: String,
    pub version: i32,
    pub line_index: LineIndex,
}

impl Document {
    pub fn new(text: impl Into<String>, version: i32) -> Self {
        let text = text.into();
        let line_index = LineIndex::new(&text);
        Document {
            text,
            version,
            line_index,
        }
    }

    /// Apply a `textDocument/didChange` batch: content changes in order,
    /// then bump the version. Ranged changes are resolved against the text
    /// *as mutated by the preceding changes in the same batch*, per LSP.
    pub fn apply_content_changes(
        &mut self,
        version: i32,
        changes: impl IntoIterator<Item = TextDocumentContentChangeEvent>,
        enc: PositionEncoding,
    ) {
        for change in changes {
            match change.range {
                None => self.text = change.text,
                Some(range) => {
                    let start = self.line_index.offset(&self.text, range.start, enc);
                    let end = self.line_index.offset(&self.text, range.end, enc);
                    if start <= end && end <= self.text.len() {
                        self.text.replace_range(start..end, &change.text);
                    }
                }
            }
            self.line_index = LineIndex::new(&self.text);
        }
        self.version = version;
    }

    /// Byte offset of an LSP range start/end in this document.
    pub fn offset(&self, pos: lsp_types::Position, enc: PositionEncoding) -> usize {
        self.line_index.offset(&self.text, pos, enc)
    }

    /// LSP range for a byte span in this document.
    pub fn range(&self, span: sigil_diag::Span, enc: PositionEncoding) -> Range {
        Range {
            start: self.line_index.position(&self.text, span.start, enc),
            end: self.line_index.position(&self.text, span.end, enc),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::{Position, TextDocumentContentChangeEvent};

    fn ranged(start: (u32, u32), end: (u32, u32), text: &str) -> TextDocumentContentChangeEvent {
        TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position::new(start.0, start.1),
                end: Position::new(end.0, end.1),
            }),
            range_length: None,
            text: text.to_string(),
        }
    }

    fn full(text: &str) -> TextDocumentContentChangeEvent {
        TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: text.to_string(),
        }
    }

    #[test]
    fn full_replacement() {
        let mut doc = Document::new("abc", 0);
        doc.apply_content_changes(1, [full("€日\nx")], PositionEncoding::Utf8);
        assert_eq!(doc.text, "€日\nx");
        assert_eq!(doc.version, 1);
    }

    #[test]
    fn incremental_insert_delete_replace() {
        let mut doc = Document::new("hello world\n", 0);
        // insert "big " before "world"
        doc.apply_content_changes(1, [ranged((0, 6), (0, 6), "big ")], PositionEncoding::Utf8);
        assert_eq!(doc.text, "hello big world\n");
        // delete "big "
        doc.apply_content_changes(2, [ranged((0, 6), (0, 10), "")], PositionEncoding::Utf8);
        assert_eq!(doc.text, "hello world\n");
        // replace "world" with "€uro日"
        doc.apply_content_changes(
            3,
            [ranged((0, 6), (0, 11), "€uro日")],
            PositionEncoding::Utf8,
        );
        assert_eq!(doc.text, "hello €uro日\n");
    }

    #[test]
    fn sequential_edits_in_one_batch_see_prior_state() {
        let mut doc = Document::new("ab", 0);
        // First edit replaces everything; second edit addresses the new text.
        doc.apply_content_changes(
            1,
            [ranged((0, 0), (0, 2), "xyz"), ranged((0, 3), (0, 3), "!")],
            PositionEncoding::Utf8,
        );
        assert_eq!(doc.text, "xyz!");
    }
}
