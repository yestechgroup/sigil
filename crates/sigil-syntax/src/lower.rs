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
                root: m::TypeRef::unresolved_spanned(c.root.name.0.clone(), c.root.span),
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
            span: b.span,
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
            span: r.span,
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
            span: f.span,
        }),
        Element::Function(f) => m::SemanticElement::Function(m::Function {
            name: f.name.clone(),
            definition: f.definition.clone(),
            super_function: f
                .super_function
                .as_ref()
                .map(|s| m::TypeRef::unresolved_spanned(s.name.0.clone(), s.span)),
            transform: f
                .transform
                .iter()
                .map(|t| m::TransformAnnotation {
                    kind: match t.kind {
                        crate::ast::TransformKind::Ingest => m::TransformKind::Ingest,
                        crate::ast::TransformKind::Enrich => m::TransformKind::Enrich,
                        crate::ast::TransformKind::Projection => m::TransformKind::Projection,
                    },
                    reference: t.reference.as_ref().map(|r| r.name.0.clone()),
                    reference_span: t.reference.as_ref().map(|r| r.span).unwrap_or_default(),
                })
                .collect(),
            dispatch: f.dispatch.as_ref().map(|d| m::FunctionDispatch {
                attribute: d.attribute.clone(),
                attribute_span: d.attribute_span,
                enumeration: d.enumeration.0.clone(),
                enumeration_span: d.enumeration_span,
                value: d.value.clone(),
                value_span: d.value_span,
            }),
            annotations: lower_annotations(&f.annotations),
            doc_references: lower_docs(&f.doc_references),
            inputs: f.inputs.iter().map(lower_attribute).collect(),
            output: f.output.as_ref().map(lower_attribute),
            shortcuts: f
                .shortcuts
                .iter()
                .map(|s| m::Shortcut {
                    name: s.name.clone(),
                    definition: s.definition.clone(),
                    expression: lower_expression(&s.expression),
                    span: s.span,
                })
                .collect(),
            conditions: f.conditions.iter().map(lower_condition).collect(),
            operations: f.operations.iter().map(lower_operation).collect(),
            post_conditions: f.post_conditions.iter().map(lower_condition).collect(),
            span: f.span,
        }),
        Element::Rule(r) => m::SemanticElement::Rule(m::Rule {
            name: r.name.clone(),
            definition: r.definition.clone(),
            eligibility: r.eligibility,
            input: r.input.as_ref().map(lower_type_call),
            doc_references: lower_docs(&r.doc_references),
            expression: lower_expression(&r.expression),
            span: r.span,
        }),
        Element::Report(r) => m::SemanticElement::Report(m::Report {
            regulatory: m::RegulatoryRef {
                body: lower_named_ref(r.body.name.0.clone(), r.body.span),
                corpora: r
                    .corpora
                    .iter()
                    .map(|c| lower_named_ref(c.name.0.clone(), c.span))
                    .collect(),
                segments: r
                    .segments
                    .iter()
                    .map(|s| m::SegmentReference {
                        segment: lower_named_ref(s.segment.name.0.clone(), s.segment.span),
                        reference: s.reference.clone(),
                    })
                    .collect(),
            },
            timing: match r.timing {
                crate::ast::ReportTiming::RealTime => m::ReportTiming::RealTime,
                crate::ast::ReportTiming::T(n) => m::ReportTiming::T(n),
                crate::ast::ReportTiming::Asatp => m::ReportTiming::Asatp,
            },
            input_type: lower_type_call(&r.input_type),
            eligibility_rules: r
                .eligibility_rules
                .iter()
                .map(|e| lower_named_ref(e.name.0.clone(), e.span))
                .collect(),
            report_type: lower_named_ref(r.report_type.name.0.clone(), r.report_type.span),
            rule_source: r
                .rule_source
                .as_ref()
                .map(|s| lower_named_ref(s.name.0.clone(), s.span)),
            span: r.span,
        }),
        Element::RuleSource(s) => m::SemanticElement::ExternalRuleSource(m::ExternalRuleSource {
            name: s.name.clone(),
            super_source: s
                .super_source
                .as_ref()
                .map(|s| m::TypeRef::unresolved_spanned(s.name.0.clone(), s.span)),
            classes: s
                .classes
                .iter()
                .map(|c| m::ExternalClass {
                    data: m::TypeRef::unresolved_spanned(c.data.name.0.clone(), c.data.span),
                    attributes: c
                        .attributes
                        .iter()
                        .map(|a| m::ExternalAttribute {
                            add: a.add,
                            attribute: a.attribute.clone(),
                            attribute_span: a.attribute_span,
                            rule_references: lower_rule_refs(&a.rule_references),
                            resolved: false,
                            span: a.span,
                        })
                        .collect(),
                    span: c.span,
                })
                .collect(),
            span: s.span,
        }),
        Element::Schema(s) => m::SemanticElement::Schema(m::Schema {
            name: s.name.clone(),
            format: s.format.clone(),
            format_span: s.format_span,
            definition: s.definition.clone(),
            annotations: lower_annotations(&s.annotations),
            span: s.span,
        }),
        Element::Body(b) => m::SemanticElement::Body(m::Body {
            name: b.name.clone(),
            body_type: b.body_type.clone(),
            definition: b.definition.clone(),
            span: b.span,
        }),
        Element::Corpus(c) => m::SemanticElement::Corpus(m::Corpus {
            name: c.name.clone(),
            corpus_type: c.corpus_type.clone(),
            display_name: c.display_name.clone(),
            body: c.body.as_ref().map(|b| b.name.0.clone()),
            definition: c.definition.clone(),
            span: c.span,
        }),
        Element::Segment(s) => m::SemanticElement::Segment(m::Segment {
            name: s.name.clone(),
            span: s.span,
        }),
        Element::MetaType(met) => m::SemanticElement::MetaType(m::MetaType {
            name: met.name.clone(),
            type_ref: lower_type_call(&met.type_call),
            span: met.span,
        }),
        Element::Unsupported(_) => return None,
    })
}

