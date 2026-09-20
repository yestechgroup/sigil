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
| doc reference | `[docReference Body Corpus "S1" ...]` | `RosettaDocReference` | `DocReference` | partly | done |
| label | `[label "text"]`, `[label for path "t"]` | `LabelAnnotation` | `LabelAnnotation` | n/a | done |
| rule reference | `[ruleReference Rules]` / `[ruleReference empty]` | `RuleReferenceAnnotation` | `RuleReference` | deferred | done* |
| builtin types | `int`, `string`, `date`, `metadata`, ... | `RosettaBuiltinsService` | embedded `builtin/*.rosetta` | yes | done |
| scoping | local → imports → same-ns → `com.rosetta.model.*` → FQN | `RosettaScopeProvider` | `sigil-resolve` | yes | done |
| condition | `condition Foo: <expr>` inside a type | `simple.Condition` | — | — | **deferred** (W0001, skipped) |
| func | `func Foo:` (inputs/output/aliases/ops) | `simple.Function` | — | — | **deferred** (W0001) |
| rule / reporting rule | `reporting rule Foo from T:` | `RosettaRule` | — | — | **deferred** (W0001) |
| report | `report Body Corpus in T+1 ...` | `RosettaReport` | — | — | **deferred** (W0001) |
| rule source | `rule source Foo extends Bar { }` | `RosettaExternalRuleSource` | — | — | **deferred** (W0001) |
| schema / body / corpus / segment / metaType | `schema Foo JSON`, ... | respective classes | — | — | **deferred** (W0001) |
| expressions | literals, paths, operators, `filter`/`extract`/... | `RosettaExpression` hierarchy | — | — | **deferred** (Phase 3) |

Deferred constructs are *recognised*: the parser emits a `W0001` warning
identifying the construct and skips to the next element boundary, so
models containing them still parse for the model subset. This matches the
milestone plan in issue #1 (phases 3–9 come later).

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
| `W0001` | unsupported construct skipped (parse) |
| `E0101` | unknown type / super type |
| `E0102` | unknown annotation |
| `E0103` | annotation has no such attribute |
| `E0104` | duplicate definition within a kind |
| `E0105` | inheritance cycle |
| `E0106` | reference target of the wrong kind |

## ## Oracle harness

`tools/oracle-dumper` is a small Maven project that depends on the
published `com.regnosys.rosetta.tests` artifact (9.58.1). It demand-loads
the built-in `com.rosetta.model` resources exactly like the Java test
suite's `ModelHelper`, parses fixture files, resolves proxies, and emits
the same normalized JSON as `sigil model`; `scripts/oracle_compare.py`
diffs the two after normalization (type-argument source text, super-type
targets, configuration roots, type-alias annotations which 9.58.1 lacks).

Two oracle-version caveats: identifiers reserved in 9.58.1 (`alias`,
`enums`) are legal in current main, and annotations on type aliases do not
exist in 9.58.1. Fixtures avoid both so the comparison stays meaningful.

## The comparison artifact

`sigil model <files...>` emits canonical JSON (`format: sigil-model/1`)
with resolved fully-qualified names per reference. The Java oracle dumper
(`tools/oracle-dumper`) emits the same shape from the EMF model, and
`scripts/oracle_compare.py` diffs the two. Note: Rune's *official* JSON
serialization format documented at rune.finos.org serializes model
*instances* (data objects with `@type`/`@key`/`@ref`), not the metamodel —
it is implemented in the serialization phase (issue phase 8) where it
actually applies.
