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
    pub root: QName,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Element {
    Data(DataDef),
    Choice(DataDef),
    Enumeration(EnumDef),
    Annotation(AnnotationDecl),
    TypeAlias(TypeAliasDef),
    BasicType(BasicTypeDecl),
    RecordType(RecordTypeDecl),
    LibraryFunction(LibraryFunctionDecl),
    /// A construct recognised but not yet modelled (func, rule, report, ...).
    /// It carries a diagnostic; the element is skipped.
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
            Element::Unsupported(_) => None,
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
    pub super_type: Option<QName>,
    pub definition: Option<String>,
    pub doc_references: Vec<DocReference>,
    pub annotations: Vec<AnnotationRef>,
    pub attributes: Vec<AttributeDef>,
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
    pub super_type: Option<QName>,
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
