//! textDocument/rename (issue #2, task 3): workspace-wide rename over the
//! references site index, with declaration edits and guard rails.

mod common;

use common::{TempDir, TestClient};
use lsp_types::request::{References, Rename};
use lsp_types::{
    Position, RenameParams, TextDocumentIdentifier, TextDocumentPositionParams, WorkspaceEdit,
};

const ONE: &str = r#"namespace test

type Party: <"A counterparty">
    name string (1..1)

type Portfolio:
    parties Party (0..*)
    main Party (1..1)
"#;

const BASE: &str = r#"namespace test

type Party: <"A counterparty">
    name string (1..1)

type Portfolio:
    parties Party (0..*)
"#;

const MAIN: &str = r#"namespace test

type Trade:
    party Party (1..1)
    counterparty Party (0..1)
"#;

const ANNO: &str = r#"namespace test

annotation metaLabel:
    id string (1..1)

type Card:
    [metaLabel id]
    number string (1..1)
"#;

const CARET: &str = r#"namespace test

type ^enum:
    tag string (1..1)

type Holder:
    flag ^enum (1..1)
"#;

/// (file, start line/char, end line/char, new text) for every edit in a
/// WorkspaceEdit, sorted.
type EditSummary = Vec<(String, u32, u32, u32, u32, String)>;

fn summary(edit: Option<WorkspaceEdit>) -> EditSummary {
    let edit = edit.expect("rename returned a WorkspaceEdit");
    let mut v: EditSummary = edit
        .changes
        .as_ref()
        .expect("WorkspaceEdit.changes present")
        .iter()
        .flat_map(|(uri, edits)| {
            edits.iter().map(move |e| {
                (
                    uri.to_string(),
                    e.range.start.line,
                    e.range.start.character,
                    e.range.end.line,
                    e.range.end.character,
                    e.new_text.clone(),
                )
            })
        })
        .collect();
    v.sort();
    v
}

fn rename_at(u: &str, line: u32, character: u32, new_name: &str) -> RenameParams {
    RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: common::uri(u),
            },
            position: Position { line, character },
        },
        new_name: new_name.to_string(),
        work_done_progress_params: Default::default(),
    }
}

#[test]
fn rename_capability_is_advertised() {
    let tmp = TempDir::new("rename-caps");
    let client = TestClient::spawn_utf8(tmp.path());
    assert!(
        client
            .server_capabilities
            .capabilities
            .rename_provider
            .is_some(),
        "renameProvider capability missing"
    );
    client.shutdown();
}

#[test]
fn rename_in_single_file_updates_declaration_and_references() {
    let tmp = TempDir::new("rename-one");
    let one = tmp.write("one.rosetta", ONE);
    let mut client = TestClient::spawn_utf8(tmp.path());
    client.open(&one, ONE, 1);
    client.sync(&one);

    // From the declaration: every site in the file, one edit per site.
    let edit = client
        .request::<Rename>(rename_at(&one, 2, 5, "Counterparty"))
        .expect("rename returned a WorkspaceEdit");
    assert_eq!(
        summary(Some(edit.clone())),
        vec![
            (one.clone(), 2, 5, 2, 10, "Counterparty".to_string()), // declaration
            (one.clone(), 6, 12, 6, 17, "Counterparty".to_string()), // parties …
            (one.clone(), 7, 9, 7, 14, "Counterparty".to_string()), // main …
        ],
        "expected declaration + 2 reference edits"
    );

    // The ranges must be exact: apply them like a client would and check
    // the renamed element keeps its references.
    let changes: Vec<_> = edit.changes.unwrap()[&common::uri(&one)]
        .iter()
        .map(|e| (e.range, e.new_text.clone()))
        .collect();
    client.change(&one, 2, changes);
    client.sync(&one);

    let refs = client
        .request::<References>(lsp_types::ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: common::uri(&one),
                },
                position: Position {
                    line: 2,
                    character: 5,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: lsp_types::ReferenceContext {
                include_declaration: false,
            },
        })
        .unwrap_or_default();
    let sites: Vec<(u32, u32)> = refs
        .iter()
        .map(|l| (l.range.start.line, l.range.start.character))
        .collect();
    assert_eq!(sites, vec![(6, 12), (7, 9)], "references after rename");
    client.shutdown();
}

#[test]
fn rename_across_files_from_a_reference() {
    let tmp = TempDir::new("rename-xfile");
    let base = tmp.write("base.rosetta", BASE);
    let main = tmp.write("main.rosetta", MAIN);
    let mut client = TestClient::spawn_utf8(tmp.path());
    client.open(&base, BASE, 1);
    client.open(&main, MAIN, 1);
    client.sync(&main);

    // Start from a reference in main: the declaration in base is renamed
    // too, and edits are grouped per file.
    let edit = client.request::<Rename>(rename_at(&main, 3, 10, "Counterparty"));
    assert_eq!(
        summary(edit),
        vec![
            (base.clone(), 2, 5, 2, 10, "Counterparty".to_string()), // declaration
            (base.clone(), 6, 12, 6, 17, "Counterparty".to_string()), // Portfolio.parties
            (main.clone(), 3, 10, 3, 15, "Counterparty".to_string()), // Trade.party
            (main.clone(), 4, 17, 4, 22, "Counterparty".to_string()), // Trade.counterparty
        ],
        "edits must span both files"
    );
    client.shutdown();
}

