# Sigil ↔ Rune compatibility matrix

Sigil is a native-Rust reimplementation of the **Rune DSL** (the language of
`.rosetta` files), with the FINOS Java implementation
([finos/rune-dsl](https://github.com/finos/rune-dsl)) as the behavioural
oracle. This document is the working specification: it maps each Rune
concept to its syntax, its Ecore class in the Java implementation, and the
sigil IR that models it.

The oracle relationship is deliberately explicit:

> The Java implementation is the behavioural oracle; the Rune grammar
> (`docs/reference/Rosetta.xtext`) describes the surface language; sigil's
> semantic model (`sigil-model`) is the compatibility boundary.

## Compatibility levels

Milestones are defined by *what "compatible" means* at each stage:

1. **Syntactic** — parse every existing `.rosetta` file.
2. **Semantic** — same source produces the same resolved model (types,
   cardinalities, inheritance, imports, annotations, diagnostics).
3. **Model** — an equivalent EMF/Ecore model can be produced from sigil's.
4. **Behavioural** — expressions and functions evaluate identically.
5. **Generator** — equivalent generated output.
6. **Ecosystem** — CDM and other published models work unchanged.

Milestone 1 targets levels 1–2 for the model subset below.

## Concept matrix (Milestone 1)

| Rune concept | Syntax | Ecore class | Sigil IR | Resolved? | Status |
| --- | --- | --- | --- | --- | --- |
| namespace | `namespace a.b` (+ `override`, `scope`, `version`) | `RosettaModel` | `ModelFile.namespace/scope/version` | n/a | done |
| import | `import a.b.* as x` | `Import` | `ModelFile.imports` | yes | done |
| qualifiable root | `isEvent root Foo;` | `RosettaQualifiableConfiguration` | `ModelFile.configurations` | yes | done |
| data type | `type Foo extends Bar:` | `simple.Data` | `SemanticElement::Data` | yes | done |
| choice | `choice Foo:` | `simple.Choice` | `SemanticElement::Data { is_choice }` | yes | done |
| attribute | `attr int (1..1) <"doc">` | `simple.Attribute` + `RosettaCardinality` | `Attribute` + `Cardinality` | yes | done |
| attribute override | `override attr int (1..1)` | `Attribute.override` | `Attribute.is_override` | yes | done |
| cardinality | `(0..1)` `(1..*)` `(0..10)` | `RosettaCardinality` | `Cardinality { min, max }` | n/a | done |
| enum | `enum Foo extends Bar:` | `RosettaEnumeration` | `SemanticElement::Enumeration` | yes | done |
| enum value | `EUR displayName "Euro" <"doc">` | `RosettaEnumValue` | `EnumValue` | n/a | done |
| annotation decl | `annotation metadata: [prefix M] attrs` | `simple.Annotation` | `SemanticElement::Annotation` | yes | done |
| annotation use | `[metadata scheme]`, qualifiers | `AnnotationRef` + `AnnotationQualifier` | `AnnotationRef` | yes | done |
| type alias | `typeAlias int(digits int): number(...)` | `RosettaTypeAlias` | `SemanticElement::TypeAlias` | yes | done |
| basic type | `basicType number(digits int, ...)` | `RosettaBasicType` | `SemanticElement::BasicType` | yes | done |
| record type | `recordType date { day int }` | `RosettaRecordType` | `SemanticElement::RecordType` | yes | done |
| library function | `library function Min(x number) number` | `RosettaExternalFunction` | `SemanticElement::LibraryFunction` | yes | done |
| doc reference | `[docReference for path? Body Corpus (Seg "ref")* …]` | `RosettaDocReference` | `DocReference` (`forPath` sigil-only) | partly | done* |
| label | `[label "text"]`, `[label for path "t"]` | `LabelAnnotation` | `LabelAnnotation` | n/a | done |
| rule reference | `[ruleReference Rules]` / `[ruleReference empty]` | `RuleReferenceAnnotation` | `RuleReference` | target kind | done (phase 4) |
| builtin types | `int`, `string`, `date`, `metadata`, ... | `RosettaBuiltinsService` | embedded `builtin/*.rosetta` | yes | done |
| scoping | local → imports → same-ns → `com.rosetta.model.*` → FQN | `RosettaScopeProvider` | `sigil-resolve` | yes | done |
| condition | `condition Foo: <expr>` inside a type | `simple.Condition` | `Condition` + `expr::Expr` | annotations + symbol heads | done (phase 3/4) |
| func | `func Foo:` (inputs/output/aliases/ops) | `simple.Function` | `SemanticElement::Function` | yes (function-scoped) | done (phase 4) |
| func dispatch | `func Foo(attr: Enum->VALUE)` | `simple.FunctionDispatch` | `Function.dispatch` | dispatch refs | done (phase 4)* |
| func extends | `func Foo extends Bar:` | `simple.Function.superFunction` | `Function.super_function` | target kind | done (phase 4)* |
| transform annotation | `[ingest X]` / `[enrich]` / `[projection Y]` | `simple.TransformAnnotation` | `Function.transform` | ref kept as written | done (phase 4)* |
| alias | `alias name: expr` | `simple.ShortcutDeclaration` | `Function.shortcuts` | expression heads | done (phase 4) |
| operation | `set\|add root (-> seg)*: expr as-key?` | `simple.Operation` + `simple.Segment` | `Function.operations` | root + path chain | done (phase 4) |
| post-condition | `post-condition Foo: <expr>` | `simple.Condition (postCondition)` | `Function.post_conditions` | expression heads | done (phase 4) |
| rule / reporting rule | `reporting rule Foo from T:` | `RosettaRule` | `SemanticElement::Rule` | input + expression heads | done (phase 4) |
| report | `report Body Corpus in T+1 ...` | `RosettaReport` | `SemanticElement::Report` | regulatory refs, rules, types | done (phase 4)* |
| rule source | `rule source Foo extends Bar { }` | `RosettaExternalRuleSource` | `SemanticElement::ExternalRuleSource` | class data, attributes, rule refs | done (phase 4) |
| schema | `schema Foo JSON` | `Schema` | `SemanticElement::Schema` | annotations | done (phase 4)* |
| body / corpus / segment | `body T Foo`, `corpus ...`, `segment Foo` | `RosettaBody` / `RosettaCorpus` / `RosettaSegment` | respective elements | n/a | done (phase 4) |
| metaType | `metaType Foo number` | `RosettaMetaType` | `SemanticElement::MetaType` | type | done (phase 4) |
| expressions | literals, paths, operators, `filter`/`extract`/... | `RosettaExpression` hierarchy | `sigil_model::expr::Expr` (+ normalized JSON and printer) | symbol heads (phase 4) | done (parse, IR, differential) |

Rows marked `*` use syntax or metamodel features that the published 9.58.1
oracle does not have (see "Oracle-version caveats"); they are covered by
sigil-only conformance fixtures and normalized out of the oracle comparison.

### Expression symbol resolution (phase 4)

`SymbolReference` heads inside type conditions, function
conditions/aliases/operations/post-conditions and rule expressions resolve
against the innermost scope first, mirroring
`RosettaScopeProvider.getSymbolParentScope`:

1. inline-function closure parameters (innermost),
2. the function scope: inputs, the output and the aliases — minus the
   output for non-post conditions, and minus the alias itself inside its
   own declaration,
3. the enclosing data type's (inherited) attributes for type conditions,
   or the rule input type's attributes for rule expressions,
4. the file scope (local elements, imports, own namespace, built-ins).

Unknown heads produce `E0101`. A dotted head (`Product.price`,
`com.rosetta.model.number`) resolves as a qualified name or as a local
`Type.feature` chain. Known limitations, deliberately unresolved:

* `->` feature segments inside *expressions* are parsed but not
  type-checked;
* bare heads that only resolve via the expected type (enum values used
  without qualification, `metadata` attributes) are not resolved;
* `ChoiceOperation` attributes, `switch` reference guards, `to-enum` and
  `as` targets are kept as written.

Operation *assign paths* (`set result -> price:`) *are* resolved: the
assign root must be the output or an alias, and each segment must exist on
the receiver type chain (through data inheritance). Paths rooted at an
alias are left unchecked because alias expression types are not inferred.

## Scoping rules (as implemented)

Reproduced from `RosettaScopeProvider` (which extends Xtext's
`ImportedNamespaceAwareLocalScopeProvider`):

1. Elements of the local file.
2. Explicit imports **in declaration order** — first match wins; wildcards
   (`a.b.*`) and aliases (`import a.b.C as c`) honoured.
3. A wildcard import of the file's **own namespace**, so files sharing a
   namespace can reference each other unqualified.
4. The implicit wildcard import of the built-in `com.rosetta.model`
   namespace (`LIB_NAMESPACE`).
5. Fully-qualified names resolve against the global index.

Scoping is kind-filtered: a `typeAlias calculation` and an
`annotation calculation` may coexist in `com.rosetta.model` because type
references and annotation references look in separate symbol spaces.
Duplicate definitions *within* a kind are reported (`E0104`).

## Diagnostic codes

| Code | Meaning |
| --- | --- |
| `E0001` | syntax error (parse) |
| `W0001` | reserved — unused since phase 4 (unsupported-construct skip, kept for future grammar additions) |
| `E0101` | unknown type / super type / unknown expression symbol head |
| `E0102` | unknown annotation |
| `E0103` | annotation has no such attribute |
| `E0104` | duplicate definition within a kind |
| `E0105` | inheritance cycle |
| `E0106` | reference target of the wrong kind |
| `E0107` | unknown attribute (operation path, dispatch attribute, rule-source attribute) or enum value |

## Oracle harness

`tools/oracle-dumper` is a small Maven project that depends on the
published `com.regnosys.rosetta.tests` artifact (9.58.1). It demand-loads
the built-in `com.rosetta.model` resources exactly like the Java test
suite's `ModelHelper`, parses fixture files, resolves proxies, and emits
the same normalized JSON as `sigil model`; `scripts/oracle_compare.py`
diffs the two after normalization (type-argument source text, super-type
targets, configuration roots, type-alias annotations which 9.58.1 lacks).

Oracle-version caveats (9.58.1 versus current main; fixtures avoid them so
the comparison stays meaningful):

* identifiers reserved in 9.58.1 (`alias`, `enums`, `func`, `report`, ...)
  are legal in main — a namespace named `test.func` fails to parse in
  9.58.1;
* annotations on type aliases do not exist in 9.58.1;
* the `as` / `as-key` operators do not exist in 9.58.1 (newer-main syntax);
* `with-meta` inside a type condition crashes 9.58.1 scoping;
* conditions using `required choice` / `optional choice` parse differently
  in 9.58.1 (they interact with the `Choice` rule);
* inline-function parameters in 9.58.1 sit *before* the bracket
  (`filter a, b [a + b]`, or the parameter-less `[body]`), not inside it;
* `func extends`, `TransformAnnotation` (`[ingest ...]`) and `Schema`
  do not exist in 9.58.1: `[ingest X]` parses there as an ordinary
  annotation reference and the comparison normalizes the fields away;
* function *dispatch* (`func F(x: Enum->VALUE)`) parses in 9.58.1 but its
  function-scoped references fail to link (the dispatch attribute, assign
  roots and path segments stay unresolved proxies), so dispatch functions
  are covered by sigil-only conformance fixtures. The failure is specific
  to runs where the dispatch target's file is not loaded into the same
  JVM: over the whole-corpus CDM 6.7.0 differential (`--cdm`, Phase 2 of
  issue #10) all 28 legacy dispatch links resolve on the oracle side and
  match sigil, so there they are compared, not normalized away;
* single-letter names (`enum E:`) are rejected by the 9.58.1 grammar
  (lexer collision with an internal token);
* `docReference for <path>` (attribute-anchored references) exists in the
  reference grammar (main) but is rejected outright by 9.58.1: sigil
  accepts it, parses the `for` path, and carries it in
  `DocReference.forPath` — covered by the sigil-only
  `conformance/model/doc-ref-for-path` fixture, and no `tests/oracle/`
  fixture may use it;
* within a `[docReference ...]`, the tail keywords (`rationale`,
  `rationale_author`, `structured_provision`, `provision`,
  `reportedField`) are Xtext keyword tokens: the corresponding constructs
  win over the greedy `Segment "ref"` pair repetition, and a bare-string
  segment list (`CFTC Part45 "S1" "S2"`) is a syntax error on both sides;
* the grammar's "without left parameter" expression forms (`filter x`,
  `or x`) derive a generated implicit `item` as their missing side in the
  EMF model; sigil mirrors that (`Expr::ImplicitVariable`), so both sides
  agree.

Reverse-drift and cross-file-resolution caveats observed by the CDM 6.7.0
differential (`python3 scripts/oracle_compare.py --cdm`; details and
occurrence counts in `docs/cdm-conformance.md`, Phase 2):

* **reverse drift — 9.56-era synonym syntax**: CDM 6.7.0 (SDK 9.56.0-era)
  uses attribute-level `[synonym <source> value "..."]` qualifiers and
  top-level `synonym source` declarations that main's grammar dropped.
  9.58.1 still parses them; sigil rejects them — 18 of the corpus's 99
  files (the exact set is re-derived and printed on every differential
  run). Sigil stays strict: newer CDM replaced synonyms with ingest
  mappings, and re-widening the grammar for dead syntax is explicitly out
  of scope;
* **`as`-cast divergence — no occurrences to exclude**: the `as` /
  `as-key` operators do not exist in 9.58.1 (bullet above), and CDM 6.7.0
  turned out to contain no `expr as Type` casts at all, so the
  differential's skip list is empty. The genuine parse-*shape* divergence
  on that corpus is instead the parameter-less `condition X: one-of`
  form: both grammars define it as `OneOfOperation` over a derived
  implicit `item` (which is what 9.58.1 emits), but sigil's parser
  currently shapes it as `Binary("one", "-", "of")` — the differential
  normalizes exactly that shape to the grammatical one (20 sites in 10
  files; the fixtures' `currency one-of` has a left operand and already
  agrees);
* **cross-file reference resolution**: 9.58.1 resolves cross-references
  only against the files loaded into the same JVM, and leaves
  *cross-file* links unresolved that sigil resolves whole-workspace —
  operation path segments beyond the function's own file, constructor
  pair keys targeting another file's attributes, and (in single-file
  runs) dispatch links and doc-reference bodies/corpora. The differential
  therefore runs both tools whole-corpus and compares per-file slices;
  what remains genuinely unresolved on the oracle side (path segment
  chains, constructor pair keys) is normalized away, everything else is
  compared. Its dumper can also drag a directly preceding `//` comment
  into a cross-reference's source text; the differential trims reference
  texts to their last line.

`Choice.getConditions()` in the 9.58.1 Ecore model also returns a hardcoded
`one-of item` condition when a choice declares none; sigil mirrors that
derived behaviour in its canonical JSON.

## The comparison artifact

`sigil model <files...>` emits canonical JSON (`format: sigil-model/1`)
with resolved fully-qualified names per reference. The Java oracle dumper
(`tools/oracle-dumper`) emits the same shape from the EMF model, and
`scripts/oracle_compare.py` diffs the two. Note: Rune's *official* JSON
serialization format documented at rune.finos.org serializes model
*instances* (data objects with `@type`/`@key`/`@ref`), not the metamodel —
it is implemented in the serialization phase (issue phase 8) where it
actually applies.

## Real-world corpus: the FINOS Common Domain Model

Beyond the small oracle fixtures, sigil is gated on the full published
**FINOS Common Domain Model** (CDM), pinned by SHA
(`eb0eea955ef8409f034e1a9c28714d00a511a61a`, master; legacy tag `6.7.0`
for the Phase 2 reverse-drift study): 145 `.rosetta` files, ~44k lines,
parsed and resolved whole-workspace against a committed golden snapshot of
parse status, diagnostics by code, and element counts
(`tests/cdm/master-snapshot.json`). The corpus is **not** diagnostic-clean
by design — it references the external `fpml.*` model (`E0101`s) and
declares dispatch overloads that sigil flags as `E0104`s — so this is a
sigil-only golden suite rather than an oracle comparison. The **legacy**
pin (CDM 6.7.0) additionally gets a full value-level Java-oracle
differential: `python3 scripts/oracle_compare.py --cdm` (issue #10
Phase 2; 81 of 99 files compared against the 9.58.1 oracle after
documented normalization — see `docs/cdm-conformance.md` and the
reverse-drift caveats above). Fetch with `scripts/fetch-cdm.sh master` /
`legacy` (auto-skips when absent); see `docs/cdm-conformance.md` for the
full contract, the known-diagnostic landscape at the pin, and the
license/attribution note (CSL 1.0).
