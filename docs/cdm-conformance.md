# CDM conformance (golden corpus)

The Java-oracle comparison (`tests/oracle/` + `scripts/oracle_compare.py`)
proves *semantic parity* on small fixtures. The CDM suite answers a
different question: **can sigil digest the full published FINOS Common
Domain Model?** — ~44k lines of real-world Rune written by other people,
with heavy dispatch-overloading, ingest functions, regulatory
bodies/corpora/segments, and references to the external `fpml.*` model.

It is a **sigil-only golden suite**: the corpus is parsed and resolved
whole-workspace, the result is summarized, and the summary is asserted
against a committed snapshot (`tests/cdm/master-snapshot.json`). The corpus
is **not** diagnostic-clean and **not** self-contained — diagnostics are
expected, and the snapshot pins the current landscape so that accidental
changes to parse/resolve behaviour cannot slip through.

## Pins

| Pin | Value | Used for |
| --- | --- | --- |
| `MASTER_SHA` | `eb0eea955ef8409f034e1a9c28714d00a511a61a` | Phase 1 golden snapshot (this doc) |
| `LEGACY_TAG` | `6.7.0` | Phase 2 reverse-drift study (older published CDM) |

`MASTER_SHA` is defined in `scripts/fetch-cdm.sh` **and** mirrored as
`MASTER_SHA` in `crates/sigil-cli/tests/cdm_conformance.rs` (the snapshot
records it, and the test refuses to compare across revisions).

## Fetch

```sh
scripts/fetch-cdm.sh master            # pinned SHA, ~3 MB sparse checkout
scripts/fetch-cdm.sh legacy            # LEGACY_TAG (or: scripts/fetch-cdm.sh legacy 6.7.0)
scripts/fetch-cdm.sh master --force    # refetch / repair
```

Default cache location is `<repo>/target/cdm-corpus/<master|legacy>`
(gitignored); override with `SIGIL_CDM_CACHE=<dir>`. Fetching is
idempotent (skips when the pinned revision is already checked out and
verified), uses a shallow blobless clone with sparse-checkout of
`rosetta-source/src/main/rosetta` only, verifies the checked-out HEAD
against the pin, and writes a `NOTICE.md` with the required attribution
into the corpus directory.

## Run

```sh
cargo test -p sigil-cli --test cdm_conformance
```

Without the corpus the test prints a skip note and passes (so the normal
suite stays green on machines that never fetched it). With the corpus it
runs the analysis twice and asserts both runs produce identical summaries
(determinism guard) before comparing against the snapshot; budget ~20 s.

## Snapshot

`tests/cdm/master-snapshot.json` contains, at the pinned SHA:

- corpus totals (files, lines),
- per-file parse status (`"ok"` or the syntax error code — the corpus is a
  flat directory, so file names are unique keys),
- syntax diagnostics by code (corpus-wide),
- resolution diagnostics by code, corpus-wide **and** per file,
- element totals per `ElementKind` (corpus files only; the embedded
  `com.rosetta.model` builtins are excluded).

Regenerate deliberately after an intentional parser/resolver change or a
re-pin:

```sh
UPDATE_CDM_SNAPSHOT=1 cargo test -p sigil-cli --test cdm_conformance
```

Review the diff: it is the exact behavioural footprint of the change over
the whole CDM. A regeneration that changes `parse.ok` downwards without
that being the intent is a regression.

## Re-pinning on upstream movement

1. Update `MASTER_SHA` in `scripts/fetch-cdm.sh` and in
   `crates/sigil-cli/tests/cdm_conformance.rs`.
2. `scripts/fetch-cdm.sh master --force`
3. `UPDATE_CDM_SNAPSHOT=1 cargo test -p sigil-cli --test cdm_conformance`
4. Inspect the snapshot diff and commit it with a rationale.

## License / attribution

The CDM is distributed under the **Community Specification License 1.0**,
whose §1.2 requires attribution. The fetch script prints the attribution
on every fetch and writes it to `NOTICE.md` inside the corpus directory;
the corpus itself is never committed to this repo — only the derived
numeric snapshot is.

## Known-diagnostic landscape at `MASTER_SHA`

Recorded verbatim by the snapshot; none of these are chased to zero:

- **Parse: 142/145 ok** (`E0001` × 3, known sigil parser bugs):
  - `base-staticdata-codelist-type.rosetta` — `condition` inside a type;
  - `legaldocumentation-csa-func.rosetta`, `margin-schedule-func.rosetta`
    — decimal literals (`1.00`, `0.01`) in expressions.
- **`E0101` × 3799** — the corpus references the external `fpml.*` model
  (mapping functions under `ingest-fpml-*`), which is *not* part of this
  corpus; every such reference is unknown by construction, and expression
  symbols cascade from the missing types. Also a handful of unknown
  symbols over corpus-local names.
