//! Wave 2: textDocument/documentSymbol.

mod common;

use common::{TempDir, TestClient};
use lsp_types::request::DocumentSymbolRequest;
use lsp_types::{DocumentSymbol, DocumentSymbolResponse, SymbolKind};

const DOC: &str = r#"namespace demo.symbols

import cdm.base.staticdata.*

type Trade: <"A trade">
    tradeId string (1..1) <"Identifier">
    [metadata id]
    quantity number (0..1)

choice Payout:
    InterestRatePayout <"Payout option">

enum Currency: <"ISO currencies">
    EUR displayName "Euro" <"Euro">
    USD displayName "US Dollar"

annotation metadata: <"Metadata annotation">
    scheme string (0..1)
"#;

fn flat(symbols: &[DocumentSymbol]) -> Vec<(String, SymbolKind, usize)> {
    let mut out = Vec::new();
    for s in symbols {
        out.push((
            s.name.clone(),
            s.kind,
            s.children.as_ref().map(|c| c.len()).unwrap_or(0),
        ));
        if let Some(children) = &s.children {
            out.extend(flat(children));
        }
    }
    out
}

#[test]
fn document_symbols_tree() {
    let tmp = TempDir::new("docsym");
    let u = tmp.write("symbols.rosetta", DOC);
    let mut client = TestClient::spawn_utf8(tmp.path());
    client.open(&u, DOC, 1);

    let symbols: Vec<DocumentSymbol> = nested_symbols(&mut client, &u);

    let got = flat(&symbols);
    let kind = |k: SymbolKind| k;
    assert_eq!(
        got,
        vec![
            ("demo.symbols".to_string(), kind(SymbolKind::NAMESPACE), 4),
            ("Trade".to_string(), kind(SymbolKind::STRUCT), 2),
            ("tradeId".to_string(), kind(SymbolKind::FIELD), 0),
            ("quantity".to_string(), kind(SymbolKind::FIELD), 0),
            ("Payout".to_string(), kind(SymbolKind::STRUCT), 1),
            ("InterestRatePayout".to_string(), kind(SymbolKind::FIELD), 0),
            ("Currency".to_string(), kind(SymbolKind::ENUM), 2),
            ("EUR".to_string(), kind(SymbolKind::ENUM_MEMBER), 0),
            ("USD".to_string(), kind(SymbolKind::ENUM_MEMBER), 0),
            ("metadata".to_string(), kind(SymbolKind::INTERFACE), 1),
            ("scheme".to_string(), kind(SymbolKind::FIELD), 0),
        ],
        "document symbol tree mismatch"
    );
    client.shutdown();
}

#[test]
fn document_symbols_ranges_point_at_names() {
    let tmp = TempDir::new("docsym-range");
    let u = tmp.write("symbols.rosetta", DOC);
    let mut client = TestClient::spawn_utf8(tmp.path());
    client.open(&u, DOC, 1);

    let symbols: Vec<DocumentSymbol> = nested_symbols(&mut client, &u);

    let ns = &symbols[0];
    assert_eq!(ns.name, "demo.symbols");
    // selection_range must cover the namespace name on line 0.
    assert_eq!(
        ns.selection_range.start,
        lsp_types::Position {
            line: 0,
            character: 10
        }
    );
    assert_eq!(ns.selection_range.end.character, 22);
    // range contains selection_range (LSP invariant).
    assert!(ns.range.start <= ns.selection_range.start);
    assert!(ns.selection_range.end <= ns.range.end);

    let trade = &ns.children.as_ref().unwrap()[0];
    assert_eq!(trade.name, "Trade");
    // `Trade` on line 4, columns 5..10.
    assert_eq!(
        trade.selection_range.start,
        lsp_types::Position {
            line: 4,
            character: 5
        }
    );
    assert_eq!(
        trade.selection_range.end,
        lsp_types::Position {
            line: 4,
            character: 10
        }
    );

    let attr = &trade.children.as_ref().unwrap()[0];
    assert_eq!(attr.name, "tradeId");
    assert_eq!(
        attr.selection_range.start,
        lsp_types::Position {
            line: 5,
            character: 4
        }
    );
    assert_eq!(
        attr.selection_range.end,
        lsp_types::Position {
            line: 5,
            character: 11
        }
    );

    let eur = &ns.children.as_ref().unwrap()[2].children.as_ref().unwrap()[0];
    assert_eq!(eur.name, "EUR");
    assert_eq!(
        eur.selection_range.start,
        lsp_types::Position {
            line: 13,
            character: 4
        }
    );

    client.shutdown();
}

#[test]
fn document_symbols_after_incremental_change() {
    let tmp = TempDir::new("docsym-edit");
    let u = tmp.write("symbols.rosetta", DOC);
    let mut client = TestClient::spawn_utf8(tmp.path());
    client.open(&u, DOC, 1);

    // Rename Trade -> Transaction via a ranged edit on line 4.
    client.change(
        &u,
        2,
        vec![(
            lsp_types::Range {
                start: lsp_types::Position {
                    line: 4,
                    character: 5,
                },
                end: lsp_types::Position {
                    line: 4,
                    character: 10,
                },
            },
            "Transaction".to_string(),
        )],
    );
    client.sync(&u);

    let symbols: Vec<DocumentSymbol> = nested_symbols(&mut client, &u);
    assert_eq!(symbols[0].children.as_ref().unwrap()[0].name, "Transaction");
    client.shutdown();
}

fn nested_symbols(client: &mut TestClient, u: &str) -> Vec<DocumentSymbol> {
    match client.request::<DocumentSymbolRequest>(lsp_types::DocumentSymbolParams {
        text_document: lsp_types::TextDocumentIdentifier {
            uri: common::uri(u),
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    }) {
        Some(DocumentSymbolResponse::Nested(symbols)) => symbols,
        other => panic!("expected nested document symbols, got {other:?}"),
    }
}
