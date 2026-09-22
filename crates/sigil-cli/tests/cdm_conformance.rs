//! Golden conformance over the FINOS **Common Domain Model** (CDM): the
//! full published `.rosetta` corpus (145 files), pinned by SHA, as the
//! "does sigil digest the real world?" companion to the small curated
//! `tests/conformance/` corpus. See `docs/cdm-conformance.md`.
//!
//! The corpus is **not** vendored into this repo (license size) and is
//! **not** expected to be diagnostic-clean: it references the external
//! `fpml.*` model and contains genuine upstream overloads. Instead this
//! test asserts a committed golden snapshot of sigil's parse/resolve
//! behaviour over the corpus, regenerated deliberately via
//! `UPDATE_CDM_SNAPSHOT=1`.
//!
//! Skips (prints a note, passes) when the corpus is absent; fetch it with
//! `scripts/fetch-cdm.sh master`. When the corpus *is* present the analysis
//! runs twice and both runs must agree (determinism guard).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use sigil_resolve::{resolve, ElementKind};

/// The pinned CDM revision (see `scripts/fetch-cdm.sh`); the snapshot must
/// always record exactly this SHA.
const MASTER_SHA: &str = "eb0eea955ef8409f034e1a9c28714d00a511a61a";

fn snapshot_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/cdm/master-snapshot.json")
}

/// Locate the fetched CDM corpus; `None` when it has not been fetched.
/// Mirrors the default/env logic of `scripts/fetch-cdm.sh` (do not add a
/// dependency on the shell script).
fn corpus_dir() -> Option<PathBuf> {
    let cache_root = match std::env::var("SIGIL_CDM_CACHE") {
        Ok(dir) => PathBuf::from(dir),
        Err(_) => Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/cdm-corpus"),
    };
    let dir = cache_root
        .join("master")
        .join("rosetta-source/src/main/rosetta");
    dir.is_dir().then_some(dir)
}

/// Last path segment of a `file://` URI — the corpus is a flat directory,
/// so basenames are unique and keep the snapshot machine-independent.
fn basename(uri: &str) -> &str {
    uri.rsplit('/').next().unwrap_or(uri)
}

fn kind_name(kind: ElementKind) -> &'static str {
    match kind {
        ElementKind::Data => "data",
        ElementKind::Enumeration => "enumeration",
        ElementKind::Annotation => "annotation",
        ElementKind::TypeAlias => "typeAlias",
        ElementKind::BasicType => "basicType",
        ElementKind::RecordType => "recordType",
        ElementKind::LibraryFunction => "libraryFunction",
        ElementKind::Function => "function",
        ElementKind::Rule => "rule",
        ElementKind::Report => "report",
        ElementKind::ExternalRuleSource => "externalRuleSource",
        ElementKind::Schema => "schema",
        ElementKind::Body => "body",
        ElementKind::Corpus => "corpus",
        ElementKind::Segment => "segment",
        ElementKind::MetaType => "metaType",
    }
}

fn counts_by_code<I: IntoIterator<Item = String>>(codes: I) -> BTreeMap<String, usize> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for code in codes {
        *counts.entry(code).or_default() += 1;
    }
    counts
}

