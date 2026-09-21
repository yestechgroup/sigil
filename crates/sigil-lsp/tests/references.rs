//! Wave 6: textDocument/references + workspace/symbol.

mod common;

use common::{TempDir, TestClient};
use lsp_types::request::{References, WorkspaceSymbolRequest};
use lsp_types::{Position, Uri};

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

fn references(
    client: &mut TestClient,
    u: &str,
    line: u32,
    character: u32,
    include_declaration: bool,
) -> Vec<lsp_types::Location> {
    client
        .request::<References>(lsp_types::ReferenceParams {
            text_document_position: lsp_types::TextDocumentPositionParams {
                text_document: lsp_types::TextDocumentIdentifier { uri: uri_of(u) },
                position: Position { line, character },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: lsp_types::ReferenceContext {
                include_declaration,
            },
        })
        .unwrap_or_default()
}

fn workspace_symbols(client: &mut TestClient, query: &str) -> Vec<lsp_types::SymbolInformation> {
    match client.request::<WorkspaceSymbolRequest>(lsp_types::WorkspaceSymbolParams {
        query: query.to_string(),
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    }) {
        Some(lsp_types::WorkspaceSymbolResponse::Flat(syms)) => syms,
        #[allow(deprecated)]
        Some(lsp_types::WorkspaceSymbolResponse::Nested(syms)) => syms
            .into_iter()
            .map(|s| lsp_types::SymbolInformation {
                name: s.name,
                kind: s.kind,
                tags: s.tags,
                deprecated: None,
                location: match s.location {
                    lsp_types::OneOf::Left(loc) => loc,
                    lsp_types::OneOf::Right(loc) => lsp_types::Location {
                        uri: loc.uri,
                        range: Default::default(),
                    },
                },
                container_name: s.container_name,
            })
            .collect(),
        None => Vec::new(),
    }
}

fn uri_of(u: &str) -> Uri {
    common::uri(u)
}

fn summary(locs: &[lsp_types::Location]) -> Vec<(String, u32, u32)> {
    let mut v: Vec<_> = locs
        .iter()
        .map(|l| {
            (
                l.uri.to_string(),
                l.range.start.line,
                l.range.start.character,
            )
        })
        .collect();
    v.sort();
    v
}

#[test]
fn references_find_all_type_usages() {
    let tmp = TempDir::new("refs");
    let base = tmp.write("base.rosetta", BASE);
    let main = tmp.write("main.rosetta", MAIN);
    let mut client = TestClient::spawn_utf8(tmp.path());
    client.open(&base, BASE, 1);
    client.open(&main, MAIN, 1);
    client.sync(&main);

    // From the declaration in base (line 2), with declaration included.
    let refs = references(&mut client, &base, 2, 5, true);
    assert_eq!(
        summary(&refs),
        vec![
            (base.clone(), 2, 0),  // declaration
            (base.clone(), 6, 12), // Portfolio.parties
            (main.clone(), 3, 10), // Trade.party
            (main.clone(), 4, 17), // Trade.counterparty
        ],
        "all references to test.Party"
    );

    // Without the declaration.
    let refs = references(&mut client, &base, 2, 5, false);
    assert_eq!(
        summary(&refs),
        vec![
            (base.clone(), 6, 12),
            (main.clone(), 3, 10),
            (main.clone(), 4, 17),
        ]
    );

    // From a usage site in main — same answer set.
    let refs = references(&mut client, &main, 3, 10, true);
    assert_eq!(
        summary(&refs),
        vec![
            (base.clone(), 2, 0),
            (base.clone(), 6, 12),
            (main.clone(), 3, 10),
            (main.clone(), 4, 17),
        ]
    );
    client.shutdown();
}

#[test]
fn references_after_incremental_edit() {
    let tmp = TempDir::new("refs-edit");
    let base = tmp.write("base.rosetta", BASE);
    let main = tmp.write("main.rosetta", MAIN);
    let mut client = TestClient::spawn_utf8(tmp.path());
    client.open(&base, BASE, 1);
    client.open(&main, MAIN, 1);
    client.sync(&main);

    // Rename `counterparty` usage to a different type: replace one `Party`.
    client.change(
        &main,
        2,
        vec![(
            lsp_types::Range {
                start: Position {
                    line: 4,
                    character: 17,
                },
                end: Position {
                    line: 4,
                    character: 22,
                },
            },
            "string".to_string(),
        )],
    );
    client.sync(&main);

    let refs = references(&mut client, &base, 2, 5, false);
    assert_eq!(
        summary(&refs),
        vec![(base.clone(), 6, 12), (main.clone(), 3, 10)],
        "the edited usage dropped out"
    );
    client.shutdown();
}

#[test]
fn workspace_symbol_search() {
    let tmp = TempDir::new("wssym");
    let base = tmp.write("base.rosetta", BASE);
    let main = tmp.write("main.rosetta", MAIN);
    let mut client = TestClient::spawn_utf8(tmp.path());
    client.open(&base, BASE, 1);
    client.open(&main, MAIN, 1);
    client.sync(&main);

    // Substring over FQNs, case-insensitive.
    let syms = workspace_symbols(&mut client, "trad");
    assert_eq!(syms.len(), 1, "{syms:?}");
    assert_eq!(syms[0].name, "Trade");
    assert_eq!(syms[0].container_name.as_deref(), Some("test"));
    assert_eq!(syms[0].location.uri.to_string(), main);
    assert_eq!(
        syms[0].location.range.start,
        Position {
            line: 2,
            character: 0
        }
    );

    let syms = workspace_symbols(&mut client, "test.party");
    let mut names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
    names.sort();
    assert_eq!(names, vec!["Party"]);

    // Builtin elements are searchable too.
    let syms = workspace_symbols(&mut client, "serializationformat");
    assert!(
        syms.iter().any(|s| s.name == "SerializationFormat"),
        "builtin SerializationFormat missing: {:?}",
        syms.iter().map(|s| s.name.clone()).collect::<Vec<_>>()
    );
    client.shutdown();
}
