//! textDocument/semanticTokens (issue #2, task 4): exact highlighting for
//! types vs enums vs annotations vs keywords, grounded in resolution.

mod common;

use common::{TempDir, TestClient};
use lsp_types::request::SemanticTokensFullRequest;
use lsp_types::{
    SemanticTokens, SemanticTokensFullOptions, SemanticTokensParams,
    SemanticTokensServerCapabilities, TextDocumentIdentifier,
};

const FIXTURE: &str = r#"namespace test

// leading comment
annotation metaLabel:
    id string (1..1)

/* block
   comment */
type Party:
    name string (1..1)

enum Currency: <"ISO codes">
    EUR displayName "Euro"
    USD

type Trade: <"A trade">
    [metaLabel id]
    buyer Party (1..1)
    currency Currency (1..1)
    amount number (1..1)
"#;

/// The legend as flat name lists, for decoding token indices.
fn legend(client: &TestClient) -> (Vec<String>, Vec<String>) {
    match client
        .server_capabilities
        .capabilities
        .semantic_tokens_provider
        .as_ref()
        .expect("semanticTokensProvider capability")
    {
        SemanticTokensServerCapabilities::SemanticTokensOptions(opts) => (
            opts.legend
                .token_types
                .iter()
                .map(|t| t.as_str().to_string())
                .collect(),
            opts.legend
                .token_modifiers
                .iter()
                .map(|m| m.as_str().to_string())
                .collect(),
        ),
        other => panic!("unexpected semanticTokensProvider shape: {other:?}"),
    }
}

