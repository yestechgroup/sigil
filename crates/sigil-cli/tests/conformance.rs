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
    let mut value = canonical_json(&resolution);
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
