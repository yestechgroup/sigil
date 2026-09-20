//! Chumsky-based parser for the Rune `.rosetta` grammar (Milestone 1 subset).
//!
//! Grammar subset (see `docs/reference/Rosetta.xtext` for the full grammar):
//! - model header: `override? namespace`, `scope`, `version`, imports,
//!   qualifiable configurations
//! - elements: `type`, `choice`, `enum`, `annotation`, `typeAlias`,
//!   `basicType`, `recordType`, `library function`
//! - attributes with full cardinalities, definitions (`<"...">`),
//!   annotation refs (`[metadata scheme]`, qualifiers), doc references,
//!   labels, rule references
//!
//! Unsupported constructs (`func`, `rule`, `report`, `schema`, conditions,
//! ...) produce a diagnostic identifying the construct and are skipped up to
//! the next element boundary.

use chumsky::prelude::*;
use sigil_diag::{Diagnostic, SourceFile, Span};

use crate::ast::*;

type AErr<'src> = extra::Err<Rich<'src, char, SimpleSpan>>;

/// Convert a chumsky span into the toolchain-wide span type.
fn span_of(span: SimpleSpan) -> Span {
    Span::new(span.start(), span.end())
}

/// Keywords that start a top-level element. Used to keep identifiers from
/// swallowing element starts (e.g. enum values, choice options).
const ELEMENT_KEYWORDS: &[&str] = &[
    "namespace",
    "type",
    "enum",
    "choice",
    "annotation",
    "typeAlias",
    "basicType",
    "recordType",
    "library",
    "func",
    "report",
    "rule",
    "schema",
    "body",
    "corpus",
    "segment",
    "metaType",
];

/// Constructs we recognise but do not yet model.
const UNSUPPORTED_KEYWORDS: &[&str] = &[
    "func",
    "report",
    "rule",
    "schema",
    "body",
    "corpus",
    "segment",
    "metaType",
    "reporting",
    "eligibility",
];

fn ws<'src>() -> impl Parser<'src, &'src str, (), AErr<'src>> + Clone {
    let line = just("//")
        .ignore_then(none_of('\n').repeated())
        .then_ignore(just('\n').or_not())
        .ignored();
    let block = just("/*")
        .ignore_then(any().and_is(just("*/").not()).repeated().ignored())
        .ignore_then(just("*/"))
        .ignored();
    let space = any().filter(|c: &char| c.is_whitespace()).ignored();
    choice((block, line, space)).repeated().ignored()
}

/// An Xtext `ID`: `^? [A-Za-z_] [A-Za-z0-9_]*`.
/// Returns `(was_caret_escaped, unescaped_name)`.
fn id_token<'src>() -> impl Parser<'src, &'src str, (bool, String), AErr<'src>> + Clone {
    let start = any().filter(|c: &char| c.is_ascii_alphabetic() || *c == '_');
    let rest = any().filter(|c: &char| c.is_ascii_alphanumeric() || *c == '_');
    let ident = start
        .then(rest.repeated().collect::<Vec<char>>())
        .map(|(first, rest_chars)| {
            let mut name = String::new();
            name.push(first);
            name.extend(rest_chars);
            name
        });
    just('^')
        .to(true)
        .or_not()
        .then(ident)
        .then_ignore(ws())
        .map(|(escaped, name)| (escaped.unwrap_or(false), name))
        .labelled("identifier")
}

/// A plain identifier (ValidID). Rune allows a few keywords as identifiers,
/// which falls out naturally here because keywords are matched textually.
fn name<'src>() -> impl Parser<'src, &'src str, String, AErr<'src>> + Clone {
    id_token().map(|(_, name)| name)
}

/// Matches exactly the keyword `k`, rejecting `^k` caret escapes.
fn kw<'src>(k: &'static str) -> impl Parser<'src, &'src str, (), AErr<'src>> + Clone {
    id_token()
        .try_map(move |(escaped, text), span| {
            if !escaped && text == k {
                Ok(())
            } else {
                Err(Rich::custom(span, format!("expected '{k}'")))
            }
        })
        .labelled(k)
}

fn sym<'src>(s: &'static str) -> impl Parser<'src, &'src str, (), AErr<'src>> + Clone {
    just(s).to(()).then_ignore(ws()).labelled(s.to_string())
}

/// An Xtext `STRING` terminal: single- or double-quoted with escapes.
fn string_lit<'src>() -> impl Parser<'src, &'src str, String, AErr<'src>> + Clone {
    let escape = just('\\').ignore_then(choice((
        just('\\'),
        just('"'),
        just('\''),
        just('n').to('\n'),
        just('t').to('\t'),
        just('r').to('\r'),
        just('b').to('\u{0008}'),
        just('f').to('\u{000C}'),
        any(),
    )));
    let dq = escape
        .or(none_of("\"\\"))
        .repeated()
        .collect::<Vec<char>>()
        .delimited_by(just('"'), just('"'))
        .map(|cs| cs.into_iter().collect::<String>());
    let sq = escape
        .or(none_of("'"))
        .repeated()
        .collect::<Vec<char>>()
        .delimited_by(just('\''), just('\''))
        .map(|cs| cs.into_iter().collect::<String>());
    dq.or(sq).then_ignore(ws()).labelled("string literal")
}