- **`E0104` × 29** — duplicate definitions: upstream declares *dispatch
  overloads* (several `func`s sharing one name, discriminated by a
  dispatch attribute, e.g. `YearFraction`, `DayCountBasis`); sigil's
  per-kind namespace flags the re-declarations (first wins).
- **`E0107` × 53** — mostly dispatch attributes not found on the winning
  overload's inputs (a direct consequence of the `E0104` overload
  collapse), plus some unknown attribute/enum-value references.
- **`E0106` × 2** — reference target of the wrong kind
  (`CalculationPeriod` used where a type is expected).

When the parser bugs above are fixed and the snapshot is regenerated, the
`E0001` entries disappear (145/145) and the per-file resolution counts may
shift for the newly-parsed files — that is expected; regenerate and
review.

## Phase 2: Java-oracle differential over the legacy corpus (issue #10)

Phase 1 is deliberately sigil-only. Phase 2 closes the loop against the
**Java behavioural oracle** (9.58.1): where the `tests/oracle/` fixtures
prove parity on small hand-written files, this proves it on ~80 files of
real published Rune by running the *whole* legacy corpus through both
tools and comparing the two dumps per file.

### Invocation

```sh
cargo build -p sigil-cli
python3 scripts/oracle_compare.py --cdm       # alias: --cdm-legacy
```

Requires JDK 21 + Maven (as for the fixture comparison). The runner
(`scripts/oracle_compare.py`):

1. ensures the legacy corpus exists (invokes `scripts/fetch-cdm.sh
   legacy`; idempotent) and errors with fetch instructions otherwise;
2. runs `sigil check` over all files and removes the parse failures from
   the value comparison (reverse drift, below — it prints the exact set on
   every run);
3. removes anything on the in-script skip list (`CDM_SKIP_LIST`, per-file
   reason strings; empty today, see below);
4. runs **both tools once over the whole comparable workspace** — sigil
   `model` in one invocation, the oracle dumper in one JVM with all files
   in `-Dexec.args` — and compares **per-file slices** of the two dumps;
5. applies the fixture normalization (`normalize()`) plus the
   CDM-specific reductions documented below, diffs per file, prints the
   summary (`compared N, matched M, excluded-skip-list K, mismatched L`
   plus the first differing leaf paths per mismatch), and exits non-zero
   on any unskipped mismatch.

Whole-corpus input is a correctness requirement, not an optimization: the
oracle resolves cross-file references only against the resources loaded
into that JVM, and sigil's `resolve()` is whole-workspace by design.
Single-file (or small-batch) oracle runs leave cross-file references
unresolved and produce false mismatches — the comparison therefore slices
whole-corpus dumps; it never runs the tools per file. One JVM for the
whole corpus is also the measured sweet spot (~8–9 s, ~3 MB of JSON for
81 files): "batching" means many files per `-Dexec.args`, never one JVM
per file, and a batch small enough to split the workspace would silently
unresolve cross-batch references.

Measured at implementation (sigil @ main, oracle 9.58.1): ~20 s end to end
(sigil `model` ~5 s, oracle ~9 s, remainder JVM/Maven overhead).

### Coverage (measured)

| | |
| --- | --- |
| corpus files (CDM 6.7.0) | 99 |
| sigil parse failures — reverse drift, excluded | 18 (enumerated every run) |
| excluded via skip list | 0 |
| compared & matched | 81 / 81 |
| mismatched | 0 |

The Phase 2 probe estimated "79 clean / 20 failing"; the current sigil
build parses 81 of 99. The runner derives the failing set from `sigil
check` on every invocation, so the table above can always be re-derived.

### Reverse drift: 9.56-era synonym syntax (the 18 parse failures)

