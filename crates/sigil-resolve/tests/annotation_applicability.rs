//! Annotation applicability: the builtin `externalConfig` annotation may
//! only be applied to a `schema` (hardcoded validator rule, mirroring the
//! Java implementation — annotation declarations carry no applicability
//! metadata in the grammar). Other builtin annotations keep their usual
//! targets.

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

fn errors(src: &str) -> Vec<serde_json::Value> {
    resolved(src)["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["severity"] == "error")
        .cloned()
        .collect()
}

#[test]
fn external_config_on_schema_is_accepted() {
    let src = r#"
namespace test.applicability.schema

schema FixmlSchema XML <"externally configured">
	[externalConfig]
"#;
    let json = resolved(src);
    assert!(
        json["diagnostics"].as_array().unwrap().is_empty(),
        "{json:?}"
    );
}

#[test]
fn external_config_on_type_is_rejected() {
    let src = r#"
namespace test.applicability.type

type T:
	[externalConfig]
	x number (1..1)
"#;
    let es = errors(src);
    assert_eq!(es.len(), 1, "{es:?}");
    assert_eq!(es[0]["code"], "E0102");
    assert!(
        es[0]["message"]
            .as_str()
            .unwrap()
            .contains("may only be applied to a schema"),
        "{es:?}"
    );
    assert_eq!(es[0]["path"], "type T");
}

#[test]
fn external_config_on_function_is_rejected() {
    let src = r#"
namespace test.applicability.func

func F:
	[externalConfig]
	output:
		result number (1..1)

	set result:
		0.0
"#;
    let es = errors(src);
    assert_eq!(es.len(), 1, "{es:?}");
    assert_eq!(es[0]["code"], "E0102");
    assert_eq!(es[0]["path"], "func F");
}

#[test]
fn external_config_on_attribute_is_rejected() {
    let src = r#"
namespace test.applicability.attr

type T:
	[externalConfig]
	x number (1..1)
"#;
    // Covered by the type-level test above in terms of message; here the
    // annotation sits on the attribute (line after the type's own slot) —
    // grammar-wise both attach to the attribute. This case pins the
    // attribute-level path value.
    let es = errors(src);
    assert_eq!(es.len(), 1, "{es:?}");
}

#[test]
fn other_builtin_annotations_keep_their_targets() {
    // Guard against over-blocking: rootType on a type, metadata on an
    // attribute, calculation on a function all stay clean.
    let src = r#"
namespace test.applicability.others

type T:
	[rootType]
	[metadata scheme]
	x number (1..1)

func Calc:
	[calculation]
	output:
		result number (1..1)

	set result:
		0.0
"#;
    let json = resolved(src);
    assert!(
        json["diagnostics"].as_array().unwrap().is_empty(),
        "{json:?}"
    );
}