/// Unsigned integer (`INT` terminal).
fn uint<'src>() -> impl Parser<'src, &'src str, u32, AErr<'src>> + Clone {
    text::int(10)
        .try_map(|s: &str, span| {
            s.parse::<u32>()
                .map_err(|_| Rich::custom(span, format!("'{s}' is out of range")))
        })
        .then_ignore(ws())
        .labelled("integer")
}

/// Signed integer literal.
fn int_lit<'src>() -> impl Parser<'src, &'src str, i128, AErr<'src>> + Clone {
    let sign = one_of("+-").or_not();
    sign.then(text::int(10))
        .try_map(|(sign, s): (Option<char>, &str), span| {
            let negative = sign == Some('-');
            s.parse::<i128>()
                .map(|v| if negative { -v } else { v })
                .map_err(|_| Rich::custom(span, format!("'{s}' is out of range")))
        })
        .then_ignore(ws())
        .labelled("integer literal")
}

/// `BigDecimal` literal — requires a fractional part, per the grammar.
fn number_lit<'src>() -> impl Parser<'src, &'src str, String, AErr<'src>> + Clone {
    any()
        .filter(|c: &char| c.is_ascii_digit() || matches!(c, '+' | '-' | '.' | 'e' | 'E'))
        .repeated()
        .at_least(1)
        .collect::<String>()
        .try_map(|s: String, span| {
            let body = s.trim_start_matches(['+', '-']);
            let has_dot = body.contains('.');
            let valid = has_dot
                && body
                    .chars()
                    .all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'));
            if valid {
                Ok(s)
            } else {
                Err(Rich::custom(span, "expected number literal"))
            }
        })
        .then_ignore(ws())
        .labelled("number literal")
}

/// Qualified name: `ValidID ('.' ValidID)*`
fn qname<'src>() -> impl Parser<'src, &'src str, QName, AErr<'src>> + Clone {
    name()
        .separated_by(sym("."))
        .at_least(1)
        .collect::<Vec<String>>()
        .map(|parts| QName(parts.join(".")))
        .labelled("qualified name")
}

/// `<"definition text">` or nothing.
fn definable<'src>() -> impl Parser<'src, &'src str, Option<String>, AErr<'src>> + Clone {
    sym("<")
        .ignore_then(string_lit())
        .then_ignore(sym(">"))
        .or_not()
}

fn bool_kw<'src>() -> impl Parser<'src, &'src str, bool, AErr<'src>> + Clone {
    kw("True").to(true).or(kw("False").to(false))
}

/// `('[' ']')` — marks library function parameters as arrays.
fn array_marker<'src>() -> impl Parser<'src, &'src str, bool, AErr<'src>> + Clone {
    sym("[")
        .ignore_then(sym("]"))
        .to(true)
        .or_not()
        .map(|v| v.unwrap_or(false))
}

pub fn annotation_path<'src>() -> impl Parser<'src, &'src str, AnnotationPath, AErr<'src>> + Clone {
    let segment = choice((sym("->>").to(true), sym("->").to(false)))
        .then(name())
        .map(|(deep, attribute)| AnnotationPathSegment { attribute, deep });
    kw("item")
        .to(())
        .map(|_| AnnotationPath {
            starts_with_item: true,
            segments: Vec::new(),
        })
        .or(name()
            .then(segment.repeated().collect::<Vec<_>>())
            .map(|(first, mut segments)| {
                segments.insert(
                    0,
                    AnnotationPathSegment {
                        attribute: first,
                        deep: false,
                    },
                );
                AnnotationPath {
                    starts_with_item: false,
                    segments,
                }
            }))
        .labelled("annotation path")
}

fn qualifier_value<'src>() -> impl Parser<'src, &'src str, QualifierValue, AErr<'src>> + Clone {
    let path = qname()
        .then(
            sym("->")
                .ignore_then(name())
                .repeated()
                .at_least(1)
                .collect::<Vec<_>>(),
        )
        .map(|(data, attributes)| QualifierValue::Path(QualifierPath { data, attributes }));
    string_lit()
        .map(QualifierValue::Str)
        .or(path)
        .labelled("qualifier value")
}

fn annotation_qualifier<'src>(
) -> impl Parser<'src, &'src str, AnnotationQualifier, AErr<'src>> + Clone {
    string_lit()
        .then_ignore(sym("="))
        .then(qualifier_value())
        .map_with(|(name, value), ex| AnnotationQualifier {
            name,
            value,
            span: span_of(ex.span()),
        })
        .labelled("annotation qualifier")
}

