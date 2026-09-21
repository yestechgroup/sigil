# API stability for rev-pinned embedders

Sigil is consumed as a library by downstream repos that pin the workspace by
**git rev** (mirroring how `rexlang` pins are done). This document names the
surfaces that make up the compatibility boundary: what a rev-pinned embedder
may rely on, what is explicitly *not* stable, and what a rev bump means.

The guiding principle is the same one stated in
[docs/compatibility.md](compatibility.md): sigil's semantic model
(`sigil-model`) is the compatibility boundary — not the parser, not the
AST, not the LSP internals.

## The boundary at a glance

| Surface | Entry point | Status |
| --- | --- | --- |
| Semantic model types | `sigil-model` (`ModelFile`, `SemanticElement`, `expr::Expr`, …) | **stable** (the boundary) |
| Canonical JSON artifact | `sigil_resolve::canonical_json`, format `sigil-model/1` | **stable** (shape contract below) |
| Resolution | `sigil_resolve::resolve`, `Resolution`, `ElementId`, `ElementKind` | **stable** (semantics below) |
| Multi-file ingestion | `sigil_syntax::project::{discover_rosetta_files, load_user_files}` | **stable** (walk rules below) |
| Expression traversal | `sigil_model::expr::{ExprVisitor, walk, ExprFamily, Expr::family}` | **stable** (protocol below) |
| Parser AST (`sigil_syntax::ast`), lowering internals | — | *not* stable |
| LSP internals, CLI arg parsing, bench data, conformance fixture JSONs | — | *not* stable (fixtures are test artifacts, not API) |

Everything marked **stable** is documented against the current source; the
symbol names and file locations below were verified against the tree this
document ships in.

## 1. `sigil-model`: the data boundary

The crate docs say it directly (`crates/sigil-model/src/lib.rs:1-7`): the
model mirrors the Rune Ecore metamodel (`com.regnosys.rosetta`,
`...rosetta.simple`) rather than the surface grammar, knows nothing about
chumsky or the parser, and "this crate is the compatibility boundary for the
rest of the toolchain". `sigil-syntax` ASTs are *lowered* into these types;
`sigil-resolve` fills in the `resolved` fields. An embedder can build
`ModelFile`s itself and call `sigil_resolve::resolve` without touching the
parser at all.

Note for matchers: `SemanticElement` and `expr::Expr` are exhaustive enums
with no wildcard-friendly escape hatch (by design). Adding a variant is a
breaking change for any embedder that matches on them.

### Serialized shape contract

The serde attributes on the model types *are* the wire contract:

- **Renames** (Rust field name ≠ JSON key): `forPath` (`DocReference.for_path`),
  `type` (`Attribute.type_ref`, `TypeAlias.type_ref`, `TypeParameter.type_ref`,
  `RecordFeature.type_ref`, `LibraryParameter.type_ref`, `MetaType.type_ref`),
  `returnType` (`LibraryFunction.return_type`), `value`
  (`TypeCallArgument.argument_value`, `AnnotationQualifier.qualifier_value`),
  `importedNamespace` / `namespaceAlias` (`ImportEntry`), and
  `CardinalityMax::Unbounded` serializes as the string `"*"`.
- **Tagged enums**: `ArgumentValue` and `QualifierValue` use
  `#[serde(tag = "kind", content = "value")]`; `SemanticElement` uses
  `#[serde(tag = "kind")]` with the variant name as the tag
  (`"Data"`, `"Enumeration"`, …).
- **`skip_serializing`**: every source `Span` and every unresolved-id field
  (`TypeRef.resolved`, `AnnotationRef.annotation_resolved`, `RuleReference.resolved`,
  `PathSegment.resolved`, …) is `#[serde(skip_serializing)]` — positions and
  raw ids never appear in serialized output. The canonical JSON shape must
  stay stable; spans are carried in memory only and rendered (for
  diagnostics) by `canonical_json`.
