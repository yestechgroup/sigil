//! Criterion benchmark for the language server's full-workspace reanalysis
//! path: by design (docs/lsp.md) every didChange re-parses and re-resolves
//! every document in the workspace (`World::reanalyze` ->
//! `Analysis::compute`, built-in library included). One iteration is one
//! such didChange-triggered reanalysis cycle; the text edit itself is
//! trivial and not measured.
//!
//! The workspace is the synthetic multi-file fixture under
//! `crates/sigil-cli/benches/data/synthetic/ws` (12 files with cross-file
//! references; see the pipeline bench docs for provenance).

use std::collections::BTreeMap;
use std::path::PathBuf;

use criterion::{criterion_group, criterion_main, Criterion};
use sigil_lsp::{Analysis, Document};

fn workspace_docs(count: usize) -> BTreeMap<String, Document> {
    let dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../sigil-cli/benches/data/synthetic/ws");
    let paths = sigil_syntax::project::discover_rosetta_files(&[dir]);
    let mut docs = BTreeMap::new();
    for path in paths.into_iter().take(count) {
        let uri = sigil_syntax::project::path_to_uri(&path)
            .unwrap_or_else(|| panic!("no URI for {path:?}"));
        let text =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {path:?}: {e}"));
        docs.insert(uri, Document::new(text, 0));
    }
    docs
}

fn reanalysis(c: &mut Criterion) {
    let mut g = c.benchmark_group("lsp_did_change_reanalysis");
    for count in [2, 12] {
        let docs = workspace_docs(count);
        assert_eq!(docs.len(), count, "workspace fixture files missing");
        g.bench_function(format!("{count}_files"), |b| {
            b.iter(|| Analysis::compute(black_box_docs(&docs)))
        });
    }
    g.finish();
}

fn black_box_docs(docs: &BTreeMap<String, Document>) -> &BTreeMap<String, Document> {
    std::hint::black_box(docs)
}

criterion_group!(benches, reanalysis);
criterion_main!(benches);