/// `[annotation attribute "qual"=value ...]`
pub fn annotation_ref<'src>() -> impl Parser<'src, &'src str, AnnotationRef, AErr<'src>> + Clone {
    sym("[")
        .ignore_then(qname())
        .then(name().or_not())
        .then(annotation_qualifier().repeated().collect::<Vec<_>>())
        .then_ignore(sym("]"))
        .map_with(|((annotation, attribute), qualifiers), ex| AnnotationRef {
            annotation,
            attribute,
            qualifiers,
            span: span_of(ex.span()),
        })
        .labelled("annotation reference")
}

fn document_rationale<'src>() -> impl Parser<'src, &'src str, DocumentRationale, AErr<'src>> + Clone
{
    let rationale = kw("rationale").ignore_then(string_lit());
    let author = kw("rationale_author").ignore_then(string_lit());
    rationale
        .clone()
        .then(author.clone().or_not())
        .map(|(rationale, rationale_author)| DocumentRationale {
            rationale: Some(rationale),
            rationale_author,
        })
        .or(author
            .then(rationale.or_not())
            .map(|(rationale_author, rationale)| DocumentRationale {
                rationale,
                rationale_author: Some(rationale_author),
            }))
}

/// `[docReference for path Body Corpus (Segment "ref")* ...]`
pub fn doc_reference<'src>() -> impl Parser<'src, &'src str, DocReference, AErr<'src>> + Clone {
    let for_clause = kw("for").ignore_then(annotation_path()).or_not();
    let item = qname().then(string_lit().or_not());
    let structured = kw("structured_provision")
        .ignore_then(string_lit())
        .or_not();
    let provision = kw("provision").ignore_then(string_lit()).or_not();
    let reported = kw("reportedField").to(()).or_not();
    sym("[")
        .ignore_then(kw("docReference"))
        .ignore_then(for_clause)
        .then(item.repeated().at_least(1).collect::<Vec<_>>())
        .then(document_rationale().repeated().collect::<Vec<_>>())
        .then(structured)
        .then(provision)
        .then(reported)
        .then_ignore(sym("]"))
        .map_with(
            |(((((for_path, items), rationales), structured_provision), provision), reported),
             ex| {
                let mut names = Vec::new();
                let mut segments = Vec::new();
                for (name, segment_ref) in items {
                    match segment_ref {
                        Some(reference) => segments.push((name.0, reference)),
                        None => names.push(name),
                    }
                }
                let body = names.first().cloned().unwrap_or(QName(String::new()));
                if !names.is_empty() {
                    names.remove(0);
                }
                DocReference {
                    for_path,
                    body,
                    corpora: names,
                    segments,
                    rationales,
                    structured_provision,
                    provision,
                    reported_field: reported.is_some(),
                    span: span_of(ex.span()),
                }
            },
        )
        .labelled("doc reference")
}

/// `[label for path "label"]`
pub fn label_annotation<'src>() -> impl Parser<'src, &'src str, LabelAnnotation, AErr<'src>> + Clone
{
    let for_clause = kw("for").ignore_then(annotation_path()).or_not();
    sym("[")
        .ignore_then(kw("label"))
        .ignore_then(for_clause)
        .then(string_lit())
        .then_ignore(sym("]"))
        .map_with(|(for_path, label), ex| LabelAnnotation {
            for_path,
            label,
            span: span_of(ex.span()),
        })
        .labelled("label annotation")
}

/// `[ruleReference CDM.Rule]` / `[ruleReference empty]`
pub fn rule_reference<'src>() -> impl Parser<'src, &'src str, RuleReference, AErr<'src>> + Clone {
    let for_clause = kw("for").ignore_then(annotation_path()).or_not();
    sym("[")
        .ignore_then(kw("ruleReference"))
        .ignore_then(for_clause)
        .then(qname().map(Some).or(kw("empty").to(None)))
        .then_ignore(sym("]"))
        .map_with(|(for_path, rule), ex| RuleReference {
            for_path,
            empty: rule.is_none(),
            rule,
            span: span_of(ex.span()),
        })
        .labelled("rule reference")
}

/// The `[...]` fragment shared by data types, attributes and enum values.
fn references_and_annotations<'src>() -> impl Parser<
    'src,
    &'src str,
    (
        Vec<DocReference>,
        Vec<AnnotationRef>,
        Vec<LabelAnnotation>,
        Vec<RuleReference>,
    ),
    AErr<'src>,
> + Clone {
    #[derive(Debug, Clone)]
    enum Fragment {
        Doc(DocReference),
        Anno(AnnotationRef),
        Label(LabelAnnotation),
        Rule(RuleReference),
    }
    choice((
        doc_reference().map(Fragment::Doc),
        annotation_ref().map(Fragment::Anno),
        label_annotation().map(Fragment::Label),
        rule_reference().map(Fragment::Rule),
    ))
    .repeated()
    .collect::<Vec<_>>()
    .map(|fragments| {
        let mut docs = Vec::new();
        let mut annos = Vec::new();
        let mut labels = Vec::new();
        let mut rules = Vec::new();
        for fragment in fragments {
            match fragment {
                Fragment::Doc(d) => docs.push(d),
                Fragment::Anno(a) => annos.push(a),
                Fragment::Label(l) => labels.push(l),
                Fragment::Rule(r) => rules.push(r),
            }
        }
        (docs, annos, labels, rules)
    })
}