- **`skip_serializing_if`**: `DocReference.for_path` is
  `#[serde(rename = "forPath", skip_serializing_if = "Option::is_none")]`
  (`crates/sigil-model/src/lib.rs:135`). The key is **absent** (not `null`)
  for plain `[docReference ...]` uses. Consumers must treat a missing
  `forPath` as "no attribute-anchored path", and must expect the key to
  appear at all — it is a sigil-only extension (see §6).
- `ReportTiming` has a custom `Serialize` impl that emits the source-spelled
  strings `"real-time"`, `"T+<n>"`, `"ASATP"`.

### The canonical JSON artifact (`sigil-model/1`)

`sigil model <files...>` and `sigil_resolve::canonical_json` emit a single
JSON object with `"format": "sigil-model/1"` (constant defined at
`crates/sigil-resolve/src/lib.rs:1722`; the Java oracle dumper emits the
same shape with the value `"sigil-model/1 (oracle)"`). Top-level keys:
`format`, `files`, `diagnostics`.

Shape rules an embedder can rely on:

- Element objects are hand-rendered by `element_json` /
  `type_json` / `annotations_json` / `rule_references_json`
  (`crates/sigil-resolve/src/lib.rs:1354-1651`) — not by the serde derives —
  so the *element* JSON contract is owned by `sigil-resolve`. The serde
  derives govern the pieces embedded verbatim (`docReferences`, `labels`,
  `imports`, `configurations`), which is why the `forPath` rename above
  shows up in canonical JSON too.
- Resolved references render as **fully-qualified name strings**
  (`"resolved": "com.rosetta.model.number"`); unresolved references render
  `"resolved": null`.
- Cardinalities render as constraint strings (`"(1..1)"`, `"(0..*)"`, as
  produced by `Cardinality::to_constraint_string`).
- `files` **includes the two builtin library files** (namespace
  `com.rosetta.model`); filter them out by namespace if you only want user
  models — `scripts/oracle_compare.py` does exactly this in `normalize()`.
- Diagnostics carry `severity`, `code` (`E0001`/`E01xx`/`W0001`, see
  [compatibility.md](compatibility.md)), `message`, `file`, `path`, and a
  `span` with 1-based `line`/`column` pairs plus a `file:line:col`
  `location`. Columns count plain characters (Unicode scalar values) —
  **not** LSP UTF-16 code units and not bytes. `"span": null` when no span
  or no source text is available.

The `sigil-model/1` format constant is the version marker: a change that
breaks the shape a consumer parses must bump it.

## 2. `sigil_resolve::resolve()` semantics

```rust
pub fn resolve(user_files: Vec<ModelFile>) -> Resolution
```

- **Builtins are auto-prepended.** `resolve` calls `builtin_files()`, which
  parses the two `include_str!`-embedded `.rosetta` files
  (`crates/sigil-resolve/builtin/basictypes.rosetta` and
  `annotations.rosetta`, mirroring `RosettaBuiltinsService`) and prepends
  them. The builtin files therefore sit at **file indices 0 and 1** of
  `Resolution.files`, and their elements occupy the first flat element ids
  (all ids before the element count of those two files). Pass user files
  only — the doc comment on `resolve` is explicit that the input "must NOT
  include builtins". `Resolution::user_files()` returns `&files[2..]`.
- **Pass ALL user files in ONE call.** Scoping is per-file but
  workspace-relative (local elements → explicit imports in declaration
  order → own namespace → implicit `com.rosetta.model` wildcard → FQN; see
  the crate docs and [compatibility.md](compatibility.md)). Two separate
  `resolve` calls are two unrelated workspaces: same-namespace and
  cross-file imports only resolve within one call.
- **Deterministic order.** Flat `ElementId`s (a `pub struct ElementId(pub
  usize)`) are assigned in file submission order, then declaration order
  within the file; `element_names` / `element_spans` are aligned with that
  flat order. Combine `sigil_syntax::project::load_user_files` (§3) or sort
  your own file list to keep the order stable.
- **Whole-workspace resolution is by design, not an omission.** Models are
  multi-file — a change in one file can invalidate any other file's
  resolution — so the language server re-parses and re-resolves the entire
  workspace on every change (`docs/lsp.md`, "Analysis"). Do not "optimize"
  an embedder into per-file caching; the API is whole-workspace on purpose.
