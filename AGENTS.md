# AGENTS.md

## What this is

Native-Rust toolchain for the FINOS **Rune DSL** (the language of `.rosetta` files). This is **not** a port built on the Java implementation — all toolchain code is Rust. The Java implementation (`com.regnosys.rosetta` 9.58.1) is used only as the **behavioural oracle**: `sigil model` output must match the Java EMF dump exactly after normalization.

`docs/compatibility.md` is the working specification (concept matrix, Ecore mapping, Xtext scoping rules, diagnostic codes `E0001`/`E01xx`, oracle-version caveats). Read it before changing grammar, model, or resolution — it explains *why* code is shaped the way it is.

## Commands

**CI on GitHub is currently non-functional (as of 2026-09-21): the repo is private and the account's Actions minutes are exhausted — jobs get SIGTERM-killed (exit 143) mid-run. Use local compute instead: run the full gate below locally before every push. Do not rely on or wait for GitHub check runs.** (Workflow file is kept healthy: concurrency cancel, bench job nightly/dispatch/PR-only.)

The CI gate (`.github/workflows/ci.yml` mirrors this), in this order:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings   # warnings are errors
cargo test --workspace                      # CI sets PROPTEST_CASES=256
python3 scripts/oracle_compare.py tests/oracle/model-basic.rosetta   # also sweeps tests/oracle/*.multi/ groups
```

- CLI (binary `sigil`, hand-rolled arg parsing, no clap): `cargo run -p sigil-cli -- parse|check|model <files...>`
- Regenerate conformance fixtures: `UPDATE_EXPECT=1 cargo test -p sigil-cli --test conformance`
- Longer property-test runs: `PROPTEST_CASES=512 cargo test -p sigil-lsp`

`cargo test` does **not** include Java-oracle parity on its own — the gate above runs it explicitly (any fixture invocation also sweeps the `tests/oracle/*.multi/` multi-file groups; see the script docstring):

```sh
cargo build -p sigil-cli                            # script runs target/debug/sigil
python3 scripts/oracle_compare.py tests/oracle/*.rosetta
```

Requires JDK 21 + Maven. The script compiles the dumper itself (`mvn compile exec:java` in `tools/oracle-dumper`). First run downloads ~600 Maven jars (several minutes).

## Architecture

Pipeline: `sigil-diag` (spans, diagnostics) → `sigil-syntax` (chumsky parser → spanned AST → `lower()` to `ModelFile`) → `sigil-model` (parser-independent IR mirroring Rune's Ecore metamodel, incl. `expr::Expr`) → `sigil-resolve` (Xtext-faithful scoping, `resolve()`, `canonical_json()`) → `sigil-cli` / `sigil-lsp`.

- Resolution is whole-workspace: `resolve(models)` takes all files at once because models are multi-file. The LSP re-parses/re-resolves everything on every change by design — don't "optimize" this into per-file caching.
- Builtins (`com.rosetta.model`) are `.rosetta` files compiled into `sigil-resolve` from `crates/sigil-resolve/builtin/` — copied verbatim from the FINOS rune-dsl repo (Apache-2.0). Don't edit except to re-sync upstream; same for the vendored grammar `docs/reference/Rosetta.xtext`.
- LSP: `lsp-server` sync scaffold; all spans are byte offsets internally, converted to LSP positions at the edges; builtin definitions use `builtin:` pseudo-URIs. See `docs/lsp.md`.

## Tests

- Conformance corpus: `tests/conformance/<group>/<case>/*.rosetta` + `expected.json` (canonical resolved-model JSON). Case dirs are auto-discovered; a missing `expected.json` is written on first run (not a failure). `UPDATE_EXPECT=1` rewrites all. The spans test skips itself under `UPDATE_EXPECT` to avoid racing the corpus test.
- Oracle fixtures: `tests/oracle/*.rosetta`. They must stay parseable by the *9.58.1* oracle — newer-main-only syntax (`func extends`, `[ingest ...]`, `as`/`as-key`, dispatch functions, single-letter names, …) crashes or misparses it. Cover such features with sigil-only conformance fixtures instead; the full caveat list is in `docs/compatibility.md`.
- Acceptance criterion for parser/model/resolution changes: canonical JSON matches the Java oracle dump exactly, after the documented normalization in `scripts/oracle_compare.py`. When behaviour changes intentionally, update `expected.json` via `UPDATE_EXPECT=1` and adjust oracle fixtures/normalization together.
