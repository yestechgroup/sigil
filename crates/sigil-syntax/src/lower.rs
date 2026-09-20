//! Lowering: convert the parser AST into the parser-independent semantic
//! model in `sigil-model`.

use crate::ast::{
    AnnotationDecl as AstAnnotation, AttributeDef, DataDef, DocReference as AstDocReference,
    Element, EnumDef, QualifiableType as AstQualifiableType, TypeAliasDef, TypeCall,
    TypeCallArgumentValue,
};
use sigil_model as m;

/// Lower one parsed file into a `ModelFile`.
pub fn lower(file_name: &str, unit: &crate::ast::SourceUnit) -> m::ModelFile {
    m::ModelFile {
        name: file_name.to_string(),
        namespace: unit.namespace.name.0.clone(),
        overridden: unit.namespace.overridden,
        scope: unit.scope.as_ref().map(|(name, definition)| m::Scope {
            name: name.clone(),
            definition: definition.clone(),
        }),
        // The Ecore model defaults `version` to "0.0.0".
        version: Some(unit.version.clone().unwrap_or_else(|| "0.0.0".to_string())),
        imports: unit
            .imports
            .iter()
            .map(|i| {
                let mut ns = i.namespace.0.clone();
                if i.wildcard {
                    ns.push_str(".*");
                }
                m::ImportEntry {
                    imported_namespace: ns,
                    wildcard: i.wildcard,
                    namespace_alias: i.alias.clone(),
                }
            })
            .collect(),
        configurations: unit
            .configurations
            .iter()
            .map(|c| m::Configuration {
                q_type: match c.q_type {
                    AstQualifiableType::Event => m::QualifiableType::Event,
                    AstQualifiableType::Product => m::QualifiableType::Product,
                },
                root: m::TypeRef::unresolved(c.root.0.clone()),
            })
            .collect(),
        elements: unit.elements.iter().filter_map(lower_element).collect(),
    }
}

fn lower_element(element: &Element) -> Option<m::SemanticElement> {
    Some(match element {
        Element::Data(d) => m::SemanticElement::Data(lower_data(d, false)),
        Element::Choice(d) => m::SemanticElement::Data(lower_data(d, true)),
        Element::Enumeration(e) => m::SemanticElement::Enumeration(lower_enum(e)),
        Element::Annotation(a) => m::SemanticElement::Annotation(lower_annotation(a)),
        Element::TypeAlias(t) => m::SemanticElement::TypeAlias(lower_type_alias(t)),
        Element::BasicType(b) => m::SemanticElement::BasicType(m::BasicType {
            name: b.name.clone(),
            parameters: b.parameters.iter().map(lower_parameter).collect(),
            definition: b.definition.clone(),
        }),
        Element::RecordType(r) => m::SemanticElement::RecordType(m::RecordType {
            name: r.name.clone(),
            definition: r.definition.clone(),
            features: r
                .features
                .iter()
                .map(|f| m::RecordFeature {
                    name: f.name.clone(),
                    type_ref: lower_type_call(&f.type_call),
                })
                .collect(),
        }),
        Element::LibraryFunction(f) => m::SemanticElement::LibraryFunction(m::LibraryFunction {
            name: f.name.clone(),
            parameters: f
                .parameters
                .iter()
                .map(|(name, type_call, is_array)| m::LibraryParameter {
                    name: name.clone(),
                    type_ref: lower_type_call(type_call),
                    is_array: *is_array,
                })
                .collect(),
            return_type: lower_type_call(&f.return_type),
            definition: f.definition.clone(),
        }),
        Element::Unsupported(_) => return None,
    })
}

fn lower_data(d: &DataDef, is_choice: bool) -> m::Data {
    m::Data {
        name: d.name.clone(),
        is_choice,
        definition: d.definition.clone(),
        super_type: d
            .super_type
            .as_ref()
            .map(|s| m::TypeRef::unresolved(s.0.clone())),
        annotations: lower_annotations(&d.annotations),
        doc_references: lower_docs(&d.doc_references),
        attributes: d.attributes.iter().map(lower_attribute).collect(),
    }
}

fn lower_enum(e: &EnumDef) -> m::Enumeration {
    m::Enumeration {
        name: e.name.clone(),
        definition: e.definition.clone(),
        super_type: e
            .super_type
            .as_ref()
            .map(|s| m::TypeRef::unresolved(s.0.clone())),
        annotations: lower_annotations(&e.annotations),
        doc_references: lower_docs(&e.doc_references),
        values: e
            .values
            .iter()
            .map(|v| m::EnumValue {
                name: v.name.clone(),
                display: v.display.clone(),
                definition: v.definition.clone(),
                annotations: lower_annotations(&v.annotations),
                doc_references: lower_docs(&v.doc_references),
            })
            .collect(),
    }
}

