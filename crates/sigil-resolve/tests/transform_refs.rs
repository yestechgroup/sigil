//! Transform-annotation reference resolution: `[ingest X]` / `[projection X]`
//! resolve to a `Schema` element or a `SerializationFormat` enum literal
//! (`SchemaOrFormat` scope, Rosetta.xtext:185); `[enrich]` has no reference.

use sigil_diag::SourceFile;
use sigil_resolve::{canonical_json, resolve};

fn resolved_json(src: &str) -> serde_json::Value {
    let source = SourceFile::new("test.rosetta", src.to_string());
    let (unit, syntax) = sigil_syntax::parse(&source);
    assert!(syntax.is_empty(), "{syntax:?}");
    let model = sigil_syntax::lower("test.rosetta", &unit.unwrap());
    let resolution = resolve(vec![model]);
    canonical_json(&resolution, &[("test.rosetta".into(), src.to_string())])
}

fn transforms(src: &str) -> Vec<serde_json::Value> {
    let json = resolved_json(src);
    json["files"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|f| f["elements"].as_array().unwrap())
        .filter(|e| e["kind"] == "Function")
        .flat_map(|f| f["transform"].as_array().unwrap().iter().cloned())
        .collect()
}

const SCHEMA_REF: &str = r#"
namespace test.resolve.transform

type Trade:
	price number (1..1)

schema FixmlSchema XML <"named">

func Ingest:
	[ingest FixmlSchema]
	inputs:
		trade Trade (1..1)
	output:
		result number (1..1)

	set result:
		trade -> price
"#;

#[test]
fn schema_ref_resolves_to_fqn() {
    let t = transforms(SCHEMA_REF);
    assert_eq!(t.len(), 1);
    assert_eq!(t[0]["kind"], "ingest");
    assert_eq!(t[0]["ref"]["name"], "test.resolve.transform.FixmlSchema");
    assert!(resolved_json(SCHEMA_REF)["diagnostics"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn bare_format_literal_resolves() {
    let src = r#"
namespace test.resolve.bare

func Project:
	[projection JSON]
	output:
		result number (1..1)

	set result:
		0.0
"#;
    let t = transforms(src);
    assert_eq!(
        t[0]["ref"]["name"],
        "com.rosetta.model.SerializationFormat.JSON"
    );
}

#[test]
fn qualified_format_literal_resolves() {
    let src = r#"
namespace test.resolve.qualified

func Project:
	[projection SerializationFormat.RUNE_JSON]
	output:
		result number (1..1)

	set result:
		0.0
"#;
    let t = transforms(src);
    assert_eq!(
        t[0]["ref"]["name"],
        "com.rosetta.model.SerializationFormat.RUNE_JSON"
    );
}

#[test]
fn enrich_without_ref_stays_null() {
    let src = r#"
namespace test.resolve.enrich

func Enrich:
	[enrich]
	output:
		result number (1..1)

	set result:
		0.0
"#;
    let t = transforms(src);
    assert_eq!(t[0]["ref"], serde_json::Value::Null);
}

#[test]
fn unknown_ref_is_diagnosed_and_renders_written_name() {
    let src = r#"
namespace test.resolve.unknown

func Ingest:
	[ingest NoSuchSchema]
	output:
		result number (1..1)

	set result:
		0.0
"#;
    let json = resolved_json(src);
    assert_eq!(transforms(src)[0]["ref"]["name"], "NoSuchSchema");
    let diags = json["diagnostics"].as_array().unwrap();
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0]["code"], "E0101");
    assert_eq!(diags[0]["severity"], "error");
    assert!(
        diags[0]["message"]
            .as_str()
            .unwrap()
            .contains("NoSuchSchema"),
        "{diags:?}"
    );
}

#[test]
fn non_schema_target_is_diagnosed() {
    let src = r#"
namespace test.resolve.wrongkind

type SomeType:
	x number (1..1)

func Ingest:
	[ingest SomeType]
	output:
		result number (1..1)

	set result:
		0.0
"#;
    let json = resolved_json(src);
    let diags = json["diagnostics"].as_array().unwrap();
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0]["code"], "E0106");
    assert!(
        diags[0]["message"]
            .as_str()
            .unwrap()
            .contains("schema or serialization format"),
        "{diags:?}"
    );
}

#[test]
fn unknown_format_value_is_diagnosed() {
    let src = r#"
namespace test.resolve.badformat

func Project:
	[projection FIXML]
	output:
		result number (1..1)

	set result:
		0.0
"#;
    let json = resolved_json(src);
    let diags = json["diagnostics"].as_array().unwrap();
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0]["code"], "E0101");
    assert!(
        diags[0]["message"].as_str().unwrap().contains("FIXML"),
        "{diags:?}"
    );
}

#[test]
fn unknown_qualified_value_is_diagnosed() {
    let src = r#"
namespace test.resolve.badqualified

func Project:
	[projection SerializationFormat.NONSENSE]
	output:
		result number (1..1)

	set result:
		0.0
"#;
    let json = resolved_json(src);
    let diags = json["diagnostics"].as_array().unwrap();
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0]["code"], "E0107");
    assert!(
        diags[0]["message"].as_str().unwrap().contains("NONSENSE"),
        "{diags:?}"
    );
}

#[test]
fn qualified_head_not_an_enum_is_diagnosed() {
    let src = r#"
namespace test.resolve.badhead

type SomeType:
	x number (1..1)

func Project:
	[projection SomeType.X]
	output:
		result number (1..1)

	set result:
		0.0
"#;
    let json = resolved_json(src);
    let diags = json["diagnostics"].as_array().unwrap();
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0]["code"], "E0106");
    assert!(
        diags[0]["message"]
            .as_str()
            .unwrap()
            .contains("serialization format"),
        "{diags:?}"
    );
}

#[test]
fn fix_format_literal_resolves_after_builtin_sync() {
    let src = r#"
namespace test.resolve.fix

func Ingest:
	[ingest FIX]
	output:
		result number (1..1)

	set result:
		0.0
"#;
    let t = transforms(src);
    assert_eq!(
        t[0]["ref"]["name"],
        "com.rosetta.model.SerializationFormat.FIX"
    );
}