/// A type reference with optional arguments: `number(digits: 30)`.
pub fn type_call<'src>() -> impl Parser<'src, &'src str, TypeCall, AErr<'src>> + Clone {
    let argument = name()
        .then_ignore(sym(":"))
        .then(
            bool_kw()
                .map(TypeCallArgumentValue::Bool)
                .or(string_lit().map(TypeCallArgumentValue::Str))
                .or(number_lit().map(TypeCallArgumentValue::Number))
                .or(int_lit().map(TypeCallArgumentValue::Int))
                .or(name().map(TypeCallArgumentValue::Reference)),
        )
        .map_with(|(parameter, value), ex| TypeCallArgument {
            parameter,
            value,
            span: span_of(ex.span()),
        });
    qname()
        .then(
            argument
                .separated_by(sym(","))
                .allow_trailing()
                .collect::<Vec<_>>()
                .delimited_by(sym("("), sym(")"))
                .or_not(),
        )
        .map_with(|(name, arguments), ex| TypeCall {
            name,
            arguments: arguments.unwrap_or_default(),
            span: span_of(ex.span()),
        })
        .labelled("type reference")
}

/// `(min..max)` or `(min..*)`. Required on every attribute.
fn cardinality<'src>() -> impl Parser<'src, &'src str, Cardinality, AErr<'src>> + Clone {
    sym("(")
        .ignore_then(uint())
        .then_ignore(sym(".."))
        .then(
            uint()
                .map(CardinalityMax::Finite)
                .or(sym("*").to(CardinalityMax::Unbounded)),
        )
        .then_ignore(sym(")"))
        .map(|(min, max)| Cardinality { min, max })
        .labelled("cardinality")
}

fn attribute<'src>() -> impl Parser<'src, &'src str, AttributeDef, AErr<'src>> + Clone {
    kw("override")
        .to(true)
        .or_not()
        .then(name())
        .then(type_call())
        .then(cardinality())
        .then(definable())
        .then(references_and_annotations())
        .map_with(
            |(
                ((((is_override, name), type_call), cardinality), definition),
                (docs, annos, labels, rules),
            ),
             ex| {
                AttributeDef {
                    name,
                    is_override: is_override.unwrap_or(false),
                    type_call,
                    cardinality,
                    definition,
                    doc_references: docs,
                    annotations: annos,
                    labels,
                    rule_references: rules,
                    span: span_of(ex.span()),
                }
            },
        )
        .labelled("attribute")
}

/// A `choice` option: a bare type reference with implicit `(0..1)`
/// cardinality and no `override` marker.
fn choice_option<'src>() -> impl Parser<'src, &'src str, AttributeDef, AErr<'src>> + Clone {
    type_call()
        .then(definable())
        .then(references_and_annotations())
        .try_map(
            |((type_call, definition), (docs, annos, labels, rules)), span| {
                // An option names a type; a lone element keyword is the start of
                // the next element, not an option.
                if type_call.name.segment_count() == 1
                    && ELEMENT_KEYWORDS.contains(&type_call.name.0.as_str())
                {
                    Err(Rich::custom(span, "choice option"))
                } else {
                    Ok((type_call, definition, docs, annos, labels, rules, span))
                }
            },
        )
        .map_with(
            |(type_call, definition, docs, annos, labels, rules, span), _| AttributeDef {
                name: type_call.name.segments().last().unwrap_or("").to_string(),
                is_override: false,
                type_call,
                cardinality: Cardinality {
                    min: 0,
                    max: CardinalityMax::Finite(1),
                },
                definition,
                doc_references: docs,
                annotations: annos,
                labels,
                rule_references: rules,
                span: span_of(span),
            },
        )
        .labelled("choice option")
}

fn enum_value<'src>() -> impl Parser<'src, &'src str, EnumValueDef, AErr<'src>> + Clone {
    name()
        .filter(|n: &String| !ELEMENT_KEYWORDS.contains(&n.as_str()))
        .then(kw("displayName").ignore_then(string_lit()).or_not())
        .then(definable())
        .then(references_and_annotations())
        .map_with(
            |(((name, display), definition), (docs, annos, _, _)), ex| EnumValueDef {
                name,
                display,
                definition,
                doc_references: docs,
                annotations: annos,
                span: span_of(ex.span()),
            },
        )
        .labelled("enum value")
}

fn type_parameters<'src>() -> impl Parser<'src, &'src str, Vec<TypeParameter>, AErr<'src>> + Clone {
    let parameter = name().then(type_call()).then(definable()).map_with(
        |((name, type_call), definition), ex| TypeParameter {
            name,
            type_call,
            definition,
            span: span_of(ex.span()),
        },
    );
    parameter
        .separated_by(sym(","))
        .allow_trailing()
        .collect::<Vec<_>>()
        .delimited_by(sym("("), sym(")"))
        .or_not()
        .map(|p| p.unwrap_or_default())
}