fn lower_named_ref(name: String, span: sigil_diag::Span) -> m::NamedRef {
    m::NamedRef {
        name,
        resolved: None,
        span,
    }
}

fn lower_rule_refs(rules: &[crate::ast::RuleReference]) -> Vec<m::RuleReference> {
    rules
        .iter()
        .map(|r| m::RuleReference {
            rule: r.rule.as_ref().map(|q| q.0.clone()),
            empty: r.empty,
            resolved: None,
        })
        .collect()
}

fn lower_operation(o: &crate::ast::OperationDef) -> m::Operation {
    m::Operation {
        definition: o.definition.clone(),
        add: o.add,
        assign_root: o.assign_root.clone(),
        assign_root_span: o.assign_root_span,
        path: o
            .path
            .iter()
            .map(|p| m::PathSegment {
                feature: p.feature.clone(),
                resolved: false,
                span: p.span,
            })
            .collect(),
        expression: lower_expression(&o.expression),
        span: o.span,
    }
}

fn lower_data(d: &DataDef, is_choice: bool) -> m::Data {
    m::Data {
        name: d.name.clone(),
        is_choice,
        definition: d.definition.clone(),
        super_type: d
            .super_type
            .as_ref()
            .map(|s| m::TypeRef::unresolved_spanned(s.name.0.clone(), s.span)),
        annotations: lower_annotations(&d.annotations),
        doc_references: lower_docs(&d.doc_references),
        attributes: d.attributes.iter().map(lower_attribute).collect(),
        conditions: d.conditions.iter().map(lower_condition).collect(),
        span: d.span,
    }
}

