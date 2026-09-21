//! Wave 3: textDocument/definition (cross-file, into builtins).

mod common;

use common::{TempDir, TestClient};
use lsp_types::request::GotoDefinition;
use lsp_types::{Position, Uri};

const BASE: &str = r#"namespace test

type Party: <"A counterparty">
    name string (1..1)

type Base:
    id string (1..1)
"#;

const MAIN: &str = r#"namespace test

import test.Party

annotation external: <"Marks external sources">

type Trade extends Base: <"A trade">
    party Party (1..1)
    [metadata id]
    [external]
"#;

fn define(
    client: &mut TestClient,
    u: &str,
    line: u32,
    character: u32,
) -> Option<lsp_types::Location> {
    let response = client.request::<GotoDefinition>(lsp_types::GotoDefinitionParams {
        text_document_position_params: lsp_types::TextDocumentPositionParams {
            text_document: lsp_types::TextDocumentIdentifier { uri: uri_of(u) },
            position: Position { line, character },
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    });
    match response {
        Some(lsp_types::GotoDefinitionResponse::Scalar(loc)) => Some(loc),
        Some(lsp_types::GotoDefinitionResponse::Array(mut locs)) => {
            assert!(locs.len() <= 1, "expected at most one location");
            locs.pop()
        }
        _ => None,
    }
}

fn uri_of(u: &str) -> Uri {
    common::uri(u)
}

/// Character offset of `needle` within `line` of `text`.
fn col(text: &str, line: usize, needle: &str) -> u32 {
    text.lines().nth(line).unwrap().find(needle).unwrap() as u32
}

#[test]
fn definition_on_type_reference_cross_file() {
    let tmp = TempDir::new("defn");
    let base = tmp.write("base.rosetta", BASE);
    let main = tmp.write("main.rosetta", MAIN);
    let mut client = TestClient::spawn_utf8(tmp.path());
    client.open(&base, BASE, 1);
    client.open(&main, MAIN, 1);
    client.sync(&main);

    // `Party` in `party Party (1..1)` (line 7) — definition lands in base.
    let loc = define(&mut client, &main, 7, col(MAIN, 7, "Party")).expect("location");
    assert_eq!(loc.uri.to_string(), base);
    assert_eq!(
        loc.range.start,
        Position {
            line: 2,
            character: 0
        }
    );

    // `Base` in `extends Base` (line 6) — cross-file.
    let loc = define(&mut client, &main, 6, col(MAIN, 6, "Base")).expect("location");
    assert_eq!(loc.uri.to_string(), base);
    assert_eq!(
        loc.range.start,
        Position {
            line: 5,
            character: 0
        }
    );

    // `string` in base (line 3) — a builtin basic type: definition points
    // into the builtin pseudo-document.
    let loc = define(&mut client, &base, 3, col(BASE, 3, "string")).expect("builtin location");
    assert_eq!(
        loc.uri.to_string(),
        "builtin:basictypes.rosetta",
        "builtin targets point at the builtin pseudo-document"
    );

    client.shutdown();
}

#[test]
fn definition_on_annotation_reference() {
    let tmp = TempDir::new("defn-anno");
    let main = tmp.write("main.rosetta", MAIN);
    let mut client = TestClient::spawn_utf8(tmp.path());
    client.open(&main, MAIN, 1);
    client.sync(&main);

    // `[external]` (line 9) resolves to the local annotation declaration.
    let loc = define(&mut client, &main, 9, col(MAIN, 9, "external")).expect("location");
    assert_eq!(loc.uri.to_string(), main);
    assert_eq!(
        loc.range.start,
        Position {
            line: 4,
            character: 0
        }
    );

    // `[metadata id]` (line 8) resolves into the builtin annotations library.
    let loc = define(&mut client, &main, 8, col(MAIN, 8, "metadata")).expect("location");
    assert_eq!(loc.uri.to_string(), "builtin:annotations.rosetta");

    client.shutdown();
}

#[test]
fn definition_on_declaration_is_itself() {
    let tmp = TempDir::new("defn-self");
    let main = tmp.write("main.rosetta", MAIN);
    let mut client = TestClient::spawn_utf8(tmp.path());
    client.open(&main, MAIN, 1);
    client.sync(&main);

    // On `Trade` in its own declaration (line 6).
    let loc = define(&mut client, &main, 6, col(MAIN, 6, "Trade")).expect("location");
    assert_eq!(loc.uri.to_string(), main);
    assert_eq!(
        loc.range.start,
        Position {
            line: 6,
            character: 0
        }
    );
    client.shutdown();
}

#[test]
fn definition_unknown_is_none() {
    let tmp = TempDir::new("defn-none");
    let text = "namespace test\n\ntype Foo:\n    party Missing (1..1)\n";
    let main = tmp.write("main.rosetta", text);
    let mut client = TestClient::spawn_utf8(tmp.path());
    client.open(&main, text, 1);
    client.sync(&main);

    assert_eq!(define(&mut client, &main, 3, col(text, 3, "Missing")), None);
    // And in the middle of nowhere: no crash, no location.
    assert_eq!(define(&mut client, &main, 0, 2), None);
    client.shutdown();
}
