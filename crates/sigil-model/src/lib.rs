//! The semantic model: sigil's native Rust representation of Rune models.
//!
//! This mirrors the Rune Ecore metamodel (`com.regnosys.rosetta.rosetta`,
//! `...rosetta.simple`) rather than the surface grammar. It knows nothing
//! about chumsky or the parser: `sigil-syntax` ASTs are *lowered* into these
//! types, and this crate is the compatibility boundary for the rest of the
//! toolchain (resolution, validation, serialization, codegen).

use serde::Serialize;

/// Reference to a type by (possibly qualified) name, resolved later.
#[derive(Debug, Clone, Default, Serialize)]
pub struct TypeRef {
    pub name: String,
    pub arguments: Vec<TypeCallArgument>,
    /// Set during resolution: the element this reference points at.
    #[serde(skip_serializing)]
    pub resolved: Option<usize>,
}

impl TypeRef {
    pub fn unresolved(name: impl Into<String>) -> Self {
        TypeRef {
            name: name.into(),
            arguments: Vec::new(),
            resolved: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TypeCallArgument {
    pub parameter: String,
    #[serde(rename = "value")]
    pub argument_value: ArgumentValue,
}

#[derive(Debug, Clone, Serialize)]
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

#[derive(Debug, Clone, Default, Serialize)]
pub struct AnnotationRef {
    pub annotation: String,
    pub attribute: Option<String>,
    pub qualifiers: Vec<AnnotationQualifier>,
    /// Set during resolution: the annotation declaration this ref points at.
    #[serde(skip_serializing)]
    pub annotation_resolved: Option<usize>,
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
}

#[derive(Debug, Clone, Serialize)]
pub struct EnumValue {
    pub name: String,
    pub display: Option<String>,
    pub definition: Option<String>,
    pub annotations: Vec<AnnotationRef>,
    pub doc_references: Vec<DocReference>,
}

/// `simple.Annotation` (a declaration)
#[derive(Debug, Clone, Serialize)]
pub struct Annotation {
    pub name: String,
    pub definition: Option<String>,
    pub prefix: Option<String>,
    pub attributes: Vec<Attribute>,
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
}

/// `RosettaRecordType`
#[derive(Debug, Clone, Serialize)]
pub struct RecordType {
    pub name: String,
    pub definition: Option<String>,
    pub features: Vec<RecordFeature>,
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
}

#[derive(Debug, Clone, Serialize)]
pub struct LibraryParameter {
    pub name: String,
    #[serde(rename = "type")]
    pub type_ref: TypeRef,
    pub is_array: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind")]
pub enum SemanticElement {
    Data(Data),
    Enumeration(Enumeration),
    Annotation(Annotation),
    TypeAlias(TypeAlias),
    BasicType(BasicType),
    RecordType(RecordType),
    LibraryFunction(LibraryFunction),
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
        }
    }

    /// Whether this element can be the target of a type reference.
    pub fn is_type(&self) -> bool {
        !matches!(
            self,
            SemanticElement::Annotation(_) | SemanticElement::LibraryFunction(_)
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