#[test]
fn rename_onto_existing_name_is_allowed() {
    let tmp = TempDir::new("rename-collide");
    let base = tmp.write("base.rosetta", BASE);
    let main = tmp.write("main.rosetta", MAIN);
    let mut client = TestClient::spawn_utf8(tmp.path());
    client.open(&base, BASE, 1);
    client.open(&main, MAIN, 1);
    client.sync(&main);

    // Renaming onto an existing name is a user decision (the resulting
    // ambiguity shows up in diagnostics), so the server must not reject it.
    let edit = client.request::<Rename>(rename_at(&base, 2, 5, "Portfolio"));
    let edits = summary(edit);
    assert_eq!(edits.len(), 4, "all four sites renamed: {edits:?}");
    assert!(edits.iter().all(|e| e.5 == "Portfolio"));
    client.shutdown();
}

#[test]
fn rename_annotation_reference() {
    let tmp = TempDir::new("rename-anno");
    let anno = tmp.write("anno.rosetta", ANNO);
    let mut client = TestClient::spawn_utf8(tmp.path());
    client.open(&anno, ANNO, 1);
    client.sync(&anno);

    // Cursor on the annotation name inside `[metaLabel id]`.
    let edit = client.request::<Rename>(rename_at(&anno, 6, 5, "metaTag"));
    assert_eq!(
        summary(edit),
        vec![
            (anno.clone(), 2, 11, 2, 20, "metaTag".to_string()), // declaration
            (anno.clone(), 6, 5, 6, 14, "metaTag".to_string()),  // [metaTag id]
        ],
        "annotation declaration + annotation reference"
    );
    client.shutdown();
}

#[test]
fn rename_strips_caret_escape_from_keyword_names() {
    let tmp = TempDir::new("rename-caret");
    let caret = tmp.write("caret.rosetta", CARET);
    let mut client = TestClient::spawn_utf8(tmp.path());
    client.open(&caret, CARET, 1);
    client.sync(&caret);

    // `^enum` is the escaped spelling of the name `enum`; the `^` belongs
    // to the token, so both edits must cover it (and drop it, since the
    // new name needs no escape).
    let edit = client.request::<Rename>(rename_at(&caret, 6, 10, "Choice"));
    assert_eq!(
        summary(edit),
        vec![
            (caret.clone(), 2, 5, 2, 10, "Choice".to_string()), // type ^enum:
            (caret.clone(), 6, 9, 6, 14, "Choice".to_string()), // flag ^enum
        ],
        "edits must include the ^ escape prefix"
    );
    client.shutdown();
}

#[test]
fn rename_rejects_invalid_names() {
    let tmp = TempDir::new("rename-invalid");
    let one = tmp.write("one.rosetta", ONE);
    let mut client = TestClient::spawn_utf8(tmp.path());
    client.open(&one, ONE, 1);
    client.sync(&one);

    const REQUEST_FAILED: i32 = -32803; // lsp_server::ErrorCode::RequestFailed

    let bad = [
        "", // empty
        "type",
        "enum",
        "namespace",
        "report", // Rune keyword tokens
        "fn",
        "trait", // Rust keywords
        "foo bar",
        "foo.bar",
        "1st",
        "café",
        "^enum", // illegal identifier shape
    ];
    for name in bad {
        let resp = client.request_raw::<Rename>(rename_at(&one, 2, 5, name));
        let err = resp
            .response_result
            .expect_err(&format!("'{name}' must be rejected"));
        assert_eq!(err.code, REQUEST_FAILED, "error code for '{name}'");
        assert!(
            err.message.starts_with("cannot rename"),
            "error message for '{name}': {}",
            err.message
        );
    }

    // Positive control: `condition` is keyword-spelled but explicitly
    // allowed by the grammar's ValidID rule.
    client
        .request::<Rename>(rename_at(&one, 2, 5, "condition"))
        .unwrap()
        .changes
        .expect("ValidID keyword names are accepted");
    client.shutdown();
}

#[test]
fn rename_rejects_unresolved_and_non_renameable_targets() {
    let tmp = TempDir::new("rename-targets");
    let one = tmp.write("one.rosetta", ONE);
    let mut client = TestClient::spawn_utf8(tmp.path());
    client.open(&one, ONE, 1);
    client.sync(&one);

    const REQUEST_FAILED: i32 = -32803;

    // No symbol under the cursor (inside the definition string).
    let resp = client.request_raw::<Rename>(rename_at(&one, 2, 12, "Whatever"));
    let err = resp.response_result.expect_err("no symbol → error");
    assert_eq!(err.code, REQUEST_FAILED);

    // Attributes/enum values have no reference index yet.
    let resp = client.request_raw::<Rename>(rename_at(&one, 3, 4, "Whatever"));
    let err = resp.response_result.expect_err("attribute rename → error");
    assert_eq!(err.code, REQUEST_FAILED);
    assert!(err.message.contains("attribute"), "{}", err.message);
    client.shutdown();
}

#[test]
fn rename_rejects_builtin_target() {
    let tmp = TempDir::new("rename-builtin");
    let one = tmp.write("one.rosetta", ONE);
    let mut client = TestClient::spawn_utf8(tmp.path());
    client.open(&one, ONE, 1);
    client.sync(&one);

    // `string` is the built-in basic type: declared under `builtin:`, so
    // it is read-only and must not be renamed (nor edited anywhere).
    const REQUEST_FAILED: i32 = -32803;
    let resp = client.request_raw::<Rename>(rename_at(&one, 3, 12, "Text"));
    let err = resp.response_result.expect_err("builtin target → error");
    assert_eq!(err.code, REQUEST_FAILED);
    assert!(err.message.contains("built-in"), "{}", err.message);
    client.shutdown();
}