fn lower_enum(e: &EnumDef) -> m::Enumeration {
    m::Enumeration {
        name: e.name.clone(),
        definition: e.definition.clone(),
        super_type: e
            .super_type
            .as_ref()
            .map(|s| m::TypeRef::unresolved_spanned(s.name.0.clone(), s.span)),
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
                span: v.span,
            })
            .collect(),
        span: e.span,
    }
}

fn lower_annotation(a: &AstAnnotation) -> m::Annotation {
    m::Annotation {
        name: a.name.clone(),
        definition: a.definition.clone(),
        prefix: a.prefix.clone(),
        attributes: a.attributes.iter().map(lower_attribute).collect(),
        span: a.span,
    }
}

fn lower_type_alias(t: &TypeAliasDef) -> m::TypeAlias {
    m::TypeAlias {
        name: t.name.clone(),
        parameters: t.parameters.iter().map(lower_parameter).collect(),
        definition: t.definition.clone(),
        type_ref: lower_type_call(&t.type_call),
        annotations: lower_annotations(&t.annotations),
        span: t.span,
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
        rule_references: lower_rule_refs(&a.rule_references),
        doc_references: lower_docs(&a.doc_references),
        span: a.span,
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
        span: tc.span,
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
            span: a.span,
        })
        .collect()
}