/// Lookahead recognising the start of a top-level element.
///
/// Refines plain keyword matching: `type Foo:` is a boundary, but the
/// `type` inside `report ... with type Foo ...` is not.
fn element_boundary<'src>() -> impl Parser<'src, &'src str, (), AErr<'src>> + Clone {
    fn boundary<'src>() -> impl Parser<'src, &'src str, (), AErr<'src>> + Clone {
        let colon_or_extends = choice((sym(":").to(()), kw("extends").to(())));
        let type_like = choice((
            kw("type")
                .ignore_then(name())
                .ignore_then(colon_or_extends.clone().rewind()),
            kw("enum")
                .ignore_then(name())
                .ignore_then(colon_or_extends.clone().rewind()),
            kw("choice")
                .ignore_then(name())
                .ignore_then(colon_or_extends.clone().rewind()),
            kw("annotation")
                .ignore_then(name())
                .ignore_then(sym(":").rewind()),
            kw("typeAlias")
                .ignore_then(name())
                .ignore_then(choice((sym(":").to(()), sym("(").to(()))).rewind()),
            kw("func").ignore_then(name()).ignore_then(
                choice((sym(":").to(()), sym("(").to(()), kw("extends").to(()))).rewind(),
            ),
        ));
        let basic = kw("basicType")
            .ignore_then(name())
            .ignore_then(choice((sym("(").to(()), sym("<").to(()))).rewind());
        let library = kw("library")
            .ignore_then(kw("function"))
            .ignore_then(name())
            .ignore_then(sym("(").rewind());
        let simple = choice((
            kw("namespace").to(()),
            kw("schema").to(()),
            kw("body").to(()),
            kw("corpus").to(()),
            kw("segment").to(()),
            kw("metaType").to(()),
        ));
        choice((type_like, basic, library, simple))
    }
    boundary()
}

/// Consumes tokens until the next element boundary or EOF (not consuming it).
fn skip_to_element_boundary<'src>() -> impl Parser<'src, &'src str, (), AErr<'src>> + Clone {
    any()
        .and_is(element_boundary().not())
        .ignored()
        .repeated()
        .then_ignore(ws())
}

/// `condition Foo: <expression>` inside a type — recognised but not yet
/// modelled. Skips to the next plausible boundary.
fn unsupported_condition<'src>(
) -> impl Parser<'src, &'src str, UnsupportedElement, AErr<'src>> + Clone {
    kw("condition")
        .then(skip_to_element_boundary())
        .map_with(|(_, ()), ex| UnsupportedElement {
            keyword: "condition".to_string(),
            span: span_of(ex.span()),
        })
}

fn data_def<'src>(choice: bool) -> impl Parser<'src, &'src str, DataDef, AErr<'src>> + Clone {
    let header = if choice { kw("choice") } else { kw("type") };
    let body: Boxed<
        'src,
        'src,
        &'src str,
        (Vec<AttributeDef>, Option<UnsupportedElement>),
        AErr<'src>,
    > = if choice {
        choice_option()
            .repeated()
            .collect::<Vec<_>>()
            .map(|options| (options, None))
            .boxed()
    } else {
        attribute()
            .repeated()
            .collect::<Vec<_>>()
            .then(unsupported_condition().or_not())
            .boxed()
    };
    header
        .ignore_then(name())
        .then(kw("extends").ignore_then(qname()).or_not())
        .then_ignore(sym(":"))
        .then(definable())
        .then(references_and_annotations())
        .then(body)
        .map_with(
            |((((name, super_type), definition), (docs, annos, _, _)), (members, unsupported)),
             ex| {
                let mut span = span_of(ex.span());
                if let Some(unsupported) = unsupported {
                    span = span.merge(unsupported.span);
                }
                DataDef {
                    name,
                    super_type,
                    definition,
                    doc_references: docs,
                    annotations: annos,
                    attributes: members,
                    span,
                }
            },
        )
        .labelled(if choice { "choice" } else { "type" })
}

fn enum_def<'src>() -> impl Parser<'src, &'src str, EnumDef, AErr<'src>> + Clone {
    kw("enum")
        .ignore_then(name())
        .then(kw("extends").ignore_then(qname()).or_not())
        .then_ignore(sym(":"))
        .then(definable())
        .then(references_and_annotations())
        .then(enum_value().repeated().collect::<Vec<_>>())
        .map_with(
            |((((name, super_type), definition), (docs, annos, _, _)), values), ex| EnumDef {
                name,
                super_type,
                definition,
                doc_references: docs,
                annotations: annos,
                values,
                span: span_of(ex.span()),
            },
        )
        .labelled("enum")
}

fn annotation_decl<'src>() -> impl Parser<'src, &'src str, AnnotationDecl, AErr<'src>> + Clone {
    kw("annotation")
        .ignore_then(name())
        .then_ignore(sym(":"))
        .then(definable())
        .then(
            sym("[")
                .ignore_then(kw("prefix").ignore_then(name()))
                .then_ignore(sym("]"))
                .or_not(),
        )
        .then(attribute().repeated().collect::<Vec<_>>())
        .map_with(
            |(((name, definition), prefix), attributes), ex| AnnotationDecl {
                name,
                definition,
                prefix,
                attributes,
                span: span_of(ex.span()),
            },
        )
        .labelled("annotation")
}

