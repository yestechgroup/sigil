//! Schema format validation: `schema Name Format` requires `Format` to be
//! a value of the in-scope `SerializationFormat` enumeration (shadowing
//! respected, mirroring the transform-ref scope).

use sigil_diag::SourceFile;
use sigil_resolve::{canonical_json, resolve};

fn resolved(src: &str) -> serde_json::Value {
    let source = SourceFile::new("test.rosetta", src.to_string());
    let (unit, syntax) = sigil_syntax::parse(&source);
    assert!(syntax.is_empty(), "{syntax:?}");
    let model = sigil_syntax::lower("test.rosetta", &unit.unwrap());
    let resolution = resolve(vec![model]);
    canonical_json(&resolution, &[("test.rosetta".into(), src.to_string())])
}

#[test]
fn valid_format_resolves_clean() {
    let src = r#"
namespace test.format.valid

schema XmlSchema XML <"standard format">
schema FixSchema FIX <"fixed-width format">
"#;
    let json = resolved(src);
    let diags = json["diagnostics"].as_array().unwrap();
    assert!(diags.is_empty(), "{diags:?}");
}

#[test]
fn invalid_format_is_diagnosed_with_span() {
    let src = r#"
namespace test.format.invalid

schema BadSchema FIXML <"not a SerializationFormat value">
"#;
    let json = resolved(src);
    let diags = json["diagnostics"].as_array().unwrap();
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0]["code"], "E0107");
    assert_eq!(diags[0]["severity"], "error");
    assert!(
        diags[0]["message"]
            .as_str()
            .unwrap()
            .contains("'FIXML' is not a value of"),
        "{diags:?}"
    );
    // The diagnostic points at the format token (`FIXML`).
    let line = src.lines().position(|l| l.contains("FIXML")).unwrap() + 1;
    let col = src.lines().nth(line - 1).unwrap().find("FIXML").unwrap() + 1;
    assert_eq!(diags[0]["span"]["start"]["line"], line);
    assert_eq!(diags[0]["span"]["start"]["column"], col);
}

#[test]
fn shadowing_enum_is_respected() {
    // A user-defined `SerializationFormat` shadows the builtin: its
    // literals are the valid formats in this file.
    let src = r#"
namespace test.format.shadow

schema TradingSchema FIXML <"local enum has this value">

enum SerializationFormat:
	JSON displayName "JSON"
	FIXML displayName "FIXML"
"#;
    let json = resolved(src);
    let diags = json["diagnostics"].as_array().unwrap();
    assert!(diags.is_empty(), "{diags:?}");
}

#[test]
fn canonical_json_keeps_format_as_written() {
    let src = r#"
namespace test.format.written

schema S RUNE_JSON <"kept raw">
"#;
    let json = resolved(src);
    let schema = json["files"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|f| f["elements"].as_array().unwrap())
        .find(|e| e["kind"] == "Schema")
        .unwrap();
    assert_eq!(schema["format"], "RUNE_JSON");
}

#[test]
fn format_resolved_links_the_in_scope_enum() {
    let src = r#"
namespace test.format.resolved

schema S XML <"valid">
"#;
    let source = SourceFile::new("test.rosetta", src.to_string());
    let (unit, _) = sigil_syntax::parse(&source);
    let model = sigil_syntax::lower("test.rosetta", &unit.unwrap());
    let resolution = resolve(vec![model]);
    let schema = resolution
        .files
        .iter()
        .flat_map(|f| f.elements.iter())
        .find_map(|e| match e {
            sigil_model::SemanticElement::Schema(s) => Some(s),
            _ => None,
        })
        .unwrap();
    let enum_id = schema.format_resolved.expect("format resolved");
    assert_eq!(
        resolution.qname(sigil_resolve::ElementId(enum_id)),
        "com.rosetta.model.SerializationFormat"
    );
}
