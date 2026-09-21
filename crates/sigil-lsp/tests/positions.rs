//! Property tests for LSP position <-> byte offset conversion (Wave 0).

use proptest::prelude::*;
use sigil_lsp::position::{LineIndex, PositionEncoding};

fn alphabet() -> impl Strategy<Value = char> {
    prop_oneof![
        Just('a'),
        Just('Z'),
        Just(' '),
        Just('\n'),
        Just('€'),
        Just('日'),
        Just('𐐷'),
        Just('.'),
    ]
}

fn text() -> impl Strategy<Value = String> {
    proptest::collection::vec(alphabet(), 0..40).prop_map(|chars| chars.into_iter().collect())
}

proptest! {
    #![proptest_config(prop::test_runner::Config {
        cases: std::env::var("PROPTEST_CASES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(256),
        ..prop::test_runner::Config::default()
    })]

    #[test]
    fn offset_position_round_trip(text in text(), offset in 0usize..200) {
        let idx = LineIndex::new(&text);
        let clamped = {
            let o = offset.min(text.len());
            let mut c = o;
            while c > 0 && !text.is_char_boundary(c) {
                c -= 1;
            }
            c
        };
        for enc in [PositionEncoding::Utf8, PositionEncoding::Utf16] {
            let pos = idx.position(&text, offset, enc);
            let back = idx.offset(&text, pos, enc);
            prop_assert_eq!(back, clamped, "enc {:?} text {:?} offset {}", enc, text, offset);
        }
    }

    #[test]
    fn position_offset_round_trip(text in text(), line in 0u32..50, character in 0u32..120) {
        let idx = LineIndex::new(&text);
        let pos = lsp_types::Position { line, character };
        for enc in [PositionEncoding::Utf8, PositionEncoding::Utf16] {
            let offset = idx.offset(&text, pos, enc);
            // Offset side must be a fixpoint of position().
            let pos2 = idx.position(&text, offset, enc);
            prop_assert_eq!(idx.offset(&text, pos2, enc), offset, "enc {:?} text {:?}", enc, text);
            // Canonical position: lines saturate, columns never point past
            // the line end.
            prop_assert!(pos2.line < idx.line_count().max(1));
            prop_assert_eq!(pos2, idx.position(&text, offset, enc));
        }
    }

    #[test]
    fn utf8_and_utf16_agree_on_ascii_lines(text in text()) {
        // On pure-ASCII texts both encodings must produce identical
        // positions and offsets.
        if text.is_ascii() {
            let idx = LineIndex::new(&text);
            for offset in 0..=text.len() {
                let u8p = idx.position(&text, offset, PositionEncoding::Utf8);
                let u16p = idx.position(&text, offset, PositionEncoding::Utf16);
                prop_assert_eq!(u8p, u16p);
                prop_assert_eq!(
                    idx.offset(&text, u8p, PositionEncoding::Utf8),
                    idx.offset(&text, u16p, PositionEncoding::Utf16)
                );
            }
        }
    }
}
