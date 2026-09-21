//! Wave 1: lifecycle, incremental sync, and diagnostics.

mod common;

use common::{assert_codes, assert_incremental_sync, TempDir, TestClient};
use lsp_types::{Position, PositionEncodingKind, Range};
use sigil_lsp::DocumentTextRequest;

const BASE: &str = "namespace test\n\ntype Party:\n    name string (1..1)\n";
const MAIN: &str = "namespace test\n\ntype Trade:\n    party Party (1..1)\n";

#[test]
fn handshake_advertises_utf8_and_incremental_sync() {
    let tmp = TempDir::new("handshake");
    let client = TestClient::spawn_utf8(tmp.path());
    let caps = &client.server_capabilities;
    assert_eq!(
        caps.capabilities.position_encoding,
        Some(PositionEncodingKind::UTF8),
        "server must negotiate utf-8 when offered"
    );
    assert_incremental_sync(caps);
    client.shutdown();
}

#[test]
fn handshake_falls_back_to_utf16() {
    let tmp = TempDir::new("utf16");
    let mut client = TestClient::spawn_utf16(tmp.path());
    assert_eq!(
        client.server_capabilities.capabilities.position_encoding,
        Some(PositionEncodingKind::UTF16),
        "default LSP position encoding is UTF-16"
    );
    // And a roundtrip through the server still works.
    let u = tmp.write("a.rosetta", "namespace test\n");
    client.open(&u, "namespace test\n", 1);
    assert_eq!(client.server_text(&u).as_deref(), Some("namespace test\n"));
    client.shutdown();
}

#[test]
fn syntax_and_resolution_diagnostics() {
    let tmp = TempDir::new("diagnostics");
    let mut client = TestClient::spawn_utf8(tmp.path());

    // Unknown type: resolution diagnostic E0101 pointing at the reference.
    let u = tmp.write(
        "main.rosetta",
        "namespace test\n\ntype Foo:\n    value Unknown (1..1)\n",
    );
    client.open(
        &u,
        "namespace test\n\ntype Foo:\n    value Unknown (1..1)\n",
        1,
    );
    let diags = client.expect_diagnostics(&u, 1);
    assert_codes(&diags, &[("E0101", 3, 10)]);

    // Syntax error: E0001 from the parser.
    let v = tmp.write("broken.rosetta", "namespace test\n\ntype :\n");
    client.open(&v, "namespace test\n\ntype :\n", 1);
    let diags = client.expect_diagnostics(&v, 1);
    assert!(
        diags.iter().any(|d| common::code_of(d) == Some("E0001")),
        "expected a syntax error, got {diags:?}"
    );
    client.shutdown();
}

#[test]
fn clean_files_produce_no_diagnostics() {
    let tmp = TempDir::new("clean");
    let mut client = TestClient::spawn_utf8(tmp.path());
    let base = tmp.write("base.rosetta", BASE);
    let u = tmp.write("main.rosetta", MAIN);
    client.open(&base, BASE, 1);
    client.open(&u, MAIN, 1);
    let diags = client.expect_diagnostics(&u, 1);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
    client.shutdown();
}

#[test]
fn cross_file_resolution_between_two_open_files() {
    let tmp = TempDir::new("multifile");
    let mut client = TestClient::spawn_utf8(tmp.path());
    let base = tmp.write("base.rosetta", BASE);
    let main = tmp.write("main.rosetta", MAIN);

    client.open(&base, BASE, 1);
    let base_diags = client.expect_diagnostics(&base, 1);
    assert!(base_diags.is_empty(), "{base_diags:?}");

    client.open(&main, MAIN, 1);
    let main_diags = client.expect_diagnostics(&main, 1);
    assert!(
        main_diags.is_empty(),
        "`Party` in main must resolve into base: {main_diags:?}"
    );

    // Break the reference: `Party` no longer exists.
    let broken = "namespace test\n\ntype Trade:\n    party Partee (1..1)\n";
    client.change_full(&main, 2, broken);
    client.expect_diagnostics(&main, 2);
    assert_codes(&client.latest_diagnostics(&main), &[("E0101", 3, 10)]);

    // Fixing main clears the diagnostic again (full-workspace reanalysis).
    client.change_full(&main, 3, MAIN);
    client.expect_diagnostics(&main, 3);
    assert!(
        client.latest_diagnostics(&main).is_empty(),
        "fixed file must have no diagnostics"
    );
    client.shutdown();
}