- `Resolution` exposes: `files`, `element_names` (FQN per flat id, falling
  back to the written name for unresolvable cases such as anonymous
  reports), `element_spans`, and `diagnostics`
  (`ResolutionDiagnostic { severity, code, message, file, path, span }`;
  the byte `span` is not serialized).

## 3. Multi-file ingestion: `sigil_syntax::project`

`sigil_syntax::project` (module docs + code,
`crates/sigil-syntax/src/project.rs`) exists so embedders don't reimplement
file-system traversal. The rules mirror the language server's workspace
scan, which is the behavioural source of truth:

`discover_rosetta_files(roots: &[PathBuf]) -> Vec<PathBuf>`:

- only file names ending in `.rosetta` are collected (a root that is a
  regular file is collected as-is under the same rule);
- entries whose file name starts with `.` (hidden/dot directories) or whose
  file name is exactly `target` are skipped **with their whole subtree**;
- recursion is depth-capped: the root counts as depth 0 and traversal stops
  once depth exceeds `MAX_DEPTH` (16) — this also bounds symlink cycles;
- unreadable directories are skipped silently;
- the result is **sorted by path and deduplicated**, independent of
  directory iteration order (entries are additionally sorted per directory
  during the walk).

`load_user_files(paths: &[PathBuf]) -> (Vec<ModelFile>, Vec<Diagnostic>)`:

- accepts a mixed list of files and directories (directories are expanded
  with the rules above); the combined list is sorted + deduplicated, so
  models and diagnostics come back in a deterministic order regardless of
  input order;
- each parsed file's `SourceFile`/`ModelFile` name is its `file://` URI
  (same convention as the LSP; **no percent-encoding** — paths with spaces
  or non-ASCII will not round-trip, a documented limitation);
- unreadable files are skipped;
- returns the **syntax** diagnostics of all parsed files; resolution
  diagnostics come from the subsequent `resolve` call.

## 4. Recent embedder APIs (stable-for-pinning)

### `ElementKind` and the `Resolution` query methods

`crates/sigil-resolve/src/lib.rs`:

- `ElementKind` — one variant per `SemanticElement` variant (one per EClass
  that appears as a model root). `ElementKind::ALL` lists them in
  `SemanticElement` variant order; `ElementKind::of(&SemanticElement)`
  classifies.
- `Resolution::kind_of(ElementId) -> ElementKind`.
- `Resolution::elements_of_kind(kind) -> Vec<(ElementId, &SemanticElement)>`
  — ordered by flat id (declaration order). **Includes builtins**: the
  builtin prefix is always exactly two files (§2), so exclude them via the
  `user_files()` bounds / the builtin element count when you only want user
  elements.
- `Resolution::find_by_name(name) -> Option<(ElementId, &SemanticElement)>`
  — exact FQN match first; otherwise the *first* (flat-id order) element
  whose simple name matches. Anonymous elements (reports) are never returned
  by the simple-name fallback.
- `Resolution::qname(ElementId)` and `Resolution::element(ElementId)` round
  out the read API; ids are stable within one `Resolution` value.

### Expression traversal: `ExprVisitor`, `walk`, `Expr::family`

Defined in `crates/sigil-model/src/expr_visit.rs` and re-exported at
`sigil_model::expr::{ExprVisitor, walk, ExprFamily}` so consumers have a
single import site (`crates/sigil-model/src/expr.rs:16-19`):

- `walk(expr, visitor)` is a depth-first, **pre-order**, read-only
  traversal: per node it calls `ExprVisitor::visit_expr`, then exactly one
  family hook chosen by `Expr::family`, then descends into children in
  source order.
- All trait methods default to no-ops; hooks receive the whole node, so
  implementors re-`match` for payloads. Override only what you need.