fn request_tokens(client: &mut TestClient, uri: &str) -> SemanticTokens {
    // `SemanticTokensFullRequest::Result` is already an `Option`.
    let result = client.request::<SemanticTokensFullRequest>(SemanticTokensParams {
        text_document: TextDocumentIdentifier {
            uri: common::uri(uri),
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    });
    match result {
        Some(lsp_types::SemanticTokensResult::Tokens(tokens)) => tokens,
        other => panic!("unexpected semantic tokens result: {other:?}"),
    }
}

/// Decode the relative-delta encoding into absolute
/// `(line, char, length, type name, modifier names)` tuples.
fn decode(
    client: &TestClient,
    tokens: SemanticTokens,
) -> Vec<(u32, u32, u32, String, Vec<String>)> {
    let (types, modifiers) = legend(client);
    let mut out = Vec::new();
    let (mut line, mut character) = (0u32, 0u32);
    for t in &tokens.data {
        line += t.delta_line;
        if t.delta_line > 0 {
            character = t.delta_start;
        } else {
            character += t.delta_start;
        }
        let token_type = types
            .get(t.token_type as usize)
            .unwrap_or_else(|| panic!("token type index {} not in legend", t.token_type))
            .clone();
        let mods: Vec<String> = modifiers
            .iter()
            .enumerate()
            .filter(|(i, _)| t.token_modifiers_bitset & (1 << i) != 0)
            .map(|(_, m)| m.clone())
            .collect();
        out.push((line, character, t.length, token_type, mods));
    }
    out
}

fn expected_mod(mods: &[&str]) -> Vec<String> {
    mods.iter().map(|m| m.to_string()).collect()
}

#[test]
fn semantic_tokens_capability_is_advertised() {
    let tmp = TempDir::new("semtok-caps");
    let client = TestClient::spawn_utf8(tmp.path());
    match client
        .server_capabilities
        .capabilities
        .semantic_tokens_provider
        .as_ref()
        .expect("semanticTokensProvider capability missing")
    {
        SemanticTokensServerCapabilities::SemanticTokensOptions(opts) => {
            assert_eq!(opts.full, Some(SemanticTokensFullOptions::Bool(true)));
            assert_eq!(opts.range, None, "only full requests are supported");
            let types: Vec<&str> = opts.legend.token_types.iter().map(|t| t.as_str()).collect();
            for wanted in [
                "namespace",
                "type",
                "enum",
                "enumMember",
                "decorator",
                "function",
                "property",
                "keyword",
                "comment",
                "string",
                "number",
            ] {
                assert!(
                    types.contains(&wanted),
                    "legend missing '{wanted}': {types:?}"
                );
            }
        }
        other => panic!("unexpected semanticTokensProvider shape: {other:?}"),
    }
    client.shutdown();
}

#[test]
fn semantic_tokens_full_golden() {
    let tmp = TempDir::new("semtok-golden");
    let uri = tmp.write("fixture.rosetta", FIXTURE);
    let mut client = TestClient::spawn_utf8(tmp.path());
    client.open(&uri, FIXTURE, 1);
    client.sync(&uri);

    let tokens = request_tokens(&mut client, &uri);
    let got = decode(&client, tokens);
    let decl = expected_mod(&["declaration"]);
    let no_mods: Vec<String> = Vec::new();

    // Every keyword position, declaration name, comment, string, number and
    // resolved reference in the fixture, exactly.
    assert_eq!(
        got,
        vec![
            (0, 0, 9, "keyword".into(), no_mods.clone()), // namespace
            (0, 10, 4, "namespace".into(), decl.clone()), // test
            (2, 0, 18, "comment".into(), no_mods.clone()), // // leading comment
            (3, 0, 10, "keyword".into(), no_mods.clone()), // annotation
            (3, 11, 9, "decorator".into(), decl.clone()), // metaLabel (decl)
            (4, 4, 2, "property".into(), decl.clone()),   // id
            (4, 7, 6, "type".into(), no_mods.clone()),    // string (builtin)
            (4, 15, 1, "number".into(), no_mods.clone()), // (1..1)
            (4, 18, 1, "number".into(), no_mods.clone()),
            (6, 0, 8, "comment".into(), no_mods.clone()), // /* block
            (7, 0, 13, "comment".into(), no_mods.clone()), //    comment */
            (8, 0, 4, "keyword".into(), no_mods.clone()), // type
            (8, 5, 5, "type".into(), decl.clone()),       // Party (decl)
            (9, 4, 4, "property".into(), decl.clone()),   // name
            (9, 9, 6, "type".into(), no_mods.clone()),    // string
            (9, 17, 1, "number".into(), no_mods.clone()),
            (9, 20, 1, "number".into(), no_mods.clone()),
            (11, 0, 4, "keyword".into(), no_mods.clone()), // enum
            (11, 5, 8, "enum".into(), decl.clone()),       // Currency (decl)
            (11, 16, 11, "string".into(), no_mods.clone()), // "ISO codes"
            (12, 4, 3, "enumMember".into(), decl.clone()), // EUR
            (12, 8, 11, "keyword".into(), no_mods.clone()), // displayName
            (12, 20, 6, "string".into(), no_mods.clone()), // "Euro"
            (13, 4, 3, "enumMember".into(), decl.clone()), // USD
            (15, 0, 4, "keyword".into(), no_mods.clone()), // type
            (15, 5, 5, "type".into(), decl.clone()),       // Trade (decl)
            (15, 13, 9, "string".into(), no_mods.clone()), // "A trade"
            (16, 5, 9, "decorator".into(), no_mods.clone()), // metaLabel (ref)
            (16, 15, 2, "property".into(), no_mods.clone()), // id (anno attr)
            (17, 4, 5, "property".into(), decl.clone()),   // buyer
            (17, 10, 5, "type".into(), no_mods.clone()),   // Party (resolved)
            (17, 17, 1, "number".into(), no_mods.clone()),
            (17, 20, 1, "number".into(), no_mods.clone()),
            (18, 4, 8, "property".into(), decl.clone()), // currency
            (18, 13, 8, "enum".into(), no_mods.clone()), // Currency ref → enum kind
            (18, 23, 1, "number".into(), no_mods.clone()),
            (18, 26, 1, "number".into(), no_mods.clone()),
            (19, 4, 6, "property".into(), decl.clone()), // amount
            (19, 11, 6, "type".into(), no_mods.clone()), // number (builtin)
            (19, 19, 1, "number".into(), no_mods.clone()),
            (19, 22, 1, "number".into(), no_mods.clone()),
        ],
        "unexpected semantic token stream"
    );
    client.shutdown();
}

#[test]
fn semantic_tokens_reflect_edits() {
    let tmp = TempDir::new("semtok-edit");
    let uri = tmp.write(
        "edit.rosetta",
        "namespace test\n\ntype Trade:\n    id string (1..1)\n",
    );
    let mut client = TestClient::spawn_utf8(tmp.path());
    client.open(
        &uri,
        "namespace test\n\ntype Trade:\n    id string (1..1)\n",
        1,
    );
    client.sync(&uri);

    let tokens = request_tokens(&mut client, &uri);
    let before = decode(&client, tokens);
    assert!(before.iter().any(|t| t.0 == 2
        && t.1 == 5
        && t.2 == 5
        && t.3 == "type"
        && !t.4.is_empty()));

    let edited = "namespace test\n\ntype Counterparty:\n    id string (1..1)\n";
    client.change_full(&uri, 2, edited);
    client.sync(&uri);

    let tokens = request_tokens(&mut client, &uri);
    let after = decode(&client, tokens);
    assert!(
        after
            .iter()
            .any(|t| t.0 == 2 && t.1 == 5 && t.2 == 12 && t.3 == "type" && !t.4.is_empty()),
        "declaration token must track the edited text: {after:?}"
    );
    client.shutdown();
}

#[test]
fn semantic_tokens_use_negotiated_encoding() {
    // `€` is 3 UTF-8 bytes but 1 UTF-16 code unit: the string token length
    // must follow the negotiated position encoding.
    let text = "namespace test\n\ntype T: <\"€ trade\">\n";
    let tmp8 = TempDir::new("semtok-u8");
    let uri8 = tmp8.write("u8.rosetta", text);
    let mut client8 = TestClient::spawn_utf8(tmp8.path());
    client8.open(&uri8, text, 1);
    client8.sync(&uri8);
    let tokens = request_tokens(&mut client8, &uri8);
    let utf8 = decode(&client8, tokens);
    assert_eq!(
        utf8.last(),
        Some(&(2, 9, 11, "string".to_string(), Vec::new()))
    );
    client8.shutdown();

    let tmp16 = TempDir::new("semtok-u16");
    let uri16 = tmp16.write("u16.rosetta", text);
    let mut client16 = TestClient::spawn_utf16(tmp16.path());
    client16.open(&uri16, text, 1);
    client16.sync(&uri16);
    let tokens = request_tokens(&mut client16, &uri16);
    let utf16 = decode(&client16, tokens);
    assert_eq!(
        utf16.last(),
        Some(&(2, 9, 9, "string".to_string(), Vec::new()))
    );
    client16.shutdown();
}