#[test]
fn duplicate_in_main_reports_in_later_file_and_clears() {
    // Sorted analysis order: a_main.rosetta before z_base.rosetta, so the
    // duplicate `Widget` (the breakage lives in main) is reported in
    // z_base — a diagnostic in base caused by main, cleared by fixing main.
    let tmp = TempDir::new("duplicate");
    let mut client = TestClient::spawn_utf8(tmp.path());
    let main = tmp.write(
        "a_main.rosetta",
        "namespace test\n\ntype Widget:\n    x int (1..1)\n",
    );
    let base = tmp.write(
        "z_base.rosetta",
        "namespace test\n\ntype Widget:\n    y int (1..1)\n",
    );
    client.open(
        &main,
        "namespace test\n\ntype Widget:\n    x int (1..1)\n",
        1,
    );
    client.expect_diagnostics(&main, 1);
    client.open(
        &base,
        "namespace test\n\ntype Widget:\n    y int (1..1)\n",
        1,
    );
    let base_diags = client.expect_diagnostics(&base, 1);
    assert_codes(&base_diags, &[("E0104", 2, 0)]);

    // Fix main: rename its type.
    client.change(
        &main,
        2,
        vec![(
            Range {
                start: Position {
                    line: 2,
                    character: 5,
                },
                end: Position {
                    line: 2,
                    character: 11,
                },
            },
            "Gadget".to_string(),
        )],
    );
    client.expect_diagnostics(&main, 2);
    client.sync(&main);
    assert!(
        client.latest_diagnostics(&base).is_empty(),
        "fixing a_main must clear z_base: {:?}",
        client.latest_diagnostics(&base)
    );
    client.shutdown();
}

#[test]
fn incremental_change_updates_diagnostics() {
    let tmp = TempDir::new("incremental");
    let _base = tmp.write("base.rosetta", BASE); // so `Party` resolves
    let u = tmp.write("main.rosetta", MAIN);
    let mut client = TestClient::spawn_utf8(tmp.path());
    client.open(&u, MAIN, 1);
    client.expect_diagnostics(&u, 1);

    // Insert "x" before `Party` with a ranged edit: `Party` -> `xParty`.
    client.change(
        &u,
        2,
        vec![(
            Range {
                start: Position {
                    line: 3,
                    character: 10,
                },
                end: Position {
                    line: 3,
                    character: 10,
                },
            },
            "x".to_string(),
        )],
    );
    client.expect_diagnostics(&u, 2);
    assert_codes(&client.latest_diagnostics(&u), &[("E0101", 3, 10)]);
    assert_eq!(
        client.server_text(&u).as_deref(),
        Some("namespace test\n\ntype Trade:\n    party xParty (1..1)\n")
    );

    // Delete it again with a ranged delete.
    client.change(
        &u,
        3,
        vec![(
            Range {
                start: Position {
                    line: 3,
                    character: 10,
                },
                end: Position {
                    line: 3,
                    character: 11,
                },
            },
            String::new(),
        )],
    );
    client.expect_diagnostics(&u, 3);
    assert!(client.latest_diagnostics(&u).is_empty());
    client.shutdown();
}

#[test]
fn close_publishes_empty_diagnostics() {
    let tmp = TempDir::new("close");
    let mut client = TestClient::spawn_utf8(tmp.path());
    let u = tmp.write(
        "main.rosetta",
        "namespace test\n\ntype Foo:\n    value Unknown (1..1)\n",
    );
    client.open(
        &u,
        "namespace test\n\ntype Foo:\n    value Unknown (1..1)\n",
        1,
    );
    client.expect_diagnostics(&u, 1);
    client.close(&u);
    client.sync(&u);
    assert!(
        client.latest_diagnostics(&u).is_empty(),
        "closing must clear diagnostics: {:?}",
        client.latest_diagnostics(&u)
    );
    // And the document is gone from the server.
    assert_eq!(client.server_text(&u), None);
    client.shutdown();
}

#[test]
fn workspace_scan_loads_unopened_files() {
    let tmp = TempDir::new("scan");
    // Write both files to disk BEFORE initialize; only open `main`.
    let _base = tmp.write("base.rosetta", BASE);
    let main = tmp.write("main.rosetta", MAIN);
    let mut client = TestClient::spawn_utf8(tmp.path());
    client.open(&main, MAIN, 1);
    let diags = client.expect_diagnostics(&main, 1);
    assert!(
        diags.is_empty(),
        "`Party` must resolve against the scanned base.rosetta: {diags:?}"
    );
    client.shutdown();
}

#[test]
fn custom_text_request_reflects_changes() {
    let tmp = TempDir::new("textreq");
    let mut client = TestClient::spawn_utf8(tmp.path());
    let u = tmp.write("main.rosetta", "namespace test\n");
    client.open(&u, "namespace test\n", 1);
    client.change_full(&u, 2, "namespace other\n");
    assert_eq!(client.server_text(&u).as_deref(), Some("namespace other\n"));
    client.shutdown();
}

#[test]
fn unknown_document_text_is_none() {
    let tmp = TempDir::new("textreq-none");
    let mut client = TestClient::spawn_utf8(tmp.path());
    assert_eq!(
        client.request::<DocumentTextRequest>(document_text_params_for("file:///nowhere.rosetta")),
        None
    );
    client.shutdown();
}

fn document_text_params_for(u: &str) -> sigil_lsp::DocumentTextParams {
    sigil_lsp::DocumentTextParams {
        text_document: lsp_types::TextDocumentIdentifier {
            uri: common::uri(u),
        },
    }
}