- `ExprFamily` is the coarse 13-variant taxonomy (`Literal`, `Path`,
  `Arithmetic`, `Logical`, `Comparison`, `Quantifier`, `ListOp`,
  `FunctionOp`, `Cast`, `ControlFlow`, `Meta`, `Choice`, `Constructor`).
  `ExprFamily::as_str()` returns stable kebab-case names (`"list-op"`,
  `"function-op"`, …) suitable for logs/serialization.
- The walker is lossless by construction: the family dispatch and child
  recursion are exhaustive `match`es with no wildcard arm, so a new `Expr`
  variant fails to compile until it is classified and traversed. Pinners
  get the same compile-time guarantee for their own family matches.

## 5. Rev-pinning expectations

- **Pin by rev (or tag).** There is no release channel; a rev is the unit
  of upgrade.
- **Rev bumps should be deliberate.** Any rev bump that touches §1–§4
  (model shapes, the canonical JSON contract or its format constant,
  `resolve` semantics, the `project` walk rules, or the visitor protocol)
  is *breaking* for embedders and must be called out.
- **Where bumps are documented:** there is currently **no `CHANGELOG.md`**
  in this repo and no git tags yet (verified: `git tag` is empty), so rev
  bumps are documented via the commit history / release notes only. This is
  a gap: the recommendation is to start a `CHANGELOG.md` with one entry per
  boundary-affecting change, so `codegraph`-style embedders can diff pin
  bumps against a list instead of against `git log`. Until that exists,
  treat every rev bump as potentially breaking and re-run your own corpus
  through `canonical_json` before moving the pin.
- The conformance corpus (`tests/conformance/**/expected.json`, regenerated
  with `UPDATE_EXPECT=1`) is the best available "what changed in the model
  output" record in the meantime — it is a test artifact, but its diffs
  track the `sigil-model/1` shape.

## 6. Known limitations (read before relying on resolution output)

These are documented divergences, not accidents; see
[compatibility.md](compatibility.md) for the full matrix.

- **Expression `->` segments are parsed but not type-checked.** Only the
  head symbol of a `SymbolReference` resolves; `->`/`->>` feature-call
  chains inside expressions are kept as written
  (`resolve_expr_heads` doc comment,
  `crates/sigil-resolve/src/lib.rs:897-904`; compatibility.md
  "Expression symbol resolution" known limitations). Similarly unresolved
  by design: bare heads that only resolve via the expected type, and
  `ChoiceOperation` attributes / `switch` reference guards / `to-enum` /
  `as` targets. Operation *assign paths* *are* checked, except paths rooted
  at an alias (alias types are not inferred).
- **Oracle 9.58.1 drift.** The Java oracle is the *published* 9.58.1
  artifact; current-main metamodel features have no counterpart there and
  are normalized out of comparisons by `scripts/oracle_compare.py`:
  annotations on type aliases, `func extends` (`superFunction`), transform
  annotations (`transform`), and more — see "Oracle-version caveats" in
  compatibility.md. Fixtures for such features are sigil-only conformance
  cases.
- **`docReference for <path>` / `forPath`.** The attribute-anchored form
  exists in the reference grammar (main) but is rejected outright by
  9.58.1. Sigil parses it and carries the path steps in
  `DocReference.for_path` (JSON `forPath`, §1); it is covered by the
  sigil-only `conformance/model/doc-ref-for-path` fixture, and no
  `tests/oracle/` fixture may use it.
- **Multi-file scoping divergences** found by the `tests/oracle/*.multi/`
  fixtures (compared as one workspace on both sides, files sorted — see the
  `scripts/oracle_compare.py` module docstring):
  - *alias-as-prefix-only*: in 9.58.1 an import alias works only as a
    namespace prefix (`dep.Product`); a bare name reachable only through
    the aliased wildcard does not resolve there (comment in
    `tests/oracle/import-alias.multi/main.rosetta`).
  - *ambiguity*: when two imports could supply the same simple name, 9.58.1
    leaves the reference unresolved (`null`), while sigil resolves
    first-import-wins; that behaviour is covered by Rust-only conformance
    fixtures instead (commit `5510879`).
- `file://` URIs are not percent-encoded (§3) — fine for byte-identical
  round-trips, lossy for exotic paths.
