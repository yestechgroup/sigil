//! Criterion benchmarks for the toolchain pipeline, mirroring what
//! `sigil model` does: `parse` -> `lower` -> `resolve` -> canonical JSON.
//!
//! Fixtures (under `benches/data/`):
//! - `hero-model/` — the FINOS rune-dsl profiling "hero model"
//!   (`rune-profiling`), vendored verbatim; Apache-2.0 like the builtin
//!   library. Small (~160 lines).
//! - `synthetic/large.rosetta` — a large deterministic generated model
//!   (~7.3k lines: 300 types, 16 enums, 600 rules, 100 funcs, with
//!   cross-references and expressions). This is a synthetic stand-in for a
//!   production-size model until a larger real one is vendored.

use std::hint::black_box;
use std::path::PathBuf;

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use sigil_diag::{Severity, SourceFile};
use sigil_model::ModelFile;
use sigil_syntax::ast::SourceUnit;

fn data(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("benches")
        .join("data")
        .join(rel)
}

fn read_files(rels: &[&str]) -> Vec<SourceFile> {
    rels.iter()
        .map(|rel| {
            let path = data(rel);
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("cannot read {path:?}: {e}"));
            SourceFile::new(*rel, text)
        })
        .collect()
}

fn clean_units(files: &[SourceFile]) -> Vec<(String, SourceUnit)> {
    let mut units = Vec::new();
    for file in files {
        let (unit, diags) = sigil_syntax::parse(file);
        assert!(
            diags.iter().all(|d| d.severity != Severity::Error),
            "bench fixture {} must parse without errors: {diags:?}",
            file.name
        );
        if let Some(unit) = unit {
            units.push((file.name.clone(), unit));
        }
    }
    units
}

/// Parse every fixture and require a clean parse. When `clean_resolve` is
/// set, also require a clean resolution. The vendored hero model uses
/// `[ruleReference]`, an annotation outside the 9.58.1 builtin library, so
/// it legitimately resolves with `E0102`s; the synthetic fixture must be
/// fully clean.
fn clean_models(units: &[(String, SourceUnit)], clean_resolve: bool) -> Vec<ModelFile> {
    let models: Vec<ModelFile> = units
        .iter()
        .map(|(name, unit)| sigil_syntax::lower(name, unit))
        .collect();
    if clean_resolve {
        let resolution = sigil_resolve::resolve(models.clone());
        assert!(
            resolution.diagnostics.is_empty(),
            "bench fixtures must resolve without diagnostics: {:?}",
            resolution.diagnostics
        );
    }
    models
}

fn bench_stage(c: &mut Criterion, group: &str, rels: &[&str], clean_resolve: bool) {
    let files = read_files(rels);
    let units = clean_units(&files);
    let models = clean_models(&units, clean_resolve);
    let sources: Vec<(String, String)> = files
        .iter()
        .map(|f| (f.name.clone(), f.text.clone()))
        .collect();

    let mut g = c.benchmark_group(group);
    g.bench_function("parse", |b| {
        b.iter(|| {
            let units: Vec<_> = black_box(&files)
                .iter()
                .filter_map(|f| sigil_syntax::parse(f).0)
                .collect();
            black_box(units)
        })
    });
    g.bench_function("lower", |b| {
        b.iter_batched(
            || units.clone(),
            |units| {
                let models: Vec<_> = units
                    .iter()
                    .map(|(name, unit)| sigil_syntax::lower(name, unit))
                    .collect();
                black_box(models)
            },
            BatchSize::SmallInput,
        )
    });
    g.bench_function("resolve", |b| {
        b.iter_batched(
            || models.clone(),
            |models| black_box(sigil_resolve::resolve(models)),
            BatchSize::SmallInput,
        )
    });
    g.bench_function("model", |b| {
        b.iter(|| {
            let models: Vec<ModelFile> = black_box(&files)
                .iter()
                .filter_map(|f| {
                    let (unit, _) = sigil_syntax::parse(f);
                    unit.map(|u| sigil_syntax::lower(&f.name, &u))
                })
                .collect();
            let resolution = sigil_resolve::resolve(models);
            black_box(sigil_resolve::canonical_json(
                &resolution,
                black_box(&sources),
            ))
        })
    });
    g.finish();
}

fn pipeline(c: &mut Criterion) {
    bench_stage(
        c,
        "hero_model",
        &["hero-model/reg-model.rosetta", "hero-model/reg.rosetta"],
        false,
    );
    bench_stage(c, "synthetic_large", &["synthetic/large.rosetta"], true);
}

criterion_group!(benches, pipeline);
criterion_main!(benches);