fn lower_docs(docs: &[AstDocReference]) -> Vec<m::DocReference> {
    docs.iter()
        .map(|d| m::DocReference {
            for_path: d.for_path.as_ref().map(lower_path),
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

fn lower_path(path: &crate::ast::AnnotationPath) -> Vec<String> {
    let mut steps = Vec::new();
    if path.starts_with_item {
        steps.push("item".to_string());
    }
    for segment in &path.segments {
        if segment.deep {
            steps.push(format!("->>{}", segment.attribute));
        } else {
            steps.push(segment.attribute.clone());
        }
    }
    steps
}

// ---- expressions -----------------------------------------------------------

/// Lower a parsed expression into the parser-independent IR.
pub fn lower_expression(e: &crate::ast::SpannedExpr) -> m::expr::Expr {
    use crate::ast::{
        AstArithOp, AstCmpOp, AstEqOp, AstExistsMod, AstFunctionalOp, AstLogicOp, AstNecessity,
        ExprKind,
    };
    use m::expr as x;

    match &e.kind {
        ExprKind::Boolean(value) => x::Expr::BooleanLiteral { value: *value },
        ExprKind::Str(value) => x::Expr::StringLiteral {
            value: value.clone(),
        },
        ExprKind::Number(text) => x::Expr::NumberLiteral { text: text.clone() },
        ExprKind::Int(text) => x::Expr::IntLiteral { text: text.clone() },
        ExprKind::Empty => x::Expr::empty_list(),
        ExprKind::List(elements) => x::Expr::ListLiteral {
            elements: elements.iter().map(lower_expression).collect(),
        },
        ExprKind::Symbol {
            name,
            explicit_args,
            args,
        } => x::Expr::SymbolReference {
            symbol: name.0.clone(),
            explicit_arguments: *explicit_args,
            raw_args: args.iter().map(lower_expression).collect(),
        },
        ExprKind::Item => x::Expr::ImplicitVariable,
        ExprKind::FeatureCall {
            receiver,
            feature,
            deep,
        } => {
            let receiver = Box::new(lower_expression(receiver));
            if *deep {
                x::Expr::DeepFeatureCall {
                    receiver,
                    feature: feature.clone(),
                }
            } else {
                x::Expr::FeatureCall {
                    receiver,
                    feature: feature.clone(),
                }
            }
        }
        ExprKind::Arithmetic { op, left, right } => x::Expr::ArithmeticOperation {
            operator: match op {
                AstArithOp::Add => x::ArithmeticOperator::Add,
                AstArithOp::Subtract => x::ArithmeticOperator::Subtract,
                AstArithOp::Multiply => x::ArithmeticOperator::Multiply,
                AstArithOp::Divide => x::ArithmeticOperator::Divide,
            },
            left: Box::new(lower_expression(left)),
            right: Box::new(lower_expression(right)),
        },
        ExprKind::Logical { op, left, right } => x::Expr::LogicalOperation {
            operator: match op {
                AstLogicOp::And => x::LogicalOperator::And,
                AstLogicOp::Or => x::LogicalOperator::Or,
            },
            left: Box::new(lower_expression(left)),
            right: Box::new(lower_expression(right)),
        },
        ExprKind::Equality {
            op,
            card_mod,
            left,
            right,
        } => x::Expr::EqualityOperation {
            operator: match op {
                AstEqOp::Eq => x::EqualityOperator::Eq,
                AstEqOp::NotEq => x::EqualityOperator::NotEq,
            },
            card_mod: lower_card_mod(*card_mod),
            left: Box::new(lower_expression(left)),
            right: Box::new(lower_expression(right)),
        },
        ExprKind::Comparison {
            op,
            card_mod,
            left,
            right,
        } => x::Expr::ComparisonOperation {
            operator: match op {
                AstCmpOp::Ge => x::ComparisonOperator::Ge,
                AstCmpOp::Le => x::ComparisonOperator::Le,
                AstCmpOp::Gt => x::ComparisonOperator::Gt,
                AstCmpOp::Lt => x::ComparisonOperator::Lt,
            },
            card_mod: lower_card_mod(*card_mod),
            left: Box::new(lower_expression(left)),
            right: Box::new(lower_expression(right)),
        },
        ExprKind::Contains { left, right } => x::Expr::ContainsExpression {
            left: Box::new(lower_expression(left)),
            right: Box::new(lower_expression(right)),
        },
        ExprKind::Disjoint { left, right } => x::Expr::DisjointExpression {
            left: Box::new(lower_expression(left)),
            right: Box::new(lower_expression(right)),
        },
        ExprKind::Default { left, right } => x::Expr::DefaultOperation {
            left: Box::new(lower_expression(left)),
            right: Box::new(lower_expression(right)),
        },
        ExprKind::Join { left, right } => x::Expr::JoinOperation {
            left: Box::new(lower_expression(left)),
            // A separator-less join keeps the generated `""` literal the
            // EMF derived state inserts.
            right: Box::new(
                right
                    .as_ref()
                    .map(|r| lower_expression(r))
                    .unwrap_or_else(x::Expr::generated_empty_separator),
            ),
            explicit_separator: right.is_some(),
        },
        ExprKind::Conditional {
            if_,
            ifthen,
            elsethen,
        } => x::Expr::ConditionalExpression {
            if_: Box::new(lower_expression(if_)),
            ifthen: Box::new(lower_expression(ifthen)),
            // A missing `else` is the generated empty list literal with
            // `full == false`, as in the EMF model.
            elsethen: Box::new(
                elsethen
                    .as_ref()
                    .map(|e| lower_expression(e))
                    .unwrap_or_else(x::Expr::empty_list),
            ),
            full: elsethen.is_some(),
        },
        ExprKind::OnlyExists {
            args,
            has_parentheses,
        } => x::Expr::OnlyExistsExpression {
            args: args.iter().map(lower_expression).collect(),
            has_parentheses: *has_parentheses,
        },
        ExprKind::Exists { modifier, argument } => x::Expr::ExistsExpression {
            modifier: match modifier {
                None => x::ExistsModifier::None,
                Some(AstExistsMod::Single) => x::ExistsModifier::Single,
                Some(AstExistsMod::Multiple) => x::ExistsModifier::Multiple,
            },
            argument: Box::new(lower_expression(argument)),
        },
        ExprKind::Absent { argument } => x::Expr::AbsentExpression {
            argument: Box::new(lower_expression(argument)),
        },
        ExprKind::OnlyElement { argument } => x::Expr::OnlyElement {
            argument: Box::new(lower_expression(argument)),
        },
        ExprKind::Count { argument } => x::Expr::CountOperation {
            argument: Box::new(lower_expression(argument)),
        },
        ExprKind::Flatten { argument } => x::Expr::FlattenOperation {
            argument: Box::new(lower_expression(argument)),
        },
        ExprKind::Distinct { argument } => x::Expr::DistinctOperation {
            argument: Box::new(lower_expression(argument)),
        },
        ExprKind::Reverse { argument } => x::Expr::ReverseOperation {
            argument: Box::new(lower_expression(argument)),
        },
        ExprKind::First { argument } => x::Expr::FirstOperation {
            argument: Box::new(lower_expression(argument)),
        },
        ExprKind::Last { argument } => x::Expr::LastOperation {
            argument: Box::new(lower_expression(argument)),
        },
        ExprKind::Sum { argument } => x::Expr::SumOperation {
            argument: Box::new(lower_expression(argument)),
        },
        ExprKind::AsKey { argument } => x::Expr::AsKeyOperation {
            argument: Box::new(lower_expression(argument)),
        },
        ExprKind::OneOf { argument } => x::Expr::OneOfOperation {
            argument: Box::new(lower_expression(argument)),
        },
        ExprKind::Choice {
            necessity,
            attributes,
            argument,
        } => x::Expr::ChoiceOperation {
            necessity: match necessity {
                AstNecessity::Optional => x::Necessity::Optional,
                AstNecessity::Required => x::Necessity::Required,
            },
            attributes: attributes.clone(),
            argument: Box::new(lower_expression(argument)),
        },
        ExprKind::ToString { argument } => x::Expr::ToStringOperation {
            argument: Box::new(lower_expression(argument)),
        },
        ExprKind::ToNumber { argument } => x::Expr::ToNumberOperation {
            argument: Box::new(lower_expression(argument)),
        },
        ExprKind::ToInt { argument } => x::Expr::ToIntOperation {
            argument: Box::new(lower_expression(argument)),
        },
        ExprKind::ToTime { argument } => x::Expr::ToTimeOperation {
            argument: Box::new(lower_expression(argument)),
        },
        ExprKind::ToEnum {
            enumeration,
            argument,
        } => x::Expr::ToEnumOperation {
            enumeration: enumeration.0.clone(),
            argument: Box::new(lower_expression(argument)),
        },
        ExprKind::ToDate { argument } => x::Expr::ToDateOperation {
            argument: Box::new(lower_expression(argument)),
        },
        ExprKind::ToDateTime { argument } => x::Expr::ToDateTimeOperation {
            argument: Box::new(lower_expression(argument)),
        },
        ExprKind::ToZonedDateTime { argument } => x::Expr::ToZonedDateTimeOperation {
            argument: Box::new(lower_expression(argument)),
        },
        ExprKind::Switch { argument, cases } => x::Expr::SwitchOperation {
            argument: Box::new(lower_expression(argument)),
            cases: cases
                .iter()
                .map(|case| x::SwitchCase {
                    guard: case.guard.as_ref().map(|guard| match guard {
                        crate::ast::AstSwitchGuard::Literal(expr) => {
                            x::SwitchGuard::Literal(Box::new(lower_expression(expr)))
                        }
                        crate::ast::AstSwitchGuard::Reference(name) => {
                            x::SwitchGuard::Reference(name.0.clone())
                        }
                    }),
                    expression: Box::new(lower_expression(&case.expression)),
                })
                .collect(),
        },
        ExprKind::WithMeta { argument, entries } => x::Expr::WithMetaOperation {
            argument: Box::new(lower_expression(argument)),
            entries: entries
                .iter()
                .map(|(key, value)| x::WithMetaEntry {
                    key: key.clone(),
                    value: Box::new(lower_expression(value)),
                })
                .collect(),
        },
        ExprKind::As { type_, argument } => x::Expr::AsOperation {
            type_: type_.0.clone(),
            argument: Box::new(lower_expression(argument)),
        },
        ExprKind::Functional {
            op,
            argument,
            function,
        } => {
            let argument = Box::new(lower_expression(argument));
            let function = function.as_ref().map(|f| x::InlineFunction {
                parameters: f.parameters.clone(),
                body: Box::new(lower_expression(&f.body)),
            });
            match op {
                AstFunctionalOp::Then => x::Expr::ThenOperation { argument, function },
                AstFunctionalOp::Filter => x::Expr::FilterOperation { argument, function },
                AstFunctionalOp::Extract => x::Expr::MapOperation { argument, function },
                AstFunctionalOp::Reduce => x::Expr::ReduceOperation { argument, function },
                AstFunctionalOp::Sort => x::Expr::SortOperation { argument, function },
                AstFunctionalOp::Min => x::Expr::MinOperation { argument, function },
                AstFunctionalOp::Max => x::Expr::MaxOperation { argument, function },
            }
        }
        ExprKind::Constructor {
            type_call,
            values,
            implicit_empty,
        } => x::Expr::ConstructorExpression {
            type_call: lower_type_call(type_call),
            values: values
                .iter()
                .map(|(key, value)| x::ConstructorPair {
                    key: key.clone(),
                    value: Box::new(lower_expression(value)),
                })
                .collect(),
            implicit_empty: *implicit_empty,
        },
    }
}

fn lower_card_mod(mod_: Option<crate::ast::AstCardinalityMod>) -> m::expr::CardinalityModifier {
    match mod_ {
        None => m::expr::CardinalityModifier::None,
        Some(crate::ast::AstCardinalityMod::Any) => m::expr::CardinalityModifier::Any,
        Some(crate::ast::AstCardinalityMod::All) => m::expr::CardinalityModifier::All,
    }
}

fn lower_condition(c: &crate::ast::ConditionDef) -> m::Condition {
    m::Condition {
        name: c.name.clone(),
        definition: c.definition.clone(),
        expression: lower_expression(&c.expression),
        annotations: lower_annotations(&c.annotations),
        doc_references: lower_docs(&c.doc_references),
        span: c.span,
    }
}

#[cfg(test)]
mod tests {
    use sigil_diag::SourceFile;

    use crate::parser::parse;

    #[test]
    fn doc_reference_for_path_is_lowered() {
        let src = "namespace t\n\
                   body CFTC CFTCBody <\"b\">\n\
                   corpus CFTC Part45 <\"p\">\n\
                   segment S1\n\
                   type Product:\n\
                   \tproductId string (1..1)\n\
                   \t\t[docReference for item->>productId CFTC Part45 S1 \"S1\" rationale \"why\" provision \"p\" reportedField]\n";
        let file = SourceFile::new("t.rosetta", src);
        let (unit, diags) = parse(&file);
        assert!(diags.is_empty(), "{diags:?}");
        let model = super::lower("t.rosetta", unit.as_ref().unwrap());
        let doc = match model
            .elements
            .iter()
            .find(|e| matches!(e, sigil_model::SemanticElement::Data(_)))
        {
            Some(sigil_model::SemanticElement::Data(d)) => &d.attributes[0].doc_references[0],
            other => panic!("expected data, got {other:?}"),
        };
        assert_eq!(
            doc.for_path.as_ref().unwrap(),
            &vec!["item".to_string(), "->>productId".to_string()],
        );
        assert_eq!(doc.body, "CFTC");
        assert_eq!(doc.corpora, vec!["Part45"]);
        assert_eq!(doc.segments, vec![("S1".to_string(), "S1".to_string())]);
        assert_eq!(doc.rationales.len(), 1);
        assert_eq!(doc.rationales[0].rationale.as_deref(), Some("why"));
        assert_eq!(doc.provision.as_deref(), Some("p"));
        assert!(doc.reported_field);
    }
}