fn type_alias<'src>() -> impl Parser<'src, &'src str, TypeAliasDef, AErr<'src>> + Clone {
    kw("typeAlias")
        .ignore_then(name())
        .then(type_parameters())
        .then_ignore(sym(":"))
        .then(definable())
        .then(type_call())
        .then(references_and_annotations())
        .map_with(
            |((((name, parameters), definition), type_call), (docs, annos, _, _)), ex| {
                TypeAliasDef {
                    name,
                    parameters,
                    definition,
                    type_call,
                    doc_references: docs,
                    annotations: annos,
                    span: span_of(ex.span()),
                }
            },
        )
        .labelled("type alias")
}

fn basic_type<'src>() -> impl Parser<'src, &'src str, BasicTypeDecl, AErr<'src>> + Clone {
    kw("basicType")
        .ignore_then(name())
        .then(type_parameters())
        .then(definable())
        .map_with(|((name, parameters), definition), ex| BasicTypeDecl {
            name,
            parameters,
            definition,
            span: span_of(ex.span()),
        })
        .labelled("basic type")
}

fn record_type<'src>() -> impl Parser<'src, &'src str, RecordTypeDecl, AErr<'src>> + Clone {
    let feature = name()
        .then(type_call())
        .map_with(|(name, type_call), ex| RecordFeature {
            name,
            type_call,
            span: span_of(ex.span()),
        });
    kw("recordType")
        .ignore_then(name())
        .then_ignore(sym("{"))
        .then(definable())
        .then(feature.repeated().collect::<Vec<_>>())
        .then_ignore(sym("}"))
        .map_with(|((name, definition), features), ex| RecordTypeDecl {
            name,
            definition,
            features,
            span: span_of(ex.span()),
        })
        .labelled("record type")
}

fn library_function<'src>() -> impl Parser<'src, &'src str, LibraryFunctionDecl, AErr<'src>> + Clone
{
    let parameter = name()
        .then(type_call())
        .then(array_marker())
        .map(|((name, type_call), is_array)| (name, type_call, is_array));
    kw("library")
        .ignore_then(kw("function"))
        .ignore_then(name())
        .then(
            parameter
                .separated_by(sym(","))
                .allow_trailing()
                .collect::<Vec<_>>()
                .delimited_by(sym("("), sym(")"))
                .or_not(),
        )
        .then(type_call())
        .then(definable())
        .map_with(
            |(((name, parameters), return_type), definition), ex| LibraryFunctionDecl {
                name,
                parameters: parameters.unwrap_or_default(),
                return_type,
                definition,
                span: span_of(ex.span()),
            },
        )
        .labelled("library function")
}

/// `func ...`, `rule ...`, `report ...`, etc. — recognised but not yet
/// modelled; skipped up to the next element boundary.
fn unsupported_element<'src>(
) -> impl Parser<'src, &'src str, UnsupportedElement, AErr<'src>> + Clone {
    id_token()
        .try_map(|(escaped, text), span| {
            if !escaped && UNSUPPORTED_KEYWORDS.contains(&text.as_str()) {
                Ok(text)
            } else {
                Err(Rich::custom(span, "unsupported construct"))
            }
        })
        .then(
            choice((
                kw("source").to("source"),
                kw("rule").to("rule"),
                kw("function").to("function"),
            ))
            .or_not(),
        )
        .then(skip_to_element_boundary())
        .map_with(|((keyword, suffix), _), ex| UnsupportedElement {
            keyword: match suffix {
                Some(suffix) => format!("{keyword} {suffix}"),
                None => keyword,
            },
            span: span_of(ex.span()),
        })
        .labelled("element")
}

fn element<'src>() -> impl Parser<'src, &'src str, Element, AErr<'src>> + Clone {
    choice((
        data_def(false).map(Element::Data),
        data_def(true).map(Element::Choice),
        enum_def().map(Element::Enumeration),
        annotation_decl().map(Element::Annotation),
        type_alias().map(Element::TypeAlias),
        basic_type().map(Element::BasicType),
        record_type().map(Element::RecordType),
        library_function().map(Element::LibraryFunction),
    ))
}

fn import_decl<'src>() -> impl Parser<'src, &'src str, ImportDecl, AErr<'src>> + Clone {
    kw("import")
        .ignore_then(qname())
        .then(sym(".").ignore_then(sym("*")).to(true).or_not())
        .then(kw("as").ignore_then(name()).or_not())
        .map_with(|((namespace, wildcard), alias), ex| {
            let wildcard = wildcard.unwrap_or(false);
            ImportDecl {
                namespace,
                wildcard,
                alias,
                span: span_of(ex.span()),
            }
        })
        .labelled("import")
}

