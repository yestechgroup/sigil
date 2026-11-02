//! Deprecation warnings: referencing a `[deprecated]` element emits a
//! warning diagnostic at the reference site while resolution still
//! succeeds. Covers type refs, annotation refs, `extends` parents, report
//! named refs, and transform-annotation schema refs.

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

fn warnings(src: &str) -> Vec<serde_json::Value> {
    resolved(src)["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["severity"] == "warning")
        .cloned()
        .collect()
}

#[test]
fn type_ref_to_deprecated_type_warns() {
    let src = r#"
namespace test.deprecated.type

type OldType: <"superseded">
	[deprecated]
	price number (1..1)

type Holder:
	old OldType (0..1)
"#;
    let ws = warnings(src);
    assert_eq!(ws.len(), 1, "{ws:?}");
    assert_eq!(ws[0]["code"], "W0101");
    assert!(
        ws[0]["message"]
            .as_str()
            .unwrap()
            .contains("'test.deprecated.type.OldType' is deprecated"),
        "{ws:?}"
    );
    // The warning points at the reference site, not the declaration.
    assert_eq!(ws[0]["path"], "type Holder.attributes[0]");
}

#[test]
fn deprecated_schema_ref_via_transform_warns() {
    let src = r#"
namespace test.deprecated.schema

schema OldSchema XML <"superseded">
	[deprecated]

func Ingest:
	[ingest OldSchema]
	output:
		result number (1..1)

	set result:
		0.0
"#;
    let ws = warnings(src);
    assert_eq!(ws.len(), 1, "{ws:?}");
    assert_eq!(ws[0]["code"], "W0101");
    assert!(
        ws[0]["message"].as_str().unwrap().contains("OldSchema"),
        "{ws:?}"
    );
    assert_eq!(ws[0]["path"], "func Ingest.transform[0]");
}

#[test]
fn extends_deprecated_function_warns() {
    let src = r#"
namespace test.deprecated.extends

func Base: <"superseded">
	[deprecated]
	output:
		result number (1..1)

	set result:
		0.0

func Derived extends Base:
	output:
		result number (1..1)

	set result:
		0.0
"#;
    let ws = warnings(src);
    assert_eq!(ws.len(), 1, "{ws:?}");
    assert_eq!(ws[0]["path"], "func Derived");
}

#[test]
fn deprecated_enum_ref_via_type_warns() {
    let src = r#"
namespace test.deprecated.enum

enum OldEnum: <"superseded">
	[deprecated]
	A
	B

type Holder:
	value OldEnum (0..1)
"#;
    let ws = warnings(src);
    assert_eq!(ws.len(), 1, "{ws:?}");
    assert!(
        ws[0]["message"]
            .as_str()
            .unwrap()
            .contains("'test.deprecated.enum.OldEnum' is deprecated"),
        "{ws:?}"
    );
}

#[test]
fn non_deprecated_references_stay_clean() {
    let src = r#"
namespace test.deprecated.clean

type Fine:
	price number (1..1)

type Holder:
	fine Fine (0..1)
"#;
    let json = resolved(src);
    let diags = json["diagnostics"].as_array().unwrap();
    assert!(diags.is_empty(), "{diags:?}");
}

#[test]
fn same_named_annotation_in_other_namespace_does_not_trigger() {
    // The marker is matched by resolved qualified name: a user-defined
    // `deprecated` annotation must not mark anything.
    let src = r#"
namespace test.deprecated.lookalike

annotation deprecated: <"not the builtin">

type T:
	[deprecated]
	x number (1..1)

type Holder:
	t T (0..1)
"#;
    let ws = warnings(src);
    assert!(ws.is_empty(), "{ws:?}");
}

#[test]
fn warnings_do_not_fail_resolution() {
    let src = r#"
namespace test.deprecated.exit

type Old:
	[deprecated]
	x number (1..1)

type Holder:
	old Old (0..1)
"#;
    let json = resolved(src);
    // Only warnings, no errors: the model still resolves fully.
    let errors: Vec<_> = json["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["severity"] == "error")
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    let user_file = json["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["namespace"] == "test.deprecated.exit")
        .unwrap();
    assert_eq!(
        user_file["elements"].as_array().unwrap().len(),
        2,
        "both types present in the resolved model"
    );
}
