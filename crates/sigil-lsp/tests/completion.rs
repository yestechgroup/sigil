//! Wave 5: textDocument/completion — context-aware suggestions.

mod common;

use common::{TempDir, TestClient};
use lsp_types::request::Completion;
use lsp_types::{CompletionContext, CompletionItemKind, CompletionParams, Position, Uri};

const BASE: &str = "namespace test\n\ntype Party:\n    name string (1..1)\n";

const MAIN: &str = "namespace test\n\ntype Trade:\n    party Party (1..1)\n";

fn complete(
    client: &mut TestClient,
    u: &str,
    line: u32,
    character: u32,
) -> lsp_types::CompletionList {
    let items = client.request::<Completion>(CompletionParams {
        text_document_position: lsp_types::TextDocumentPositionParams {
            text_document: lsp_types::TextDocumentIdentifier { uri: uri_of(u) },
            position: Position { line, character },
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: Some(CompletionContext {
            trigger_kind: lsp_types::CompletionTriggerKind::INVOKED,
            trigger_character: None,
        }),
    });
    match items {
        Some(lsp_types::CompletionResponse::List(list)) => list,
        Some(lsp_types::CompletionResponse::Array(items)) => lsp_types::CompletionList {
            is_incomplete: false,
            items,
        },
        None => lsp_types::CompletionList {
            is_incomplete: false,
            items: Vec::new(),
        },
    }
}

fn uri_of(u: &str) -> Uri {
    common::uri(u)
}

fn labels(list: &lsp_types::CompletionList) -> Vec<String> {
    list.items.iter().map(|i| i.label.clone()).collect()
}

#[test]
fn completion_in_type_reference_position() {
    let tmp = TempDir::new("compl-type");
    let base = tmp.write("base.rosetta", BASE);
    let main = tmp.write("main.rosetta", MAIN);
    let mut client = TestClient::spawn_utf8(tmp.path());
    client.open(&base, BASE, 1);
    // Complete right after `party ` on line 3 (before the newline).
    let line = "namespace test\n\ntype Trade:\n    party \n";
    client.open(&main, line, 1);
    client.sync(&main);

    let list = complete(&mut client, &main, 3, 10);
    let got = labels(&list);
    assert!(
        got.contains(&"Party".to_string()),
        "user type missing: {got:?}"
    );
    assert!(
        got.contains(&"int".to_string()) && got.contains(&"string".to_string()),
        "builtin types missing: {got:?}"
    );
    // Party's item carries its FQN as detail.
    let party = list
        .items
        .iter()
        .find(|i| i.label == "Party")
        .expect("Party item");
    assert_eq!(party.detail.as_deref(), Some("test.Party"));
    assert_eq!(party.kind, Some(CompletionItemKind::CLASS));
    // And it is sorted, deterministic.
    let mut sorted = got.clone();
    sorted.sort();
    assert_eq!(got, sorted);
    client.shutdown();
}

#[test]
fn completion_filters_by_partial() {
    let tmp = TempDir::new("compl-filter");
    let _base = tmp.write("base.rosetta", BASE); // workspace scan makes Party visible
    let main = tmp.write("main.rosetta", MAIN);
    let mut client = TestClient::spawn_utf8(tmp.path());
    let line = "namespace test\n\ntype Trade:\n    party Pa\n";
    client.open(&main, line, 1);
    client.sync(&main);

    let got = labels(&complete(&mut client, &main, 3, 11));
    assert!(got.contains(&"Party".to_string()), "{got:?}");
    assert!(!got.contains(&"string".to_string()), "unfiltered: {got:?}");
    client.shutdown();
}

#[test]
fn completion_inside_annotation_bracket_offers_attributes() {
    let tmp = TempDir::new("compl-anno");
    let main = tmp.write("main.rosetta", MAIN);
    let mut client = TestClient::spawn_utf8(tmp.path());
    let line = "namespace test\n\ntype Trade:\n    party Party (1..1)\n    [metadata \n";
    client.open(&main, line, 1);
    client.sync(&main);

    let got = labels(&complete(&mut client, &main, 4, 14));
    for expected in ["id", "key", "reference", "scheme", "location", "address"] {
        assert!(
            got.contains(&expected.to_string()),
            "metadata.{expected} missing: {got:?}"
        );
    }
    assert!(
        !got.contains(&"metadata".to_string()),
        "annotation names must not leak: {got:?}"
    );
    client.shutdown();
}

#[test]
fn completion_after_open_bracket_offers_annotation_names() {
    let tmp = TempDir::new("compl-anno-name");
    let main = tmp.write("main.rosetta", MAIN);
    let mut client = TestClient::spawn_utf8(tmp.path());
    let line = "namespace test\n\ntype Trade:\n    party Party (1..1)\n    [met\n";
    client.open(&main, line, 1);
    client.sync(&main);

    let got = labels(&complete(&mut client, &main, 4, 8));
    assert!(got.contains(&"metadata".to_string()), "{got:?}");
    assert!(
        !got.contains(&"scheme".to_string()),
        "attributes are not annotations: {got:?}"
    );
    client.shutdown();
}

#[test]
fn completion_top_level_keywords() {
    let tmp = TempDir::new("compl-kw");
    let main = tmp.write("main.rosetta", MAIN);
    let mut client = TestClient::spawn_utf8(tmp.path());
    let text = "namespace test\n\nen\n";
    client.open(&main, text, 1);
    client.sync(&main);

    let got = labels(&complete(&mut client, &main, 2, 2));
    assert!(got.contains(&"enum".to_string()), "{got:?}");
    assert!(
        !got.contains(&"type".to_string()),
        "only prefix matches: {got:?}"
    );

    // Empty line at top level: the whole keyword set is offered.
    let text = "namespace test\n\n\n";
    client.change_full(&main, 2, text);
    client.sync(&main);
    let got = labels(&complete(&mut client, &main, 2, 0));
    for expected in ["type", "choice", "enum", "annotation", "typeAlias"] {
        assert!(
            got.contains(&expected.to_string()),
            "{expected} missing: {got:?}"
        );
    }
    client.shutdown();
}

#[test]
fn completion_inside_enum_body_offers_displayname() {
    let tmp = TempDir::new("compl-enum");
    let main = tmp.write("main.rosetta", MAIN);
    let mut client = TestClient::spawn_utf8(tmp.path());
    let line = "namespace test\n\nenum Side:\n    BUY \n";
    client.open(&main, line, 1);
    client.sync(&main);

    let got = labels(&complete(&mut client, &main, 3, 8));
    assert_eq!(got, vec!["displayName".to_string()], "{got:?}");
    client.shutdown();
}

#[test]
fn completion_respects_incremental_edits() {
    let tmp = TempDir::new("compl-edit");
    let main = tmp.write("main.rosetta", MAIN);
    let mut client = TestClient::spawn_utf8(tmp.path());
    client.open(&main, MAIN, 1);
    client.sync(&main);

    // Type an incomplete annotation on a new attribute line.
    let insert = "\n    [me";
    let start = Position {
        line: 4,
        character: 0,
    };
    client.change(
        &main,
        2,
        vec![(lsp_types::Range { start, end: start }, insert.to_string())],
    );
    client.sync(&main);

    let got = labels(&complete(&mut client, &main, 5, 7));
    assert!(got.contains(&"metadata".to_string()), "{got:?}");
    client.shutdown();
}