fn qualifiable_configuration<'src>(
) -> impl Parser<'src, &'src str, QualifiableConfiguration, AErr<'src>> + Clone {
    kw("isEvent")
        .to(QualifiableType::Event)
        .or(kw("isProduct").to(QualifiableType::Product))
        .then_ignore(kw("root"))
        .then(qname())
        .then_ignore(sym(";"))
        .map_with(|(q_type, root), ex| QualifiableConfiguration {
            q_type,
            root,
            span: span_of(ex.span()),
        })
        .labelled("qualifiable configuration")
}

pub fn source_unit<'src>() -> impl Parser<'src, &'src str, SourceUnit, AErr<'src>> + Clone {
    let header = ws().ignore_then(
        kw("override")
            .to(true)
            .or_not()
            .then_ignore(kw("namespace"))
            .then(qname().map(Some).or(string_lit().map(|s| Some(QName(s)))))
            .then(sym(":").ignore_then(definable()).or_not())
            .then(kw("scope").ignore_then(name()).then(definable()).or_not())
            .then(kw("version").ignore_then(string_lit()).or_not())
            .map_with(
                |((((overridden, namespace), definition), scope), version), ex| {
                    (
                        NamespaceDecl {
                            overridden: overridden.unwrap_or(false),
                            name: namespace.unwrap_or(QName(String::new())),
                            definition: definition.flatten(),
                            span: span_of(ex.span()),
                        },
                        scope,
                        version,
                    )
                },
            ),
    );
    header
        .then(import_decl().repeated().collect::<Vec<_>>())
        .then(qualifiable_configuration().repeated().collect::<Vec<_>>())
        .then(
            element()
                .or(unsupported_element().map(Element::Unsupported))
                .repeated()
                .collect::<Vec<_>>(),
        )
        .then_ignore(end())
        .map_with(
            |((((namespace, scope, version), imports), configurations), elements), ex| SourceUnit {
                namespace,
                scope,
                version,
                imports,
                configurations,
                elements,
                span: span_of(ex.span()),
            },
        )
        .labelled("model")
}

/// Parse one `.rosetta` file into a spanned AST plus parse diagnostics.
///
/// Runs on a dedicated thread with a large stack: chumsky's combinator
/// trees are stack-hungry during construction and backtracking, and real
/// models can be deeply nested.
pub fn parse(file: &SourceFile) -> (Option<SourceUnit>, Vec<Diagnostic>) {
    let file = file.clone();
    std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(move || parse_inner(&file))
        .expect("failed to spawn parser thread")
        .join()
        .expect("parser thread panicked")
}

