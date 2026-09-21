//! Spanned AST produced by the sigil parser.
//!
//! This tree mirrors the surface grammar of the Rune DSL (see
//! `docs/reference/Rosetta.xtext`). It is intentionally separate from the
//! semantic model in `sigil-model`: the parser is replaceable, this tree is
//! not the canonical representation.

use sigil_diag::Span;

/// A dot-separated qualified name, e.g. `cdm.base.staticdata.party.Party`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct QName(pub String);

impl QName {
    pub fn segments(&self) -> impl Iterator<Item = &str> {
        self.0.split('.')
    }

    pub fn segment_count(&self) -> usize {
        self.0.split('.').count()
    }
}

/// A qualified name together with the source span of its text — used for
/// bare references (super types, configuration roots) that are not part of
/// a larger spanned node.
#[derive(Debug, Clone)]
pub struct SpannedQName {
    pub name: QName,
    pub span: Span,
}

impl std::fmt::Display for QName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone)]
pub struct SourceUnit {
    pub namespace: NamespaceDecl,
    pub scope: Option<(String, Option<String>)>,
    pub version: Option<String>,
    pub imports: Vec<ImportDecl>,
    pub configurations: Vec<QualifiableConfiguration>,
    pub elements: Vec<Element>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct NamespaceDecl {
    /// `override namespace ...`
    pub overridden: bool,
    pub name: QName,
    pub definition: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ImportDecl {
    /// The imported namespace, without any trailing `.*` wildcard.
    pub namespace: QName,
    /// Whether the import ended with `.*`.
    pub wildcard: bool,
    /// `import a.b.C as c`
    pub alias: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualifiableType {
    Event,
    Product,
}

impl QualifiableType {
    pub fn as_str(self) -> &'static str {
        match self {
            QualifiableType::Event => "isEvent",
            QualifiableType::Product => "isProduct",
        }
    }
}

#[derive(Debug, Clone)]
pub struct QualifiableConfiguration {
    pub q_type: QualifiableType,
    pub root: SpannedQName,
    pub span: Span,
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum Element {
    Data(DataDef),
    Choice(DataDef),
    Enumeration(EnumDef),
    Annotation(AnnotationDecl),
    TypeAlias(TypeAliasDef),
    BasicType(BasicTypeDecl),
    RecordType(RecordTypeDecl),
    LibraryFunction(LibraryFunctionDecl),
    Function(FunctionDef),
    Rule(RuleDef),
    Report(ReportDef),
    RuleSource(RuleSourceDef),
    Schema(SchemaDef),
    Body(BodyDef),
    Corpus(CorpusDef),
    Segment(SegmentDecl),
    MetaType(MetaTypeDecl),
    /// A construct recognised but not yet modelled. It carries a diagnostic;
    /// the element is skipped. Dead since phase 4 (every grammar element is
    /// modelled) but kept so future grammar additions have a place to land.
    Unsupported(UnsupportedElement),
}

impl Element {
    pub fn name(&self) -> Option<&str> {
        match self {
            Element::Data(d) | Element::Choice(d) => Some(&d.name),
            Element::Enumeration(e) => Some(&e.name),
            Element::Annotation(a) => Some(&a.name),
            Element::TypeAlias(t) => Some(&t.name),
            Element::BasicType(b) => Some(&b.name),
            Element::RecordType(r) => Some(&r.name),
            Element::LibraryFunction(f) => Some(&f.name),
            Element::Function(f) => Some(&f.name),
            Element::Rule(r) => Some(&r.name),
            Element::RuleSource(s) => Some(&s.name),
            Element::Schema(s) => Some(&s.name),
            Element::Body(b) => Some(&b.name),
            Element::Corpus(c) => Some(&c.name),
            Element::Segment(s) => Some(&s.name),
            Element::MetaType(m) => Some(&m.name),
            // Reports are anonymous root elements.
            Element::Report(_) | Element::Unsupported(_) => None,
        }
    }

    pub fn span(&self) -> Span {
        match self {
            Element::Data(d) | Element::Choice(d) => d.span,
            Element::Enumeration(e) => e.span,
            Element::Annotation(a) => a.span,
            Element::TypeAlias(t) => t.span,
            Element::BasicType(b) => b.span,
            Element::RecordType(r) => r.span,
            Element::LibraryFunction(f) => f.span,
            Element::Function(f) => f.span,
            Element::Rule(r) => r.span,
            Element::Report(r) => r.span,
            Element::RuleSource(s) => s.span,
            Element::Schema(s) => s.span,
            Element::Body(b) => b.span,
            Element::Corpus(c) => c.span,
            Element::Segment(s) => s.span,
            Element::MetaType(m) => m.span,
            Element::Unsupported(u) => u.span,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UnsupportedElement {
    pub keyword: String,
    pub span: Span,
}

/// `type Foo extends Bar: <"..."> ...`
#[derive(Debug, Clone)]
pub struct DataDef {
    pub name: String,
    pub super_type: Option<SpannedQName>,
    pub definition: Option<String>,
    pub doc_references: Vec<DocReference>,
    pub annotations: Vec<AnnotationRef>,
    pub attributes: Vec<AttributeDef>,
    pub conditions: Vec<ConditionDef>,
    pub span: Span,
}

/// `attr int (1..1) <"..."> [metadata scheme]`
#[derive(Debug, Clone)]
pub struct AttributeDef {
    pub name: String,
    pub is_override: bool,
    pub type_call: TypeCall,
    pub cardinality: Cardinality,
    pub definition: Option<String>,
    pub doc_references: Vec<DocReference>,
    pub annotations: Vec<AnnotationRef>,
    pub labels: Vec<LabelAnnotation>,
    pub rule_references: Vec<RuleReference>,
    pub span: Span,
}

/// A type reference with optional type arguments: `number(digits: 30)`.
#[derive(Debug, Clone)]
pub struct TypeCall {
    pub name: QName,
    pub arguments: Vec<TypeCallArgument>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct TypeCallArgument {
    pub parameter: String,
    pub value: TypeCallArgumentValue,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeCallArgumentValue {
    Reference(String),
    Int(i128),
    Number(String),
    Str(String),
    Bool(bool),
}

/// `(1..1)`, `(0..*)`, `(2..10)`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cardinality {
    pub min: u32,
    pub max: CardinalityMax,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardinalityMax {
    Finite(u32),
    Unbounded,
}

impl Cardinality {
    /// Mirrors `RosettaCardinality.toConstraintString()` in the Java implementation.
    pub fn to_constraint_string(&self) -> String {
        match self.max {
            CardinalityMax::Unbounded => format!("({}..*)", self.min),
            CardinalityMax::Finite(max) => format!("({}..{})", self.min, max),
        }
    }
}

/// `[metadata scheme]`, `[qualification Product]`
#[derive(Debug, Clone)]
pub struct AnnotationRef {
    pub annotation: QName,
    pub attribute: Option<String>,
    pub qualifiers: Vec<AnnotationQualifier>,
    pub span: Span,
}

/// `"pointsTo"=VehicleOrder->customer`
#[derive(Debug, Clone)]
pub struct AnnotationQualifier {
    pub name: String,
    pub value: QualifierValue,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum QualifierValue {
    Str(String),
    /// `Trade->price` — a data reference followed by one or more attributes.
    Path(QualifierPath),
}

#[derive(Debug, Clone)]
pub struct QualifierPath {
    pub data: QName,
    pub attributes: Vec<String>,
}

/// `[docReference ...]`
#[derive(Debug, Clone)]
pub struct DocReference {
    pub for_path: Option<AnnotationPath>,
    pub body: QName,
    pub corpora: Vec<QName>,
    /// `(segmentName "segmentRef")` pairs.
    pub segments: Vec<(String, String)>,
    pub rationales: Vec<DocumentRationale>,
    pub structured_provision: Option<String>,
    pub provision: Option<String>,
    pub reported_field: bool,
    pub span: Span,
}

#[derive(Debug, Clone, Default)]
pub struct DocumentRationale {
    pub rationale: Option<String>,
    pub rationale_author: Option<String>,
}

/// `item` or `attr->attr2` / `attr->>attr2` chains used by `[... for ...]`.
#[derive(Debug, Clone)]
pub struct AnnotationPath {
    pub starts_with_item: bool,
    pub segments: Vec<AnnotationPathSegment>,
}

#[derive(Debug, Clone)]
pub struct AnnotationPathSegment {
    pub attribute: String,
    pub deep: bool,
}

/// `[label "Human label"]`
#[derive(Debug, Clone)]
pub struct LabelAnnotation {
    pub for_path: Option<AnnotationPath>,
    pub label: String,
    pub span: Span,
}

/// `[ruleReference CDM.Rule]` / `[ruleReference empty]`
#[derive(Debug, Clone)]
pub struct RuleReference {
    pub for_path: Option<AnnotationPath>,
    pub rule: Option<QName>,
    pub empty: bool,
    pub span: Span,
}

/// `enum Foo extends Bar: <"..."> values...`
#[derive(Debug, Clone)]
pub struct EnumDef {
    pub name: String,
    pub super_type: Option<SpannedQName>,
    pub definition: Option<String>,
    pub doc_references: Vec<DocReference>,
    pub annotations: Vec<AnnotationRef>,
    pub values: Vec<EnumValueDef>,
    pub span: Span,
}

/// `JSON <"description">` / `EUR displayName "Euro"`
#[derive(Debug, Clone)]
pub struct EnumValueDef {
    pub name: String,
    pub display: Option<String>,
    pub definition: Option<String>,
    pub doc_references: Vec<DocReference>,
    pub annotations: Vec<AnnotationRef>,
    pub span: Span,
}

/// `annotation metadata: <"..."> [prefix Meta] attributes...`
#[derive(Debug, Clone)]
pub struct AnnotationDecl {
    pub name: String,
    pub definition: Option<String>,
    pub prefix: Option<String>,
    pub attributes: Vec<AttributeDef>,
    pub span: Span,
}

/// `typeAlias int(digits int, min int, max int): number(...)`
#[derive(Debug, Clone)]
pub struct TypeAliasDef {
    pub name: String,
    pub parameters: Vec<TypeParameter>,
    pub definition: Option<String>,
    pub type_call: TypeCall,
    pub doc_references: Vec<DocReference>,
    pub annotations: Vec<AnnotationRef>,
    pub span: Span,
}

/// `digits int <"...">` inside parameter lists.
#[derive(Debug, Clone)]
pub struct TypeParameter {
    pub name: String,
    pub type_call: TypeCall,
    pub definition: Option<String>,
    pub span: Span,
}

/// `basicType string(minLength int, ...) <"...">`
#[derive(Debug, Clone)]
pub struct BasicTypeDecl {
    pub name: String,
    pub parameters: Vec<TypeParameter>,
    pub definition: Option<String>,
    pub span: Span,
}

/// `recordType date { day int ... }`
#[derive(Debug, Clone)]
pub struct RecordTypeDecl {
    pub name: String,
    pub definition: Option<String>,
    pub features: Vec<RecordFeature>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct RecordFeature {
    pub name: String,
    pub type_call: TypeCall,
    pub span: Span,
}

/// `library function Min(x number, y number) number`
#[derive(Debug, Clone)]
pub struct LibraryFunctionDecl {
    pub name: String,
    /// `(name, type_call, is_array)` triples.
    pub parameters: Vec<(String, TypeCall, bool)>,
    pub return_type: TypeCall,
    pub definition: Option<String>,
    pub span: Span,
}

// ---- expressions -----------------------------------------------------------

/// A spanned expression node (surface AST).
#[derive(Debug, Clone)]
pub struct SpannedExpr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AstCardinalityMod {
    Any,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AstNecessity {
    Optional,
    Required,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AstExistsMod {
    Single,
    Multiple,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AstArithOp {
    Add,
    Subtract,
    Multiply,
    Divide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AstLogicOp {
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AstEqOp {
    Eq,
    NotEq,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AstCmpOp {
    Ge,
    Le,
    Gt,
    Lt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AstFunctionalOp {
    Then,
    Filter,
    Extract,
    Reduce,
    Sort,
    Min,
    Max,
}

/// An inline (closure) function: `a, b [body]`, or the implicit bare form
/// (`params` empty, no brackets were written).
#[derive(Debug, Clone)]
pub struct AstInlineFunction {
    pub parameters: Vec<String>,
    pub body: Box<SpannedExpr>,
}

#[derive(Debug, Clone)]
pub enum AstSwitchGuard {
    Literal(Box<SpannedExpr>),
    Reference(QName),
}

#[derive(Debug, Clone)]
pub struct AstSwitchCase {
    pub guard: Option<AstSwitchGuard>,
    pub expression: SpannedExpr,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    Boolean(bool),
    Str(String),
    Number(String),
    Int(String),
    /// The keyword `empty` (an empty list literal in the EMF model).
    Empty,
    List(Vec<SpannedExpr>),
    Symbol {
        name: QName,
        explicit_args: bool,
        args: Vec<SpannedExpr>,
    },
    /// The implicit variable `item`.
    Item,
    FeatureCall {
        receiver: Box<SpannedExpr>,
        /// `None` for the bare `->` projection (the feature is optional in
        /// the grammar).
        feature: Option<String>,
        deep: bool,
    },
    Arithmetic {
        op: AstArithOp,
        left: Box<SpannedExpr>,
        right: Box<SpannedExpr>,
    },
    Logical {
        op: AstLogicOp,
        left: Box<SpannedExpr>,
        right: Box<SpannedExpr>,
    },
    Equality {
        op: AstEqOp,
        card_mod: Option<AstCardinalityMod>,
        left: Box<SpannedExpr>,
        right: Box<SpannedExpr>,
    },
    Comparison {
        op: AstCmpOp,
        card_mod: Option<AstCardinalityMod>,
        left: Box<SpannedExpr>,
        right: Box<SpannedExpr>,
    },
    Contains {
        left: Box<SpannedExpr>,
        right: Box<SpannedExpr>,
    },
    Disjoint {
        left: Box<SpannedExpr>,
        right: Box<SpannedExpr>,
    },
    Default {
        left: Box<SpannedExpr>,
        right: Box<SpannedExpr>,
    },
    /// `right` is `None` when no separator was written (the lowering fills
    /// the generated `""` literal, as the EMF derived state does).
    Join {
        left: Box<SpannedExpr>,
        right: Option<Box<SpannedExpr>>,
    },
    Conditional {
        if_: Box<SpannedExpr>,
        ifthen: Box<SpannedExpr>,
        elsethen: Option<Box<SpannedExpr>>,
    },
    OnlyExists {
        args: Vec<SpannedExpr>,
        has_parentheses: bool,
    },
    Exists {
        modifier: Option<AstExistsMod>,
        argument: Box<SpannedExpr>,
    },
    Absent {
        argument: Box<SpannedExpr>,
    },
    OnlyElement {
        argument: Box<SpannedExpr>,
    },
    Count {
        argument: Box<SpannedExpr>,
    },
    Flatten {
        argument: Box<SpannedExpr>,
    },
    Distinct {
        argument: Box<SpannedExpr>,
    },
    Reverse {
        argument: Box<SpannedExpr>,
    },
    First {
        argument: Box<SpannedExpr>,
    },
    Last {
        argument: Box<SpannedExpr>,
    },
    Sum {
        argument: Box<SpannedExpr>,
    },
    AsKey {
        argument: Box<SpannedExpr>,
    },
    OneOf {
        argument: Box<SpannedExpr>,
    },
    Choice {
        necessity: AstNecessity,
        attributes: Vec<String>,
        argument: Box<SpannedExpr>,
    },
    ToString {
        argument: Box<SpannedExpr>,
    },
    ToNumber {
        argument: Box<SpannedExpr>,
    },
    ToInt {
        argument: Box<SpannedExpr>,
    },
    ToTime {
        argument: Box<SpannedExpr>,
    },
    ToEnum {
        enumeration: QName,
        argument: Box<SpannedExpr>,
    },
    ToDate {
        argument: Box<SpannedExpr>,
    },
    ToDateTime {
        argument: Box<SpannedExpr>,
    },
    ToZonedDateTime {
        argument: Box<SpannedExpr>,
    },
    Switch {
        argument: Box<SpannedExpr>,
        cases: Vec<AstSwitchCase>,
    },
    WithMeta {
        argument: Box<SpannedExpr>,
        entries: Vec<(String, SpannedExpr)>,
    },
    As {
        type_: QName,
        argument: Box<SpannedExpr>,
    },
    Functional {
        op: AstFunctionalOp,
        argument: Box<SpannedExpr>,
        function: Option<AstInlineFunction>,
    },
    Constructor {
        type_call: TypeCall,
        values: Vec<(String, SpannedExpr)>,
        implicit_empty: bool,
    },
}

/// `condition Foo: <expr>` inside a type (grammar rule `Condition`).
#[derive(Debug, Clone)]
pub struct ConditionDef {
    pub name: Option<String>,
    pub definition: Option<String>,
    pub doc_references: Vec<DocReference>,
    pub annotations: Vec<AnnotationRef>,
    pub expression: SpannedExpr,
    pub span: Span,
}

// ---- functions, rules, reports ---------------------------------------------

/// `func Foo:` (grammar rule `Function`); the dispatch form
/// `func Foo(attr: Enum->VALUE)` parses into `dispatch`.
#[derive(Debug, Clone)]
pub struct FunctionDef {
    pub name: String,
    pub dispatch: Option<FunctionDispatchDef>,
    pub super_function: Option<SpannedQName>,
    pub definition: Option<String>,
    pub transform: Vec<TransformAnnotationDef>,
    pub doc_references: Vec<DocReference>,
    pub annotations: Vec<AnnotationRef>,
    pub inputs: Vec<AttributeDef>,
    pub output: Option<AttributeDef>,
    pub shortcuts: Vec<ShortcutDef>,
    pub conditions: Vec<ConditionDef>,
    pub operations: Vec<OperationDef>,
    pub post_conditions: Vec<ConditionDef>,
    pub span: Span,
}

/// The `(attr: Enum->VALUE)` part of a dispatch function. `attr` must name
/// an input attribute; `Enum->VALUE` selects the dispatched case.
#[derive(Debug, Clone)]
pub struct FunctionDispatchDef {
    pub attribute: String,
    pub attribute_span: Span,
    pub enumeration: QName,
    pub enumeration_span: Span,
    pub value: String,
    pub value_span: Span,
}

/// `[ingest X]` / `[enrich]` / `[projection Y]` — distinguished from an
/// ordinary annotation reference by the leading transform keyword.
#[derive(Debug, Clone)]
pub struct TransformAnnotationDef {
    pub kind: TransformKind,
    pub reference: Option<SpannedQName>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// `alias name: expr` (grammar rule `ShortcutDeclaration`).
#[derive(Debug, Clone)]
pub struct ShortcutDef {
    pub name: String,
    pub definition: Option<String>,
    pub expression: SpannedExpr,
    pub span: Span,
}

/// `set|add root (-> feature)*: expr` (grammar rule `Operation`).
#[derive(Debug, Clone)]
pub struct OperationDef {
    pub add: bool,
    pub assign_root: String,
    pub assign_root_span: Span,
    pub path: Vec<PathSegmentDef>,
    pub definition: Option<String>,
    pub expression: SpannedExpr,
    pub span: Span,
}

/// One `-> feature` step of an operation path.
#[derive(Debug, Clone)]
pub struct PathSegmentDef {
    pub feature: String,
    pub span: Span,
}

/// `reporting rule Foo from T:` / `eligibility rule Foo:` (grammar rule
/// `RosettaRule`).
#[derive(Debug, Clone)]
pub struct RuleDef {
    pub name: String,
    pub eligibility: bool,
    pub input: Option<TypeCall>,
    pub definition: Option<String>,
    pub doc_references: Vec<DocReference>,
    pub expression: SpannedExpr,
    pub span: Span,
}

/// `report Body Corpus ... in T+1 from T when R with type T` (grammar rule
/// `RosettaReport`).
#[derive(Debug, Clone)]
pub struct ReportDef {
    pub body: SpannedQName,
    pub corpora: Vec<SpannedQName>,
    pub segments: Vec<ReportSegmentRef>,
    pub timing: ReportTiming,
    pub input_type: TypeCall,
    pub eligibility_rules: Vec<SpannedQName>,
    pub report_type: SpannedQName,
    pub rule_source: Option<SpannedQName>,
    pub span: Span,
}

/// `(Segment "ref")` inside a report's regulatory reference.
#[derive(Debug, Clone)]
pub struct ReportSegmentRef {
    pub segment: SpannedQName,
    pub reference: String,
}

/// The `in ...` timing keyword of a report. Present in the grammar only —
/// the Ecore model does not store it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportTiming {
    RealTime,
    T(u32),
    Asatp,
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

/// `rule source Foo extends Bar { Data: + attr [ruleReference R] }`.
#[derive(Debug, Clone)]
pub struct RuleSourceDef {
    pub name: String,
    pub super_source: Option<SpannedQName>,
    pub classes: Vec<ExternalClassDef>,
    pub span: Span,
}

/// One `Data:` class inside a rule source.
#[derive(Debug, Clone)]
pub struct ExternalClassDef {
    pub data: SpannedQName,
    pub attributes: Vec<ExternalAttributeDef>,
    pub span: Span,
}

/// `(+|-) attr [ruleReference ...]*` inside a rule-source class.
#[derive(Debug, Clone)]
pub struct ExternalAttributeDef {
    pub add: bool,
    pub attribute: String,
    pub attribute_span: Span,
    pub rule_references: Vec<RuleReference>,
    pub span: Span,
}

/// `schema Foo JSON` (grammar rule `Schema`).
#[derive(Debug, Clone)]
pub struct SchemaDef {
    pub name: String,
    pub format: String,
    pub format_span: Span,
    pub definition: Option<String>,
    pub annotations: Vec<AnnotationRef>,
    pub span: Span,
}

/// `body Type Name <"...">` (grammar rule `RosettaBody`).
#[derive(Debug, Clone)]
pub struct BodyDef {
    pub name: String,
    pub body_type: String,
    pub definition: Option<String>,
    pub span: Span,
}

/// `corpus Type (Body)? ("display")? Name <"...">` (grammar rule
/// `RosettaCorpus`).
#[derive(Debug, Clone)]
pub struct CorpusDef {
    pub name: String,
    pub corpus_type: String,
    pub display_name: Option<String>,
    pub body: Option<SpannedQName>,
    pub definition: Option<String>,
    pub span: Span,
}

/// `segment Name` (grammar rule `RosettaSegment`).
#[derive(Debug, Clone)]
pub struct SegmentDecl {
    pub name: String,
    pub span: Span,
}

/// `metaType Name Type` (grammar rule `RosettaMetaType`).
#[derive(Debug, Clone)]
pub struct MetaTypeDecl {
    pub name: String,
    pub type_call: TypeCall,
    pub span: Span,
}