fn lower_annotation(a: &AstAnnotation) -> m::Annotation {
    m::Annotation {
        name: a.name.clone(),
        definition: a.definition.clone(),
        prefix: a.prefix.clone(),
        attributes: a.attributes.iter().map(lower_attribute).collect(),
    }
}

fn lower_type_alias(t: &TypeAliasDef) -> m::TypeAlias {
    m::TypeAlias {
        name: t.name.clone(),
        parameters: t.parameters.iter().map(lower_parameter).collect(),
        definition: t.definition.clone(),
        type_ref: lower_type_call(&t.type_call),
        annotations: lower_annotations(&t.annotations),
    }
}

fn lower_attribute(a: &AttributeDef) -> m::Attribute {
    m::Attribute {
        name: a.name.clone(),
        is_override: a.is_override,
        type_ref: lower_type_call(&a.type_call),
        cardinality: m::Cardinality {
            min: a.cardinality.min,
            max: match a.cardinality.max {
                crate::ast::CardinalityMax::Finite(max) => m::CardinalityMax::Finite(max),
                crate::ast::CardinalityMax::Unbounded => m::CardinalityMax::Unbounded,
            },
        },
        definition: a.definition.clone(),
        annotations: lower_annotations(&a.annotations),
        labels: a
            .labels
            .iter()
            .map(|l| m::LabelAnnotation {
                label: l.label.clone(),
            })
            .collect(),
        rule_references: a
            .rule_references
            .iter()
            .map(|r| m::RuleReference {
                rule: r.rule.as_ref().map(|q| q.0.clone()),
                empty: r.empty,
            })
            .collect(),
        doc_references: lower_docs(&a.doc_references),
    }
}

fn lower_parameter(p: &crate::ast::TypeParameter) -> m::TypeParameter {
    m::TypeParameter {
        name: p.name.clone(),
        type_ref: lower_type_call(&p.type_call),
        definition: p.definition.clone(),
    }
}

fn lower_type_call(tc: &TypeCall) -> m::TypeRef {
    m::TypeRef {
        name: tc.name.0.clone(),
        arguments: tc
            .arguments
            .iter()
            .map(|a| m::TypeCallArgument {
                parameter: a.parameter.clone(),
                argument_value: match &a.value {
                    TypeCallArgumentValue::Reference(r) => m::ArgumentValue::Reference(r.clone()),
                    TypeCallArgumentValue::Int(i) => m::ArgumentValue::Int(*i),
                    TypeCallArgumentValue::Number(n) => m::ArgumentValue::Number(n.clone()),
                    TypeCallArgumentValue::Str(s) => m::ArgumentValue::Str(s.clone()),
                    TypeCallArgumentValue::Bool(b) => m::ArgumentValue::Bool(*b),
                },
            })
            .collect(),
        resolved: None,
    }
}

fn lower_annotations(annos: &[crate::ast::AnnotationRef]) -> Vec<m::AnnotationRef> {
    annos
        .iter()
        .map(|a| m::AnnotationRef {
            annotation: a.annotation.0.clone(),
            attribute: a.attribute.clone(),
            qualifiers: a
                .qualifiers
                .iter()
                .map(|q| m::AnnotationQualifier {
                    name: q.name.clone(),
                    qualifier_value: match &q.value {
                        crate::ast::QualifierValue::Str(s) => m::QualifierValue::Str(s.clone()),
                        crate::ast::QualifierValue::Path(p) => m::QualifierValue::Path {
                            data: p.data.0.clone(),
                            attributes: p.attributes.clone(),
                        },
                    },
                })
                .collect(),
            annotation_resolved: None,
        })
        .collect()
}

fn lower_docs(docs: &[AstDocReference]) -> Vec<m::DocReference> {
    docs.iter()
        .map(|d| m::DocReference {
            body: d.body.0.clone(),
            corpora: d.corpora.iter().map(|c| c.0.clone()).collect(),
            segments: d.segments.clone(),
            rationales: d
                .rationales
                .iter()
                .map(|r| m::Rationale {
                    rationale: r.rationale.clone(),
                    rationale_author: r.rationale_author.clone(),
                })
                .collect(),
            structured_provision: d.structured_provision.clone(),
            provision: d.provision.clone(),
            reported_field: d.reported_field,
        })
        .collect()
}
