# sigil

A native-Rust toolchain for the [Rune DSL](https://rune.finos.org) — the
language of `.rosetta` files used by the FINOS
[rune-dsl](https://github.com/finos/rune-dsl) project (formerly Rosetta).

Sigil treats the Java implementation as the **behavioural oracle** and
reproduces Rune's semantics on native Rust foundations: a spanned AST from a
[chumsky](https://crates.io/crates/chumsky) parser, a parser-independent
semantic model mirroring Rune's Ecore metamodel, name resolution matching
Xtext scoping rules, and a canonical JSON model dump compared
differentially against the Java implementation.

See [docs/compatibility.md](docs/compatibility.md) for the concept matrix,
milestones and diagnostic codes.

## Status

- **Milestone 1 (model + resolution)**: parses all 31 `.rosetta` files in the
  rune-dsl repository; resolves type/annotation references across files with
  Xtext-faithful scoping; diagnostics with real source spans.
- **Milestone 2 (expressions)**: full expression grammar with a
  parser-independent IR, precedence-faithful parsing, a pretty-printer with
  proptest round-trip verification, and type-level `condition` support.
- **Milestone 2.5 (functions, rules, reports)**: `func` (inputs/output/
  aliases/operations/post-conditions/dispatch), `reporting`/`eligibility
  rule`, `report`, `rule source`, `schema`, `body`/`corpus`/`segment`,
  `metaType` — with function-scoped name resolution mirroring the Java
  implementation. `W0001` is retired; the full language surface parses.
- **Milestone 3 (LSP)**: `sigil-lsp` on `lsp-server` — incremental sync,
  publishDiagnostics, documentSymbol, cross-file definition, hover,
  context-aware completion, references, workspace symbols; VS Code
  extension in `editors/vscode`; see [docs/lsp.md](docs/lsp.md).
- **Differential testing**: 6 oracle fixtures (incl. an 88-condition
  expression corpus) match the Java implementation's resolved EMF model
  exactly.

## Workspace

| Crate | Purpose |
| --- | --- |
| `sigil-diag` | spans, source files, diagnostics |
| `sigil-syntax` | chumsky parser → spanned AST → lowering |
| `sigil-model` | semantic model (Ecore-mirroring IR, parser-free) |
| `sigil-resolve` | symbol tables, scoping, resolution diagnostics |
| `sigil-cli` | the `sigil` binary |

## Usage

```sh
cargo run -p sigil-cli -- parse model.rosetta   # syntax check
cargo run -p sigil-cli -- check model.rosetta   # parse + resolve
cargo run -p sigil-cli -- model  model.rosetta  # canonical model JSON
```

## Tests

```sh
cargo test                      # unit + conformance corpus
UPDATE_EXPECT=1 cargo test -p sigil-cli --test conformance   # regenerate fixtures
```

## Oracle (differential testing)

Requires a JDK (21) and Maven. The dumper loads `.rosetta` files with the
official Java implementation (`com.regnosys.rosetta` 9.58.1 from Maven
Central, builtins included) and dumps a normalized JSON view of the EMF
model; `scripts/oracle_compare.py` diffs it against `sigil model`.

```sh
cargo build -p sigil-cli
python3 scripts/oracle_compare.py tests/oracle/*.rosetta
```

The first run downloads the Maven dependency tree (~600 jars, several
minutes). Known oracle-version caveats are documented in
[docs/compatibility.md](docs/compatibility.md).

## License

Apache-2.0. The embedded builtin `.rosetta` files
(`crates/sigil-resolve/builtin/`) and the vendored grammar
(`docs/reference/`) come from the FINOS rune-dsl project (Apache-2.0).