/// Parse + resolve the whole corpus and summarize it into the snapshot's
/// JSON shape. Deterministic by construction: file discovery is sorted,
/// every map is a `BTreeMap`.
fn analyse(corpus: &Path) -> serde_json::Value {
    let paths = sigil_syntax::project::discover_rosetta_files(&[corpus.to_path_buf()]);

    let mut lines = 0usize;
    let mut file_names = BTreeSet::new();
    for path in &paths {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        lines += text.lines().count();
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            file_names.insert(name.to_string());
        }
    }

    let (models, syntax_diagnostics) =
        sigil_syntax::project::load_user_files(&[corpus.to_path_buf()]);

    // Per-file parse status: `parse` yields a unit iff the file parsed, and
    // a file that failed to parse gets E0001 diagnostics, so the first
    // diagnostic code per unparsed file names the failure.
    let parsed: BTreeSet<String> = models.iter().map(|m| m.name.clone()).collect();
    let mut parse_per_file: BTreeMap<String, String> = BTreeMap::new();
    for path in &paths {
        let uri = sigil_syntax::project::path_to_uri(path).expect("utf-8 corpus path");
        let name = basename(&uri).to_string();
        let status = if parsed.contains(&uri) {
            "ok".to_string()
        } else {
            syntax_diagnostics
                .iter()
                .find(|d| d.file == uri)
                .map(|d| d.code.to_string())
                .unwrap_or_else(|| "no-model".to_string())
        };
        parse_per_file.insert(name, status);
    }
    let ok = parse_per_file
        .values()
        .filter(|s| s.as_str() == "ok")
        .count();
    let failed = parse_per_file.len() - ok;
    let syntax_by_code = counts_by_code(syntax_diagnostics.iter().map(|d| d.code.to_string()));

    let resolution = resolve(models);
    let mut resolution_per_file: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    for d in &resolution.diagnostics {
        let name = basename(&d.file).to_string();
        // Corpus files are keyed by basename; the embedded builtins are
        // never expected to contribute diagnostics, but if they ever do,
        // keep them visible under an explicit key instead of colliding
        // with (or silently dropping from) the corpus view.
        let key = if file_names.contains(&name) {
            name
        } else {
            format!("(builtin)/{name}")
        };
        *resolution_per_file
            .entry(key)
            .or_default()
            .entry(d.code.to_string())
            .or_default() += 1;
    }
    let resolution_total =
        counts_by_code(resolution.diagnostics.iter().map(|d| d.code.to_string()));

    // Element counts over the corpus files only: `elements_of_kind` includes
    // the embedded builtins, which are always exactly the first two files.
    let builtin_elements = resolution.files[0].elements.len() + resolution.files[1].elements.len();
    let mut elements = BTreeMap::new();
    for &kind in ElementKind::ALL {
        let count = resolution
            .elements_of_kind(kind)
            .iter()
            .filter(|(id, _)| id.0 >= builtin_elements)
            .count();
        elements.insert(kind_name(kind), count);
    }

    serde_json::json!({
        "masterSha": MASTER_SHA,
        "corpus": {
            "files": paths.len(),
            "lines": lines,
        },
        "parse": {
            "ok": ok,
            "failed": failed,
            "diagnosticsByCode": syntax_by_code,
            "perFile": parse_per_file,
        },
        "resolution": {
            "diagnosticsByCode": resolution_total,
            "perFile": resolution_per_file,
        },
        "elements": elements,
    })
}

#[test]
fn cdm_corpus_matches_golden_snapshot() {
    let Some(corpus) = corpus_dir() else {
        println!(
            "skipping cdm_conformance: CDM corpus not fetched — run scripts/fetch-cdm.sh master"
        );
        return;
    };

    let update = std::env::var("UPDATE_CDM_SNAPSHOT").is_ok_and(|v| v == "1");
    let actual = analyse(&corpus);
    if update {
        let path = snapshot_path();
        std::fs::create_dir_all(path.parent().unwrap()).expect("create tests/cdm");
        std::fs::write(&path, serde_json::to_string_pretty(&actual).unwrap() + "\n")
            .expect("write master-snapshot.json");
        println!("updated {}", path.display());
        return;
    }

    // Determinism guard: a second run over the same corpus must produce an
    // identical summary.
    let again = analyse(&corpus);
    assert_eq!(
        actual, again,
        "CDM analysis is not deterministic between two runs"
    );

    let expected: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(snapshot_path())
            .expect("tests/cdm/master-snapshot.json — regenerate with UPDATE_CDM_SNAPSHOT=1"),
    )
    .expect("master-snapshot.json must be valid JSON");

    assert_eq!(
        expected["masterSha"], actual["masterSha"],
        "snapshot is pinned to a different CDM revision than the fetched corpus — \
         refetch with scripts/fetch-cdm.sh master --force (or re-pin deliberately, \
         see docs/cdm-conformance.md)"
    );
    assert_eq!(
        expected,
        actual,
        "CDM golden snapshot mismatch\n--- expected ---\n{}\n--- actual ---\n{}",
        serde_json::to_string_pretty(&expected).unwrap(),
        serde_json::to_string_pretty(&actual).unwrap()
    );
}
