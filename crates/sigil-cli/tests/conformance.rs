//! Conformance harness: each directory under `tests/conformance/` holds one
//! or more `.rosetta` files plus an `expected.json` containing the canonical
//! resolved-model JSON. Set `UPDATE_EXPECT=1` to regenerate expectations.

use std::path::{Path, PathBuf};

use sigil_diag::{Diagnostic, SourceFile};
use sigil_model::ModelFile;
use sigil_resolve::{canonical_json, resolve};

fn conformance_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/conformance")
        .canonicalize()
        .expect("tests/conformance must exist")
}

fn run_case(dir: &Path) -> Result<serde_json::Value, String> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|e| e == "rosetta").unwrap_or(false))
        .collect();
    paths.sort();

    let mut units = Vec::new();
    let mut sources: Vec<SourceFile> = Vec::new();
    let mut syntax_diagnostics: Vec<Diagnostic> = Vec::new();
    for path in paths {
        let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let file = SourceFile::new(name, text);
        let (unit, diags) = sigil_syntax::parse(&file);
        syntax_diagnostics.extend(diags);
        if let Some(unit) = unit {
            units.push(sigil_syntax::lower(&file.name, &unit));
        }
        sources.push(file);
    }

    let models: Vec<ModelFile> = units;
    let resolution = resolve(models);
    let texts: Vec<(String, String)> = sources
        .iter()
        .map(|f| (f.name.clone(), f.text.clone()))
        .collect();
    let mut value = canonical_json(&resolution, &texts);
    // Syntax diagnostics are rendered separately by the syntax test suite;
    // conformance cases focus on the semantic model.
    value["syntaxDiagnostics"] = serde_json::Value::Array(
        syntax_diagnostics
            .iter()
            .map(|d| {
                serde_json::json!({
                    "severity": d.severity.as_str(),
                    "code": d.code,
                    "message": d.message,
                })
            })
            .collect(),
    );
    Ok(value)
}

#[test]
fn conformance_corpus_matches_expected() {
    let root = conformance_root();
    let update = std::env::var("UPDATE_EXPECT").is_ok_and(|v| v == "1");
    let mut checked = 0usize;
    let mut failures = Vec::new();

    let mut case_dirs: Vec<PathBuf> = std::fs::read_dir(&root)
        .expect("conformance root")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .flat_map(|case_root| {
            std::fs::read_dir(&case_root)
                .expect("case group")
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.is_dir())
        })
        .collect();
    case_dirs.sort();

    for case in case_dirs {
        let actual = match run_case(&case) {
            Ok(v) => v,
            Err(e) => {
                failures.push(format!("{}: {e}", case.display()));
                continue;
            }
        };
        let expected_path = case.join("expected.json");
        if update || !expected_path.exists() {
            std::fs::write(
                &expected_path,
                serde_json::to_string_pretty(&actual).unwrap() + "\n",
            )
            .expect("write expected.json");
            println!("updated {}", expected_path.display());
        } else {
            let expected_text = std::fs::read_to_string(&expected_path).unwrap();
            let expected: serde_json::Value = serde_json::from_str(&expected_text).unwrap();
            if expected != actual {
                failures.push(format!(
                    "{}: model mismatch\n--- expected ---\n{}\n--- actual ---\n{}",
                    case.display(),
                    serde_json::to_string_pretty(&expected).unwrap(),
                    serde_json::to_string_pretty(&actual).unwrap()
                ));
            }
        }
        checked += 1;
    }

    assert!(checked > 0, "no conformance cases found");
    if !failures.is_empty() {
        panic!(
            "{} of {} conformance cases failed:\n{}",
            failures.len(),
            checked,
            failures.join("\n\n")
        );
    }
}

/// The E0101 diagnostic in the resolution-errors expectation must point at
/// the `Unknown` reference in the source: this guards that spans survive the
/// lowering → resolution → canonical-JSON round-trip with the right
/// coordinates.
#[test]
fn error_case_expected_spans_point_at_source() {
    // The corpus test regenerates expected.json concurrently under
    // UPDATE_EXPECT; don't race against it.
    if std::env::var("UPDATE_EXPECT").is_ok_and(|v| v == "1") {
        return;
    }
    let case = conformance_root().join("resolution/errors");
    let text = std::fs::read_to_string(case.join("case.rosetta")).expect("case.rosetta");
    let expected: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(case.join("expected.json")).expect("expected.json"),
    )
    .unwrap();

    let source = sigil_diag::SourceFile::new("case.rosetta", text.clone());
    let offset = text
        .find("Unknown")
        .expect("Unknown reference in case.rosetta");
    let (line, column) = source.line_col(offset);

    let diag = expected["diagnostics"]
        .as_array()
        .and_then(|ds| ds.iter().find(|d| d["code"] == "E0101"))
        .expect("E0101 diagnostic in expected.json");
    assert_eq!(diag["file"], "case.rosetta");
    assert_eq!(diag["span"]["start"]["line"], line);
    assert_eq!(diag["span"]["start"]["column"], column);
    assert_eq!(diag["location"], format!("case.rosetta:{line}:{column}"));
}
