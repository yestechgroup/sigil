//! Property test: incremental didChange sync (Wave 0/1).
//!
//! Random sequences of ranged edits are applied to both the client's local
//! mirror and the server. After every step the server's text (via the
//! `sigil/textDocument` request, which doubles as a FIFO barrier) must equal
//! the mirror. The alphabet deliberately includes multi-byte characters so
//! edit ranges cross UTF-8 character boundaries.

mod common;

use proptest::prelude::*;
use sigil_lsp::position::{LineIndex, PositionEncoding};

use common::TestClient;

/// An edit operation, described in *character* indices so the strategy can
/// be independent of the intermediate text length; bounds are clamped at
/// application time.
#[derive(Debug, Clone)]
struct EditOp {
    /// Char index where the edit starts.
    start: usize,
    /// Number of chars to delete.
    del: usize,
    /// Replacement text.
    ins: String,
}

fn edit_strategy() -> impl Strategy<Value = EditOp> {
    (
        0usize..30,
        0usize..8,
        prop::collection::vec(any_char_or_nl(), 0..6),
    )
        .prop_map(|(start, del, ins)| EditOp {
            start,
            del,
            ins: ins.into_iter().collect(),
        })
}

fn any_char_or_nl() -> impl Strategy<Value = char> {
    prop_oneof![
        Just('\n'),
        Just('€'),
        Just('日'),
        Just(' '),
        // Identifiers keep edits inside plausible token positions.
        Just('x'),
        Just('y'),
        Just('_'),
        Just('('),
        Just(')'),
    ]
}

fn initial_text() -> impl Strategy<Value = String> {
    proptest::collection::vec(any_char_or_nl(), 0..40).prop_map(|chars| chars.into_iter().collect())
}

/// Clamp an edit to the current text (char-aware) and return the byte range
/// plus the post-edit text.
fn apply_to_mirror(text: &mut String, op: &EditOp, enc: PositionEncoding) -> lsp_types::Range {
    let chars: Vec<char> = text.chars().collect();
    let start_c = op.start.min(chars.len());
    let end_c = (start_c + op.del).min(chars.len());
    let byte_of = |char_idx: usize| chars[..char_idx].iter().map(|c| c.len_utf8()).sum();
    let start = byte_of(start_c);
    let end = byte_of(end_c);

    let idx = LineIndex::new(text);
    let range = lsp_types::Range {
        start: idx.position(text, start, enc),
        end: idx.position(text, end, enc),
    };
    text.replace_range(start..end, &op.ins);
    range
}

proptest! {
    #![proptest_config(prop::test_runner::Config {
        cases: std::env::var("PROPTEST_CASES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(128),
        max_shrink_iters: 512,
        ..prop::test_runner::Config::default()
    })]

    #[test]
    fn incremental_sync_matches_mirror_utf8(
        initial in initial_text(),
        ops in proptest::collection::vec(edit_strategy(), 1..24),
    ) {
        let tmp = common::TempDir::new("syncprop");
        let uri = tmp.write("doc.rosetta", &initial);
        let mut client = TestClient::spawn_utf8(tmp.path());
        let enc = PositionEncoding::Utf8; // negotiated: tests advertise UTF-8

        let mut mirror = initial.clone();
        client.open(&uri, &initial, 1);
        prop_assert_eq!(client.server_text(&uri), Some(initial.clone()));

        for (i, op) in ops.iter().enumerate() {
            let range = apply_to_mirror(&mut mirror, op, enc);
            client.change(&uri, i as i32 + 2, vec![(range, op.ins.clone())]);
            prop_assert_eq!(
                client.server_text(&uri),
                Some(mirror.clone()),
                "after edit {} ({:?})",
                i,
                op
            );
        }
        client.shutdown();
    }

    #[test]
    fn incremental_sync_matches_mirror_utf16(
        initial in initial_text(),
        ops in proptest::collection::vec(edit_strategy(), 1..16),
    ) {
        // Same protocol over the UTF-16 fallback encoding.
        let tmp = common::TempDir::new("syncprop16");
        let uri = tmp.write("doc.rosetta", &initial);
        let mut client = TestClient::spawn_utf16(tmp.path());
        let enc = PositionEncoding::Utf16;

        let mut mirror = initial.clone();
        client.open(&uri, &initial, 1);
        prop_assert_eq!(client.server_text(&uri), Some(initial.clone()));

        for (i, op) in ops.iter().enumerate() {
            let range = apply_to_mirror(&mut mirror, op, enc);
            client.change(&uri, i as i32 + 2, vec![(range, op.ins.clone())]);
            prop_assert_eq!(
                client.server_text(&uri),
                Some(mirror.clone()),
                "after edit {} ({:?})",
                i,
                op
            );
        }
        client.shutdown();
    }

    #[test]
    fn batched_edits_apply_in_order(
        initial in initial_text(),
        batch in proptest::collection::vec(edit_strategy(), 2..6),
    ) {
        // Several edits in ONE didChange batch: ranges address the text as
        // left by the previous edit in the batch.
        let tmp = common::TempDir::new("syncbatch");
        let uri = tmp.write("doc.rosetta", &initial);
        let mut client = TestClient::spawn_utf8(tmp.path());
        let enc = PositionEncoding::Utf8;

        let mut mirror = initial.clone();
        client.open(&uri, &initial, 1);

        let mut edits = Vec::new();
        for op in &batch {
            let range = apply_to_mirror(&mut mirror, op, enc);
            edits.push((range, op.ins.clone()));
        }
        client.change(&uri, 2, edits);
        prop_assert_eq!(client.server_text(&uri), Some(mirror.clone()));
        client.shutdown();
    }
}
