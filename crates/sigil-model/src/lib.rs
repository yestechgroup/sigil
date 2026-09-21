//! The semantic model: sigil's native Rust representation of Rune models.
//!
//! This mirrors the Rune Ecore metamodel (`com.regnosys.rosetta.rosetta`,
//! `...rosetta.simple`) rather than the surface grammar. It knows nothing
//! about chumsky or the parser: `sigil-syntax` ASTs are *lowered* into these
//! types, and this crate is the compatibility boundary for the rest of the
//! toolchain (resolution, validation, serialization, codegen).

use serde::Serialize;
use sigil_diag::Span;

pub mod expr;

/// Reference to a type by (possibly qualified) name, resolved later.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct TypeRef {
    pub name: String,
    pub arguments: Vec<TypeCallArgument>,
    /// Set during resolution: the element this reference points at.
    #[serde(skip_serializing)]
    pub resolved: Option<usize>,
    /// Source span of the reference text (not serialized: the canonical JSON
    /// shape must stay stable).
    #[serde(skip_serializing)]
    pub span: Span,
}

impl TypeRef {
    pub fn unresolved(name: impl Into<String>) -> Self {
        TypeRef::unresolved_spanned(name, Span::default())
    }

    /// An unresolved reference carrying the source span of its text.
    pub fn unresolved_spanned(name: impl Into<String>, span: Span) -> Self {
        TypeRef {
            name: name.into(),
            arguments: Vec::new(),
            resolved: None,
            span,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TypeCallArgument {
    pub parameter: String,
    #[serde(rename = "value")]
    pub argument_value: ArgumentValue,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value")]
pub enum ArgumentValue {
    Reference(String),
    Int(i128),
    Number(String),
    Str(String),
    Bool(bool),
}

/// `(min..max)` / `(min..*)` — first-class, mirroring `RosettaCardinality`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Cardinality {
    pub min: u32,
    pub max: CardinalityMax,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CardinalityMax {
    Finite(u32),
    #[serde(rename = "*")]
    Unbounded,
}

impl Cardinality {
    /// Mirrors `RosettaCardinality.toConstraintString()`.
    pub fn to_constraint_string(&self) -> String {
        match self.max {
            CardinalityMax::Unbounded => format!("({}..*)", self.min),
            CardinalityMax::Finite(max) => format!("({}..{})", self.min, max),
        }
    }
}

/// Render an argument value the way the source writes it (used by the
/// expression printer for constructor type arguments).
pub fn argument_value_text(value: &ArgumentValue) -> String {
    match value {
        ArgumentValue::Reference(r) => r.clone(),
        ArgumentValue::Int(i) => i.to_string(),
        ArgumentValue::Number(n) => n.clone(),
        ArgumentValue::Str(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
        ArgumentValue::Bool(b) => if *b { "True" } else { "False" }.to_string(),
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct AnnotationRef {
    pub annotation: String,
    pub attribute: Option<String>,
    pub qualifiers: Vec<AnnotationQualifier>,
    /// Set during resolution: the annotation declaration this ref points at.
    #[serde(skip_serializing)]
    pub annotation_resolved: Option<usize>,
    /// Source span of the `[annotation ...]` reference (not serialized).
    #[serde(skip_serializing)]
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnnotationQualifier {
    pub name: String,
    #[serde(rename = "value")]
    pub qualifier_value: QualifierValue,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "value")]
pub enum QualifierValue {
    Str(String),
    Path {
        data: String,
        attributes: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct DocReference {
    pub body: String,
    pub corpora: Vec<String>,
    pub segments: Vec<(String, String)>,
    pub rationales: Vec<Rationale>,
    pub structured_provision: Option<String>,
    pub provision: Option<String>,
    pub reported_field: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Rationale {
    pub rationale: Option<String>,
    pub rationale_author: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LabelAnnotation {
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleReference {
    pub rule: Option<String>,
    pub empty: bool,
    /// Set during resolution: the reporting rule this reference points at
    /// (not serialized; the canonical JSON renders the resolved FQN).
    #[serde(skip_serializing)]
    pub resolved: Option<usize>,
}

/// `simple.Data` / `simple.Choice` (choices keep their options as
/// attributes with implicit `(0..1)` cardinality, as in the Java model).
#[derive(Debug, Clone, Serialize)]
pub struct Data {
    pub name: String,
    pub is_choice: bool,
    pub definition: Option<String>,
    pub super_type: Option<TypeRef>,
    pub annotations: Vec<AnnotationRef>,
    pub doc_references: Vec<DocReference>,
    pub attributes: Vec<Attribute>,
    /// `simple.Condition`s declared on this type (after the attributes, as
    /// in the grammar).
    pub conditions: Vec<Condition>,
    /// Source span of the whole declaration (not serialized).
    #[serde(skip_serializing)]
    pub span: Span,
}

/// `simple.Condition`: a named boolean expression attached to a type.
#[derive(Debug, Clone, Serialize)]
pub struct Condition {
    pub name: Option<String>,
    pub definition: Option<String>,
    pub expression: expr::Expr,
    pub annotations: Vec<AnnotationRef>,
    pub doc_references: Vec<DocReference>,
    /// Source span of the whole condition (not serialized).
    #[serde(skip_serializing)]
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct Attribute {
    pub name: String,
    pub is_override: bool,
    #[serde(rename = "type")]
    pub type_ref: TypeRef,
    pub cardinality: Cardinality,
    pub definition: Option<String>,
    pub annotations: Vec<AnnotationRef>,
    pub labels: Vec<LabelAnnotation>,
    pub rule_references: Vec<RuleReference>,
    pub doc_references: Vec<DocReference>,
    /// Source span of the whole attribute declaration (not serialized).
    #[serde(skip_serializing)]
    pub span: Span,
}

/// `RosettaEnumeration`
#[derive(Debug, Clone, Serialize)]
pub struct Enumeration {
    pub name: String,
    pub definition: Option<String>,
    pub super_type: Option<TypeRef>,
    pub annotations: Vec<AnnotationRef>,
    pub doc_references: Vec<DocReference>,
    pub values: Vec<EnumValue>,
    /// Source span of the whole declaration (not serialized).
    #[serde(skip_serializing)]
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct EnumValue {
    pub name: String,
    pub display: Option<String>,
    pub definition: Option<String>,
    pub annotations: Vec<AnnotationRef>,
    pub doc_references: Vec<DocReference>,
    /// Source span of the value declaration (not serialized).
    #[serde(skip_serializing)]
    pub span: Span,
}

/// `simple.Annotation` (a declaration)
#[derive(Debug, Clone, Serialize)]
pub struct Annotation {
    pub name: String,
    pub definition: Option<String>,
    pub prefix: Option<String>,
    pub attributes: Vec<Attribute>,
    /// Source span of the whole declaration (not serialized).
    #[serde(skip_serializing)]
    pub span: Span,
}

/// `RosettaTypeAlias`
#[derive(Debug, Clone, Serialize)]
pub struct TypeAlias {
    pub name: String,
    pub parameters: Vec<TypeParameter>,
    pub definition: Option<String>,
    #[serde(rename = "type")]
    pub type_ref: TypeRef,
    pub annotations: Vec<AnnotationRef>,
    /// Source span of the whole declaration (not serialized).
    #[serde(skip_serializing)]
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct TypeParameter {
    pub name: String,
    #[serde(rename = "type")]
    pub type_ref: TypeRef,
    pub definition: Option<String>,
}

/// `RosettaBasicType`
#[derive(Debug, Clone, Serialize)]
pub struct BasicType {
    pub name: String,
    pub parameters: Vec<TypeParameter>,
    pub definition: Option<String>,
    /// Source span of the whole declaration (not serialized).
    #[serde(skip_serializing)]
    pub span: Span,
}

/// `RosettaRecordType`
#[derive(Debug, Clone, Serialize)]
pub struct RecordType {
    pub name: String,
    pub definition: Option<String>,
    pub features: Vec<RecordFeature>,
    /// Source span of the whole declaration (not serialized).
    #[serde(skip_serializing)]
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecordFeature {
    pub name: String,
    #[serde(rename = "type")]
    pub type_ref: TypeRef,
}

/// `RosettaExternalFunction` (`library function`)
#[derive(Debug, Clone, Serialize)]
pub struct LibraryFunction {
    pub name: String,
    pub parameters: Vec<LibraryParameter>,
    #[serde(rename = "returnType")]
    pub return_type: TypeRef,
    pub definition: Option<String>,
    /// Source span of the whole declaration (not serialized).
    #[serde(skip_serializing)]
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct LibraryParameter {
    pub name: String,
    #[serde(rename = "type")]
    pub type_ref: TypeRef,
    pub is_array: bool,
}

// ---- functions, rules, reports ---------------------------------------------

/// `simple.Function`. `super_function` and `transform` exist in the current
/// main-branch metamodel but not in the published 9.58.1 oracle; the
/// comparison script normalizes them away.
#[derive(Debug, Clone, Serialize)]
pub struct Function {
    pub name: String,
    pub definition: Option<String>,
    pub super_function: Option<TypeRef>,
    pub transform: Vec<TransformAnnotation>,
    /// The `(attr: Enum->VALUE)` dispatch head, when the function is a
    /// `FunctionDispatch`.
    pub dispatch: Option<FunctionDispatch>,
    pub annotations: Vec<AnnotationRef>,
    pub doc_references: Vec<DocReference>,
    pub inputs: Vec<Attribute>,
    pub output: Option<Attribute>,
    /// `alias` declarations (`simple.ShortcutDeclaration`).
    pub shortcuts: Vec<Shortcut>,
    /// Non-post conditions; they cannot see the output.
    pub conditions: Vec<Condition>,
    /// `set`/`add` statements.
    pub operations: Vec<Operation>,
    /// Post-conditions; they see everything.
    pub post_conditions: Vec<Condition>,
    /// Source span of the whole declaration (not serialized).
    #[serde(skip_serializing)]
    pub span: Span,
}

/// The dispatch head of a `FunctionDispatch` function. References are kept
/// as written; resolution checks them (attribute ∈ inputs, value ∈ enum).
#[derive(Debug, Clone, Serialize)]
pub struct FunctionDispatch {
    pub attribute: String,
    #[serde(skip_serializing)]
    pub attribute_span: Span,
    pub enumeration: String,
    #[serde(skip_serializing)]
    pub enumeration_span: Span,
    pub value: String,
    #[serde(skip_serializing)]
    pub value_span: Span,
}

/// A transform annotation on a function: `[ingest X]`, `[enrich]`,
/// `[projection Y]`. The reference targets a `Schema` element or a value of
/// the built-in `SerializationFormat` enum; it is kept as written.
#[derive(Debug, Clone, Serialize)]
pub struct TransformAnnotation {
    pub kind: TransformKind,
    pub reference: Option<String>,
    #[serde(skip_serializing)]
    pub reference_span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TransformKind {
    Ingest,
    Enrich,
    Projection,
}

impl TransformKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TransformKind::Ingest => "ingest",
            TransformKind::Enrich => "enrich",
            TransformKind::Projection => "projection",
        }
    }
}

/// `simple.ShortcutDeclaration` (`alias name: expr`).
#[derive(Debug, Clone, Serialize)]
pub struct Shortcut {
    pub name: String,
    pub definition: Option<String>,
    pub expression: expr::Expr,
    /// Source span of the whole declaration (not serialized).
    #[serde(skip_serializing)]
    pub span: Span,
}

/// `simple.Operation`: `set|add root (-> feature)*: expr`.
#[derive(Debug, Clone, Serialize)]
pub struct Operation {
    pub definition: Option<String>,
    pub add: bool,
    /// The assign root: the function's output or a shortcut (alias) name.
    pub assign_root: String,
    #[serde(skip_serializing)]
    pub assign_root_span: Span,
    /// The `-> feature` chain after the assign root.
    pub path: Vec<PathSegment>,
    pub expression: expr::Expr,
    /// Source span of the whole operation (not serialized).
    #[serde(skip_serializing)]
    pub span: Span,
}

/// One `-> feature` step of an operation path.
#[derive(Debug, Clone, Serialize)]
pub struct PathSegment {
    pub feature: String,
    /// Set during resolution: the attribute this segment points at is
    /// verified to exist on the receiver type chain (the id is not a flat
    /// element id — attributes are not indexed elements — so only presence
    /// is recorded).
    #[serde(skip_serializing)]
    pub resolved: bool,
    #[serde(skip_serializing)]
    pub span: Span,
}

/// `RosettaRule` (`reporting rule` / `eligibility rule`).
#[derive(Debug, Clone, Serialize)]
pub struct Rule {
    pub name: String,
    pub definition: Option<String>,
    pub eligibility: bool,
    /// The `from TypeCall` input, when written.
    pub input: Option<TypeRef>,
    pub doc_references: Vec<DocReference>,
    pub expression: expr::Expr,
    /// Source span of the whole declaration (not serialized).
    #[serde(skip_serializing)]
    pub span: Span,
}

/// A resolved-by-name reference to a named element, rendered in canonical
/// JSON as the resolved FQN (falling back to the written name).
#[derive(Debug, Clone, Serialize)]
pub struct NamedRef {
    pub name: String,
    #[serde(skip_serializing)]
    pub resolved: Option<usize>,
    #[serde(skip_serializing)]
    pub span: Span,
}

/// One `(Segment "ref")` pair of a report's regulatory reference.
#[derive(Debug, Clone, Serialize)]
pub struct SegmentReference {
    pub segment: NamedRef,
    pub reference: String,
}

/// The regulatory reference (`Body Corpus (Segment "ref")*`) of a report.
#[derive(Debug, Clone, Serialize)]
pub struct RegulatoryRef {
    pub body: NamedRef,
    pub corpora: Vec<NamedRef>,
    pub segments: Vec<SegmentReference>,
}

/// The `in ...` timing keyword of a report. Grammar-only: the Ecore model
/// does not store it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportTiming {
    RealTime,
    T(u32),
    Asatp,
}

impl serde::Serialize for ReportTiming {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.as_str())
    }
}

impl ReportTiming {
    pub fn as_str(self) -> String {
        match self {
            ReportTiming::RealTime => "real-time".to_string(),
            ReportTiming::T(n) => format!("T+{n}"),
            ReportTiming::Asatp => "ASATP".to_string(),
        }
    }
}

/// `RosettaReport`.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub regulatory: RegulatoryRef,
    pub timing: ReportTiming,
    /// The `from TypeCall` input type.
    pub input_type: TypeRef,
    /// The `when R (and R)*` eligibility rules.
    pub eligibility_rules: Vec<NamedRef>,
    /// The `with type T` report type.
    pub report_type: NamedRef,
    /// The optional `with source S` rule source.
    pub rule_source: Option<NamedRef>,
    /// Source span of the whole declaration (not serialized).
    #[serde(skip_serializing)]
    pub span: Span,
}

/// `RosettaExternalRuleSource` (`rule source`).
#[derive(Debug, Clone, Serialize)]
pub struct ExternalRuleSource {
    pub name: String,
    pub super_source: Option<TypeRef>,
    pub classes: Vec<ExternalClass>,
    /// Source span of the whole declaration (not serialized).
    #[serde(skip_serializing)]
    pub span: Span,
}

/// One `Data:` class inside a rule source.
#[derive(Debug, Clone, Serialize)]
pub struct ExternalClass {
    pub data: TypeRef,
    pub attributes: Vec<ExternalAttribute>,
    /// Source span of the class (not serialized).
    #[serde(skip_serializing)]
    pub span: Span,
}

/// `(+|-) attr [ruleReference R]*` inside a rule-source class.
#[derive(Debug, Clone, Serialize)]
pub struct ExternalAttribute {
    /// `true` for `+`, `false` for `-`.
    pub add: bool,
    pub attribute: String,
    #[serde(skip_serializing)]
    pub attribute_span: Span,
    pub rule_references: Vec<RuleReference>,
    /// Set during resolution: the attribute exists on the class's data type.
    #[serde(skip_serializing)]
    pub resolved: bool,
    /// Source span of the entry (not serialized).
    #[serde(skip_serializing)]
    pub span: Span,
}

/// `Schema Name Format` (main-branch metamodel; no 9.58.1 counterpart).
#[derive(Debug, Clone, Serialize)]
pub struct Schema {
    pub name: String,
    /// A value of the built-in `SerializationFormat` enum, as written.
    pub format: String,
    #[serde(skip_serializing)]
    pub format_span: Span,
    pub definition: Option<String>,
    pub annotations: Vec<AnnotationRef>,
    /// Source span of the whole declaration (not serialized).
    #[serde(skip_serializing)]
    pub span: Span,
}

/// `RosettaBody`.
#[derive(Debug, Clone, Serialize)]
pub struct Body {
    pub name: String,
    pub body_type: String,
    pub definition: Option<String>,
    /// Source span of the whole declaration (not serialized).
    #[serde(skip_serializing)]
    pub span: Span,
}

/// `RosettaCorpus`.
#[derive(Debug, Clone, Serialize)]
pub struct Corpus {
    pub name: String,
    pub corpus_type: String,
    pub display_name: Option<String>,
    /// The parent body, as written.
    pub body: Option<String>,
    pub definition: Option<String>,
    /// Source span of the whole declaration (not serialized).
    #[serde(skip_serializing)]
    pub span: Span,
}

/// `RosettaSegment`.
#[derive(Debug, Clone, Serialize)]
pub struct Segment {
    pub name: String,
    /// Source span of the whole declaration (not serialized).
    #[serde(skip_serializing)]
    pub span: Span,
}

/// `RosettaMetaType`.
#[derive(Debug, Clone, Serialize)]
pub struct MetaType {
    pub name: String,
    #[serde(rename = "type")]
    pub type_ref: TypeRef,
    /// Source span of the whole declaration (not serialized).
    #[serde(skip_serializing)]
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
// `Function` carries far more members than, say, `Segment`; mirroring the
// Ecore classes 1:1 is worth the extra stack a `Data`/`Function` variant
// costs here.
#[allow(clippy::large_enum_variant)]
#[serde(tag = "kind")]
pub enum SemanticElement {
    Data(Data),
    Enumeration(Enumeration),
    Annotation(Annotation),
    TypeAlias(TypeAlias),
    BasicType(BasicType),
    RecordType(RecordType),
    LibraryFunction(LibraryFunction),
    Function(Function),
    Rule(Rule),
    Report(Report),
    ExternalRuleSource(ExternalRuleSource),
    Schema(Schema),
    Body(Body),
    Corpus(Corpus),
    Segment(Segment),
    MetaType(MetaType),
}

impl SemanticElement {
    pub fn name(&self) -> &str {
        match self {
            SemanticElement::Data(d) => &d.name,
            SemanticElement::Enumeration(e) => &e.name,
            SemanticElement::Annotation(a) => &a.name,
            SemanticElement::TypeAlias(t) => &t.name,
            SemanticElement::BasicType(b) => &b.name,
            SemanticElement::RecordType(r) => &r.name,
            SemanticElement::LibraryFunction(f) => &f.name,
            SemanticElement::Function(f) => &f.name,
            SemanticElement::Rule(r) => &r.name,
            SemanticElement::Report(_) => "",
            SemanticElement::ExternalRuleSource(s) => &s.name,
            SemanticElement::Schema(s) => &s.name,
            SemanticElement::Body(b) => &b.name,
            SemanticElement::Corpus(c) => &c.name,
            SemanticElement::Segment(s) => &s.name,
            SemanticElement::MetaType(m) => &m.name,
        }
    }

    /// Source span of the element's declaration.
    pub fn span(&self) -> Span {
        match self {
            SemanticElement::Data(d) => d.span,
            SemanticElement::Enumeration(e) => e.span,
            SemanticElement::Annotation(a) => a.span,
            SemanticElement::TypeAlias(t) => t.span,
            SemanticElement::BasicType(b) => b.span,
            SemanticElement::RecordType(r) => r.span,
            SemanticElement::LibraryFunction(f) => f.span,
            SemanticElement::Function(f) => f.span,
            SemanticElement::Rule(r) => r.span,
            SemanticElement::Report(r) => r.span,
            SemanticElement::ExternalRuleSource(s) => s.span,
            SemanticElement::Schema(s) => s.span,
            SemanticElement::Body(b) => b.span,
            SemanticElement::Corpus(c) => c.span,
            SemanticElement::Segment(s) => s.span,
            SemanticElement::MetaType(m) => m.span,
        }
    }

    /// Whether this element can be the target of a type reference
    /// (implementors of `RosettaType`).
    pub fn is_type(&self) -> bool {
        matches!(
            self,
            SemanticElement::Data(_)
                | SemanticElement::Enumeration(_)
                | SemanticElement::TypeAlias(_)
                | SemanticElement::BasicType(_)
                | SemanticElement::RecordType(_)
        )
    }

    /// Whether this element can be the target of an annotation reference.
    pub fn is_annotation(&self) -> bool {
        matches!(self, SemanticElement::Annotation(_))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Scope {
    pub name: String,
    pub definition: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportEntry {
    /// The imported namespace, with `.*` appended when `wildcard` is set,
    /// mirroring `Import.importedNamespace` in the Ecore model.
    #[serde(rename = "importedNamespace")]
    pub imported_namespace: String,
    pub wildcard: bool,
    #[serde(rename = "namespaceAlias")]
    pub namespace_alias: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum QualifiableType {
    Event,
    Product,
}

#[derive(Debug, Clone, Serialize)]
pub struct Configuration {
    pub q_type: QualifiableType,
    pub root: TypeRef,
}

/// One parsed-and-lowered `.rosetta` file — the Rust analogue of an
/// `RosettaModel` EObject.
#[derive(Debug, Clone, Serialize)]
pub struct ModelFile {
    pub name: String,
    pub namespace: String,
    pub overridden: bool,
    pub scope: Option<Scope>,
    pub version: Option<String>,
    pub imports: Vec<ImportEntry>,
    pub configurations: Vec<Configuration>,
    pub elements: Vec<SemanticElement>,
}