fn parse_inner(file: &SourceFile) -> (Option<SourceUnit>, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    match source_unit().parse(&file.text).into_result() {
        Ok(unit) => {
            for element in &unit.elements {
                if let crate::ast::Element::Unsupported(unsupported) = element {
                    diagnostics.push(Diagnostic::warning(
                        "W0001",
                        format!(
                            "unsupported construct '{}' skipped (not yet modelled by sigil)",
                            unsupported.keyword
                        ),
                        file,
                        unsupported.span,
                    ));
                }
            }
            (Some(unit), diagnostics)
        }
        Err(errors) => {
            for e in errors {
                let span = Span::new(e.span().start, e.span().end);
                diagnostics.push(Diagnostic::error("E0001", format!("{e}"), file, span));
            }
            (None, diagnostics)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ok(src: &str) -> SourceUnit {
        let file = SourceFile::new("test.rosetta", src);
        let (unit, diags) = parse(&file);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:#?}");
        unit.unwrap()
    }

    #[test]
    fn parses_minimal_model() {
        let unit = parse_ok("namespace test.pojo\n\ntype Foo:\n    attr int (1..1)\n");
        assert_eq!(unit.namespace.name.0, "test.pojo");
        assert_eq!(unit.elements.len(), 1);
        let data = match &unit.elements[0] {
            Element::Data(d) => d,
            other => panic!("expected data, got {other:?}"),
        };
        assert_eq!(data.name, "Foo");
        assert_eq!(data.attributes[0].name, "attr");
        assert_eq!(
            data.attributes[0].cardinality.to_constraint_string(),
            "(1..1)"
        );
    }

    #[test]
    fn parses_pojo_sample() {
        let unit = parse_ok(
            "namespace test.pojo\n\ntype Foo1:\n\tnumberAttr number (0..1)\n\tparent Parent (1..1)\n\tparentList Parent (0..10)\n\totherParentList Parent (0..*)\n\n\tstringAttr string (1..1)\n\t\t[metadata scheme]\n",
        );
        assert_eq!(unit.elements.len(), 1);
    }

    #[test]
    fn parses_inheritance_and_override() {
        let unit = parse_ok(
            "namespace t\ntype A:\n\tx int (1..1)\n\ntype B extends A:\n\toverride x int(digits: 30, max: 100) (1..1)\n",
        );
        assert_eq!(unit.elements.len(), 2);
        match &unit.elements[1] {
            Element::Data(d) => {
                assert_eq!(d.super_type.as_ref().unwrap().0, "A");
                let a = &d.attributes[0];
                assert!(a.is_override);
                assert_eq!(a.type_call.name.0, "int");
                assert_eq!(a.type_call.arguments.len(), 2);
                assert_eq!(
                    a.type_call.arguments[0].value,
                    TypeCallArgumentValue::Int(30)
                );
            }
            other => panic!("expected data, got {other:?}"),
        }
    }

    #[test]
    fn parses_enum() {
        let unit = parse_ok(
            "namespace t\nenum E extends Base: <\"docs\">\n\tA <\"first\">\n\tB displayName \"Bee\"\n",
        );
        match &unit.elements[0] {
            Element::Enumeration(e) => {
                assert_eq!(e.values.len(), 2);
                assert_eq!(e.values[1].display.as_deref(), Some("Bee"));
                assert_eq!(e.definition.as_deref(), Some("docs"));
            }
            other => panic!("expected enum, got {other:?}"),
        }
    }

    #[test]
    fn parses_choice() {
        let unit = parse_ok(
            "namespace t\nchoice Vehicle: <\"transport\">\n\t[metadata key]\n\tCar <\"a car\">\n\tBicycle\n",
        );
        match &unit.elements[0] {
            Element::Choice(c) => {
                assert_eq!(c.attributes.len(), 2);
                assert_eq!(c.attributes[0].name, "Car");
                assert_eq!(c.annotations.len(), 1);
            }
            other => panic!("expected choice, got {other:?}"),
        }
    }

    #[test]
    fn parses_annotation_decl_and_ref() {
        let unit = parse_ok(
            "namespace t\nannotation metadata: <\"meta\">\n\tkey string (0..1)\n\tid string (0..1)\n\ntype T:\n\t[metadata key]\n\tx string (0..1)\n",
        );
        assert_eq!(unit.elements.len(), 2);
        match &unit.elements[1] {
            Element::Data(d) => {
                assert_eq!(d.annotations.len(), 1);
                assert_eq!(d.annotations[0].annotation.0, "metadata");
                assert_eq!(d.annotations[0].attribute.as_deref(), Some("key"));
            }
            other => panic!("expected data, got {other:?}"),
        }
    }

    #[test]
    fn parses_imports_and_configurations() {
        let unit = parse_ok(
            "namespace t\nversion \"1.2.3\"\nimport a.b.* as dep\nimport c.D\nisEvent root Foo;\ntype Foo:\n\tx int (0..1)\n",
        );
        assert_eq!(unit.imports.len(), 2);
        assert_eq!(unit.imports[0].alias.as_deref(), Some("dep"));
        assert!(unit.imports[0].wildcard);
        assert!(!unit.imports[1].wildcard);
        assert_eq!(unit.configurations.len(), 1);
        assert_eq!(unit.configurations[0].root.0, "Foo");
    }

    #[test]
    fn parses_builtin_file_shapes() {
        let unit = parse_ok(
            "namespace com.rosetta.model\nversion \"9\"\n\nbasicType boolean <\"A boolean can either be True or False.\">\n\nbasicType number(\n    digits int\n  , fractionalDigits int\n) <\"A signed decimal number.\">\n\ntypeAlias int(digits int, min int, max int): <\"A signed decimal integer.\">\n\tnumber(digits: digits, fractionalDigits: 0, min: min, max: max)\n\nrecordType date\n{\n\tday   int\n\tmonth int\n\tyear  int\n}\n\nlibrary function Min(x number, y number) number\n",
        );
        assert_eq!(unit.elements.len(), 5);
    }

    #[test]
    fn supports_comments_and_caret_idents() {
        let unit = parse_ok(
            "// leading comment\nnamespace t /* inline */\ntype ^type:\n\tx int (1..1) // trailing\n",
        );
        match &unit.elements[0] {
            Element::Data(d) => assert_eq!(d.name, "type"),
            other => panic!("expected data, got {other:?}"),
        }
    }

    #[test]
    fn unsupported_func_is_reported_and_skipped() {
        let file = SourceFile::new(
            "t.rosetta",
            "namespace t\ntype A:\n\tx int (1..1)\nfunc MyFunc:\n  [codeImplementation]\n  output:\n    result int (1..1)\ntype B:\n\ty int (0..1)\n",
        );
        let (unit, diags) = parse(&file);
        assert!(unit.is_some());
        let unit = unit.unwrap();
        let unsupported = unit
            .elements
            .iter()
            .any(|e| matches!(e, Element::Unsupported(u) if u.keyword == "func"));
        assert!(
            unsupported,
            "expected unsupported func, got {:#?}",
            unit.elements
        );
        assert!(
            diags
                .iter()
                .any(|d| d.code == "W0001" && d.message.contains("func")),
            "expected unsupported-construct warning, got {diags:?}"
        );
    }

    #[test]
    fn syntax_error_is_reported() {
        let file = SourceFile::new("t.rosetta", "namespace t\ntype Broken\n");
        let (_, diags) = parse(&file);
        assert!(!diags.is_empty());
    }
}
