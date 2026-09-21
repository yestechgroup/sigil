//! Wave 4: textDocument/hover — golden tests for markdown content.

mod common;

use common::{TempDir, TestClient};
use lsp_types::request::HoverRequest;
use lsp_types::{HoverContents, Position, Uri};

const BASE: &str = r#"namespace test

type Party: <"A counterparty to the trade">
    name string (1..1)
"#;

const MAIN: &str = r#"namespace test

type Trade: <"A trade between parties">
    party Party (1..1)
    reference string (0..1) <"Where to find it">
    [metadata id]

enum Side: <"Trade sides">
    BUY displayName "Buy" <"Purchase side">
"#;

fn hover(client: &mut TestClient, u: &str, line: u32, character: u32) -> Option<lsp_types::Hover> {
    client.request::<HoverRequest>(lsp_types::HoverParams {
        text_document_position_params: lsp_types::TextDocumentPositionParams {
            text_document: lsp_types::TextDocumentIdentifier { uri: uri_of(u) },
            position: Position { line, character },
        },
        work_done_progress_params: Default::default(),
    })
}

fn uri_of(u: &str) -> Uri {
    common::uri(u)
}

fn col(text: &str, line: usize, needle: &str) -> u32 {
    text.lines().nth(line).unwrap().find(needle).unwrap() as u32
}

fn markdown(h: &lsp_types::Hover) -> String {
    match &h.contents {
        HoverContents::Markup(m) => m.value.clone(),
        other => panic!("expected markup hover, got {other:?}"),
    }
}

#[test]
fn hover_on_type_reference_shows_target_and_definition() {
    let tmp = TempDir::new("hover");
    let base = tmp.write("base.rosetta", BASE);
    let main = tmp.write("main.rosetta", MAIN);
    let mut client = TestClient::spawn_utf8(tmp.path());
    client.open(&base, BASE, 1);
    client.open(&main, MAIN, 1);
    client.sync(&main);

    // Hover on `Party` in main (line 3) — golden markdown.
    let h = hover(&mut client, &main, 3, col(MAIN, 3, "Party")).expect("hover");
    assert_eq!(
        markdown(&h),
        "**type** `test.Party`\n\n---\n\nA counterparty to the trade"
    );
    // The hover range covers exactly the reference text.
    let range = h.range.expect("hover range");
    assert_eq!(
        range.start,
        Position {
            line: 3,
            character: col(MAIN, 3, "Party")
        }
    );
    assert_eq!(
        range.end,
        Position {
            line: 3,
            character: col(MAIN, 3, "Party") + "Party".len() as u32
        }
    );

    // Hover on the same type's declaration in base — same content.
    let h = hover(&mut client, &base, 2, col(BASE, 2, "Party")).expect("hover");
    assert_eq!(
        markdown(&h),
        "**type** `test.Party`\n\n---\n\nA counterparty to the trade"
    );
    let _ = base;
    client.shutdown();
}

#[test]
fn hover_on_attribute_shows_resolved_type_and_cardinality() {
    let tmp = TempDir::new("hover-attr");
    let base = tmp.write("base.rosetta", BASE);
    let main = tmp.write("main.rosetta", MAIN);
    let mut client = TestClient::spawn_utf8(tmp.path());
    client.open(&base, BASE, 1);
    client.open(&main, MAIN, 1);
    client.sync(&main);

    // `party` attribute name (line 3, first word).
    let h = hover(&mut client, &main, 3, col(MAIN, 3, "party")).expect("hover");
    assert_eq!(
        markdown(&h),
        "**attribute** `Trade.party`: `test.Party` (1..1)"
    );

    // Attribute with its own definition and a builtin resolved type.
    let h = hover(&mut client, &main, 4, col(MAIN, 4, "reference")).expect("hover");
    assert_eq!(
        markdown(&h),
        "**attribute** `Trade.reference`: `com.rosetta.model.string` (0..1)\n\n---\n\nWhere to find it"
    );
    client.shutdown();
}

#[test]
fn hover_on_builtin_type_reference() {
    let tmp = TempDir::new("hover-builtin");
    let main = tmp.write("main.rosetta", MAIN);
    let mut client = TestClient::spawn_utf8(tmp.path());
    client.open(&main, MAIN, 1);
    client.sync(&main);

    // `string` resolves into com.rosetta.model: hover shows the builtin
    // basic type and its definition text from the builtin file.
    let h = hover(&mut client, &main, 4, col(MAIN, 4, "string")).expect("hover");
    let md = markdown(&h);
    assert!(
        md.starts_with("**basicType** `com.rosetta.model.string`"),
        "unexpected hover: {md}"
    );
    client.shutdown();
}

#[test]
fn hover_on_enum_value() {
    let tmp = TempDir::new("hover-enum");
    let main = tmp.write("main.rosetta", MAIN);
    let mut client = TestClient::spawn_utf8(tmp.path());
    client.open(&main, MAIN, 1);
    client.sync(&main);

    let h = hover(&mut client, &main, 8, col(MAIN, 8, "BUY")).expect("hover");
    assert_eq!(
        markdown(&h),
        "**enum value** `Side.BUY` — display: \"Buy\"\n\n---\n\nPurchase side"
    );
    client.shutdown();
}

#[test]
fn hover_nowhere_is_none() {
    let tmp = TempDir::new("hover-none");
    let main = tmp.write("main.rosetta", MAIN);
    let mut client = TestClient::spawn_utf8(tmp.path());
    client.open(&main, MAIN, 1);
    client.sync(&main);

    // Blank line, and whitespace between tokens.
    assert_eq!(hover(&mut client, &main, 1, 0), None);
    assert_eq!(hover(&mut client, &main, 6, 0), None);
    client.shutdown();
}