The legacy corpus uses the 9.56-era mapping syntax — attribute-level
`[synonym ISO20022 value "..."]` qualifiers and top-level
`synonym source ...` declarations — which the current Rune grammar (the
one sigil's parser implements) dropped: the 9.58.1 oracle still accepts
it, sigil rejects it. This is **reverse drift** (sigil stricter than the
oracle, version skew between the two reference points), the mirror image
of the "newer-main-only syntax" caveats in
[docs/compatibility.md](compatibility.md). It is **not** to be fixed:
newer CDM replaced synonyms with ingest mappings, and re-widening sigil's
grammar for dead syntax would break the main-grammar contract. The
differential discovers the failing files from parse status (never a
silent glob) and compares the remaining 81 files value-by-value. The 18
files (9 enum/type files with `[synonym ... value]` qualifiers + 9
`mapping-*-synonym.rosetta` files with `synonym source`) are printed by
every run.

### Skip list

`CDM_SKIP_LIST` in `scripts/oracle_compare.py` maps corpus file names to
explicit reason strings, mirroring the oracle-version caveats in
[docs/compatibility.md](compatibility.md). A bare glob is deliberately
avoided. **It is empty today**: the divergences anticipated by the probe
turned out to be either absent (CDM 6.7.0 contains no `expr as Type`
casts at all, so no file needs excluding for the as-cast parse
divergence) or reducible to a common shape by normalization (bare
`one-of`, below). The mechanism stays so a future genuinely-unfixable
divergence gets an enumerated entry with a reason instead of a glob.

### Normalizations (why the two dumps can agree at all)

Applied by `normalize_cdm()` to both sides before the per-file diff;
each entry is a documented, shape-exact mapping — never a value guess.
Counts are leaf-level diffs observed over the corpus at implementation.

- **Doc references, reduced everywhere.** The fixture normalization
  (`normalize_doc_refs`) only reached elements/attributes/conditions/enum
  values; CDM functions also carry `docReferences` on their `inputs`
  (~290 leaf diffs in `legaldocumentation-csa-func` alone). The CDM pass
  applies the same reduction recursively. In whole-corpus mode the oracle
  resolves `body`/`corpora` links (269 corpora links, none null) to the
  short target names, which equal sigil's written names — so the reduced
  shape compares for real on this corpus. (The `body` link stays forced
  to `null` on both sides per the fixture rationale: 9.58.1's linker
  cannot resolve the bodyType-style reference in the fixtures' shape.)
- **Dispatch links — no normalization.** The probe expected the known
  9.58.1 dispatch-linkage limitation to leave `dispatch`
  `enumeration`/`value` unresolved on the oracle side. Measured: in
  whole-corpus mode the oracle resolves all 28 legacy dispatch links
  (`base-datetime-daycount-func` ×20, `observable-asset-calculatedrate-func`
  ×3, `product-asset-floatingrate-func` ×5) and they match sigil
  exactly, so the comparison keeps them (more coverage, not less). The
  limitation manifests only when the dispatch target's file is not loaded
  in the same JVM — i.e. in single-file oracle runs, which this
  differential does not do (see above); see
  [docs/compatibility.md](compatibility.md).
- **Operation path segments, dropped (`assignRoot` kept).** 9.58.1 leaves
  the `set x -> a -> b` segment chain unresolved once it leaves the
  function's own file (occasionally even the first segment); sigil
  resolves the chain (~109 leaf diffs across `event-common-func`,
  `legaldocumentation-csa-func`, `margin-schedule-func`,
  `observable-event-func`, `product-asset-calculation-func`,
  `product-template-func`). The assign root resolves on both sides and
  stays compared; only the segment chain is dropped.
- **Constructor pair keys, dropped.** 9.58.1 cannot link a constructor
  pair's `key` crossref to the constructed type's attributes across
  files; sigil keeps the written name (70 leaf diffs in
  `event-common-func`, `margin-schedule-func`, `product-asset-func`).
  Pair values stay fully compared.
- **Bare `one-of`, rewritten on the sigil side.** The parameter-less
  `condition X: one-of` form (20 sites in 10 files) is defined by *both*
  grammars as `OneOfOperation` over a derived implicit `item` (9.58.1
  emits `{"kind": "OneOf", "argument": {"kind": "ImplicitVariable"}}`);
  sigil's parser currently shapes it as `Binary("one", "-", "of")`. The
  differential rewrites exactly that shape — `Binary` with op `-` between
  two argument-less symbol references named `one` and `of` — to the
  grammatical shape, so the rest of those 10 files is still compared.
  Fixing the parser itself is out of scope for the differential (and the
  form does not occur in the fixtures, where sigil's `one-of` handling
  already matches).
- **Comment-polluted reference texts.** The 9.58.1 dumper slices
  cross-reference text from the node model, which drags a directly
  preceding `//` line comment into the value (10 sites: symbol references
  in `event-common-func`, switch guard targets in
  `product-collateral-func`). The pass keeps the last line of any
  multi-line reference text (sigil never emits comments).
- **Annotation qualifier paths.** sigil serializes the structured
  qualifier path (`{data, attributes}`); 9.58.1 only a `Path` marker (its
  qualifier path model differs and is outside the comparison scope). The
  pass reduces both to the marker (3 files).
- **Qualifiable configurations.** Key-name drift: sigil serializes
  `q_type`, the oracle `qType` — renamed to the oracle's. The
  configuration root is compared as the bare target name (sigil does not
  serialize the resolved FQN; the last segment of the oracle's FQN is
  exactly that name). One configuration in the corpus
  (`event-qualification-func`).
