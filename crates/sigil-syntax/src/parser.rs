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
//! Expressions (type-level conditions) are parsed by `expr.rs` per the
//! full grammar.
//!
//! Unsupported constructs (`func`, `rule`, `report`, `schema`, ...) produce
//! a diagnostic identifying the construct and are skipped up to the next
//! element boundary.

use chumsky::prelude::*;
use sigil_diag::{Diagnostic, SourceFile, Span};

use crate::ast::*;

pub(crate) type AErr<'src> = extra::Err<Rich<'src, char, SimpleSpan>>;

/// Convert a chumsky span into the toolchain-wide span type.
pub(crate) fn span_of(span: SimpleSpan) -> Span {
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

/// Constructs we recognise but do not yet model. Every grammar element is
/// modelled as of phase 4, so this list is empty and the W0001 path is
/// dead code kept for future grammar additions.
const UNSUPPORTED_KEYWORDS: &[&str] = &[];

pub(crate) fn ws<'src>() -> impl Parser<'src, &'src str, (), AErr<'src>> + Clone {
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
pub(crate) fn name<'src>() -> impl Parser<'src, &'src str, String, AErr<'src>> + Clone {
    id_token().map(|(_, name)| name)
}

/// An identifier usable as a declaration name of the root elements whose
/// bodies run into following elements (`body`, `corpus`, `segment`, ...).
///
/// In Xtext, words that start another element (`type`, `report`,
/// `eligibility`, ...) are keyword *tokens*, so they can never satisfy an
/// `ID`/`ValidID` and a name can never swallow the next element's first
/// word. The textual matcher has no lexer to lean on, so the same set is
/// excluded explicitly.
fn element_name<'src>() -> impl Parser<'src, &'src str, String, AErr<'src>> + Clone {
    fn stop_words() -> Vec<&'static str> {
        let mut stop: Vec<&'static str> = ELEMENT_KEYWORDS.to_vec();
        stop.extend(["reporting", "eligibility"]);
        stop
    }
    name()
        .try_map(|text: String, span| {
            if stop_words().contains(&text.as_str()) {
                Err(Rich::custom(span, "name"))
            } else {
                Ok(text)
            }
        })
        .labelled("name")
}

/// A qualified name in a *report reference position* (`body`, `corpus`,
/// `segment` and rule references). Besides the element keywords, the
/// report-structural keywords are keyword tokens too and must never be
/// swallowed by the greedy reference lists.
fn report_ref_name<'src>() -> impl Parser<'src, &'src str, String, AErr<'src>> + Clone {
    const STOP: &[&str] = &[
        "in",
        "from",
        "when",
        "with",
        "type",
        "and",
        "reporting",
        "eligibility",
        "namespace",
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
    name()
        .try_map(move |text: String, span| {
            if STOP.contains(&text.as_str()) {
                Err(Rich::custom(span, "report reference"))
            } else {
                Ok(text)
            }
        })
        .labelled("report reference")
}

/// A qualified name over [`report_ref_name`].
fn report_ref_qname<'src>() -> impl Parser<'src, &'src str, SpannedQName, AErr<'src>> + Clone {
    report_ref_name()
        .separated_by(sym("."))
        .at_least(1)
        .collect::<Vec<String>>()
        .map_with(|parts, ex| SpannedQName {
            name: QName(parts.join(".")),
            span: span_of(ex.span()),
        })
        .labelled("report reference")
}

/// Matches exactly the keyword `k`, rejecting `^k` caret escapes.
pub(crate) fn kw<'src>(k: &'static str) -> impl Parser<'src, &'src str, (), AErr<'src>> + Clone {
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

/// Matches a hyphenated keyword token (e.g. `post-condition`, `real-time`,
/// `T+1`). These are Xtext keyword tokens rather than `ID`s, so the matcher
/// is character-based and requires a non-identifier right boundary.
pub(crate) fn word_kw<'src>(
    k: &'static str,
) -> impl Parser<'src, &'src str, (), AErr<'src>> + Clone {
    just(k)
        .to(())
        .then_ignore(
            any()
                .filter(|c: &char| c.is_ascii_alphanumeric() || *c == '_')
                .not(),
        )
        .then_ignore(ws())
        .labelled(k)
}

pub(crate) fn sym<'src>(s: &'static str) -> impl Parser<'src, &'src str, (), AErr<'src>> + Clone {
    just(s).to(()).then_ignore(ws()).labelled(s.to_string())
}

/// An Xtext `STRING` terminal: single- or double-quoted with escapes.
pub(crate) fn string_lit<'src>() -> impl Parser<'src, &'src str, String, AErr<'src>> + Clone {
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

/// Signed integer literal, keeping the source text verbatim (sign included).
pub(crate) fn int_lit_raw<'src>() -> impl Parser<'src, &'src str, String, AErr<'src>> + Clone {
    one_of("+-")
        .or_not()
        .then(text::int(10))
        .then_ignore(ws())
        .try_map(|(sign, digits): (Option<char>, &str), span| {
            digits
                .parse::<i128>()
                .map(|_| match sign {
                    Some(sign) => format!("{sign}{digits}"),
                    None => digits.to_string(),
                })
                .map_err(|_| Rich::custom(span, format!("'{digits}' is out of range")))
        })
        .labelled("integer literal")
}

/// Signed integer literal.
pub(crate) fn int_lit<'src>() -> impl Parser<'src, &'src str, i128, AErr<'src>> + Clone {
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

/// `BigDecimal` literal — requires a fractional part, per the grammar
/// (`sign? ('.' INT | INT '.' | INT '.' INT) exponent?`). Structured rather
/// than a greedy scan so that a trailing `-` (as in `2.0->>x`) is never
/// swallowed.
pub(crate) fn number_lit<'src>() -> impl Parser<'src, &'src str, String, AErr<'src>> + Clone {
    let digits = || text::int(10).map(|digits: &str| digits.to_owned());
    let core = just(".")
        .ignore_then(digits())
        .map(|fraction| format!(".{fraction}"))
        .or(digits()
            .then_ignore(just("."))
            .then(digits().map(|f| format!(".{f}")).or_not())
            .map(|(whole, fraction)| match fraction {
                Some(fraction) => format!("{whole}{fraction}"),
                None => format!("{whole}."),
            }));
    let exponent = one_of("eE")
        .then(one_of("+-").or_not())
        .then(digits())
        .map(|((marker, sign), digits)| match sign {
            Some(sign) => format!("{marker}{sign}{digits}"),
            None => format!("{marker}{digits}"),
        })
        .or_not();
    one_of("+-")
        .or_not()
        .then(core)
        .then(exponent)
        .then_ignore(ws())
        .map(|((sign, core), exponent)| {
            let mut text = String::new();
            if let Some(sign) = sign {
                text.push(sign);
            }
            text.push_str(&core);
            if let Some(exponent) = exponent {
                text.push_str(&exponent);
            }
            text
        })
        .labelled("number literal")
}

/// Qualified name: `ValidID ('.' ValidID)*`
pub(crate) fn qname<'src>() -> impl Parser<'src, &'src str, QName, AErr<'src>> + Clone {
    name()
        .separated_by(sym("."))
        .at_least(1)
        .collect::<Vec<String>>()
        .map(|parts| QName(parts.join(".")))
        .labelled("qualified name")
}

/// A qualified name together with the span of its text.
fn spanned_qname<'src>() -> impl Parser<'src, &'src str, SpannedQName, AErr<'src>> + Clone {
    qname()
        .map_with(|name, ex| SpannedQName {
            name,
            span: span_of(ex.span()),
        })
        .labelled("qualified name")
}

/// `<"definition text">` or nothing.
fn definable<'src>() -> impl Parser<'src, &'src str, Option<String>, AErr<'src>> + Clone {
    sym("<")
        .ignore_then(string_lit())
        .then_ignore(sym(">"))
        .or_not()
}

pub(crate) fn bool_kw<'src>() -> impl Parser<'src, &'src str, bool, AErr<'src>> + Clone {
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
        .ignore_then(segment.clone().repeated().collect::<Vec<_>>())
        .map(|segments| AnnotationPath {
            starts_with_item: true,
            segments,
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

/// Keywords that continue a `[docReference ...]` after the body/corpus/
/// segment reference lists. Xtext lexes them as keyword tokens, so they can
/// never satisfy a `QualifiedName` in the reference lists; the keyword
/// alternatives must win over the greedy pair repetition.
const DOC_REFERENCE_KEYWORDS: &[&str] = &[
    "rationale",
    "rationale_author",
    "structured_provision",
    "provision",
    "reportedField",
];

fn doc_ref_name<'src>() -> impl Parser<'src, &'src str, String, AErr<'src>> + Clone {
    name()
        .try_map(|text: String, span| {
            if DOC_REFERENCE_KEYWORDS.contains(&text.as_str()) {
                Err(Rich::custom(span, "doc reference"))
            } else {
                Ok(text)
            }
        })
        .labelled("doc reference")
}

fn doc_ref_qname<'src>() -> impl Parser<'src, &'src str, QName, AErr<'src>> + Clone {
    doc_ref_name()
        .separated_by(sym("."))
        .at_least(1)
        .collect::<Vec<String>>()
        .map(|parts| QName(parts.join(".")))
        .labelled("qualified name")
}

/// `[docReference for path Body Corpus (Segment "ref")* ...]`
pub fn doc_reference<'src>() -> impl Parser<'src, &'src str, DocReference, AErr<'src>> + Clone {
    let for_clause = kw("for").ignore_then(annotation_path()).or_not();
    let item = doc_ref_qname().then(string_lit().or_not());
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
    element_name()
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

fn data_def<'src>(choice: bool) -> impl Parser<'src, &'src str, DataDef, AErr<'src>> + Clone {
    let header = if choice { kw("choice") } else { kw("type") };
    let body: Boxed<'src, 'src, &'src str, (Vec<AttributeDef>, Vec<ConditionDef>), AErr<'src>> =
        if choice {
            choice_option()
                .repeated()
                .collect::<Vec<_>>()
                .map(|options| (options, Vec::new()))
                .boxed()
        } else {
            attribute()
                .repeated()
                .collect::<Vec<_>>()
                .then(
                    crate::expr::condition_def(kw("condition"), crate::expr::expression())
                        .repeated()
                        .collect::<Vec<_>>(),
                )
                .boxed()
        };
    header
        .ignore_then(name())
        .then(kw("extends").ignore_then(spanned_qname()).or_not())
        .then_ignore(sym(":"))
        .then(definable())
        .then(references_and_annotations())
        .then(body)
        .map_with(
            |((((name, super_type), definition), (docs, annos, _, _)), (members, conditions)),
             ex| {
                let mut span = span_of(ex.span());
                if let Some(last_condition) = conditions.last() {
                    span = span.merge(last_condition.span);
                }
                DataDef {
                    name,
                    super_type,
                    definition,
                    doc_references: docs,
                    annotations: annos,
                    attributes: members,
                    conditions,
                    span,
                }
            },
        )
        .labelled(if choice { "choice" } else { "type" })
}

fn enum_def<'src>() -> impl Parser<'src, &'src str, EnumDef, AErr<'src>> + Clone {
    kw("enum")
        .ignore_then(name())
        .then(kw("extends").ignore_then(spanned_qname()).or_not())
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

/// `(attr: Enum->VALUE)` — the dispatch head of a `FunctionDispatch`.
fn function_dispatch<'src>() -> impl Parser<'src, &'src str, FunctionDispatchDef, AErr<'src>> + Clone
{
    sym("(")
        .ignore_then(name().map_with(|attribute, ex| (attribute, span_of(ex.span()))))
        .then_ignore(sym(":"))
        .then(spanned_qname())
        .then_ignore(sym("->"))
        .then(name().map_with(|value, ex| (value, span_of(ex.span()))))
        .then_ignore(sym(")"))
        .map_with(|((attribute, enumeration), value), _| FunctionDispatchDef {
            attribute: attribute.0,
            attribute_span: attribute.1,
            enumeration: enumeration.name,
            enumeration_span: enumeration.span,
            value: value.0,
            value_span: value.1,
        })
        .labelled("dispatch")
}

/// `[ingest X]` / `[enrich]` / `[projection Y]`. The transform keyword
/// wins over an annotation reference of the same name, matching the
/// grammar's alternative order.
fn transform_annotation<'src>(
) -> impl Parser<'src, &'src str, TransformAnnotationDef, AErr<'src>> + Clone {
    let kind = kw("ingest")
        .to(TransformKind::Ingest)
        .or(kw("enrich").to(TransformKind::Enrich))
        .or(kw("projection").to(TransformKind::Projection));
    sym("[")
        .ignore_then(kind)
        .then(spanned_qname().or_not())
        .then_ignore(sym("]"))
        .map_with(|(kind, reference), ex| TransformAnnotationDef {
            kind,
            reference,
            span: span_of(ex.span()),
        })
        .labelled("transform annotation")
}

/// The `[...]` fragments allowed directly after a `func` header:
/// transform annotations win, then doc references, then annotation refs.
fn func_annotations<'src>() -> impl Parser<
    'src,
    &'src str,
    (
        Vec<TransformAnnotationDef>,
        Vec<DocReference>,
        Vec<AnnotationRef>,
    ),
    AErr<'src>,
> + Clone {
    #[derive(Debug, Clone)]
    enum Fragment {
        Transform(TransformAnnotationDef),
        Doc(DocReference),
        Anno(AnnotationRef),
    }
    choice((
        transform_annotation().map(Fragment::Transform),
        doc_reference().map(Fragment::Doc),
        annotation_ref().map(Fragment::Anno),
    ))
    .repeated()
    .collect::<Vec<_>>()
    .map(|fragments| {
        let mut transform = Vec::new();
        let mut docs = Vec::new();
        let mut annos = Vec::new();
        for fragment in fragments {
            match fragment {
                Fragment::Transform(t) => transform.push(t),
                Fragment::Doc(d) => docs.push(d),
                Fragment::Anno(a) => annos.push(a),
            }
        }
        (transform, docs, annos)
    })
}

/// `alias name: <"...">? expr` (grammar rule `ShortcutDeclaration`).
fn shortcut<'src, P>(expression: P) -> impl Parser<'src, &'src str, ShortcutDef, AErr<'src>> + Clone
where
    P: Parser<'src, &'src str, SpannedExpr, AErr<'src>> + Clone,
{
    kw("alias")
        .ignore_then(name())
        .then_ignore(sym(":"))
        .then(definable())
        .then(expression)
        .map_with(|((name, definition), expression), ex| ShortcutDef {
            name,
            definition,
            expression,
            span: span_of(ex.span()),
        })
        .labelled("alias")
}

/// One `-> feature` step of an operation path.
fn path_segment<'src>() -> impl Parser<'src, &'src str, PathSegmentDef, AErr<'src>> + Clone {
    sym("->")
        .ignore_then(name().map_with(|feature, ex| PathSegmentDef {
            feature,
            span: span_of(ex.span()),
        }))
        .labelled("path segment")
}

/// `set|add root (-> segment)*: <"...">? ExpressionWithAsKey`.
fn operation<'src, P>(
    expression: P,
) -> impl Parser<'src, &'src str, OperationDef, AErr<'src>> + Clone
where
    P: Parser<'src, &'src str, SpannedExpr, AErr<'src>> + Clone,
{
    choice((kw("set").to(false), kw("add").to(true)))
        .then(name().map_with(|assign_root, ex| (assign_root, span_of(ex.span()))))
        .then(path_segment().repeated().collect::<Vec<_>>())
        .then_ignore(sym(":"))
        .then(definable())
        .then(expression)
        .map_with(
            |((((add, assign_root), path), definition), expression), ex| OperationDef {
                add,
                assign_root: assign_root.0,
                assign_root_span: assign_root.1,
                path,
                definition,
                expression,
                span: span_of(ex.span()),
            },
        )
        .labelled("operation")
}

/// `func Name (dispatch)? (extends F)? : <"...">? (transform|refs|annos)*
/// (inputs: attrs+)? (output: attr?)? shortcuts* conditions* operations*
/// postConditions*` — the section order is fixed by the grammar.
fn func_def<'src>() -> impl Parser<'src, &'src str, FunctionDef, AErr<'src>> + Clone {
    #[derive(Debug, Clone)]
    struct Head {
        name: String,
        dispatch: Option<FunctionDispatchDef>,
        super_function: Option<SpannedQName>,
        definition: Option<String>,
        transform: Vec<TransformAnnotationDef>,
        doc_references: Vec<DocReference>,
        annotations: Vec<AnnotationRef>,
    }
    let expression = crate::expr::expression();
    let head = kw("func")
        .ignore_then(name())
        .then(function_dispatch().or_not())
        .then(kw("extends").ignore_then(spanned_qname()).or_not())
        .then_ignore(sym(":"))
        .then(definable())
        .then(func_annotations())
        .map_with(
            |((((name, dispatch), super_function), definition), (transform, docs, annos)), _| {
                Head {
                    name,
                    dispatch,
                    super_function,
                    definition,
                    transform,
                    doc_references: docs,
                    annotations: annos,
                }
            },
        );
    head.then(
        kw("inputs")
            .ignore_then(sym(":"))
            .ignore_then(attribute().repeated().at_least(1).collect::<Vec<_>>())
            .or_not(),
    )
    .then(
        kw("output")
            .ignore_then(sym(":"))
            .ignore_then(attribute())
            .or_not(),
    )
    .then(shortcut(expression.clone()).repeated().collect::<Vec<_>>())
    .then(
        crate::expr::condition_def(kw("condition"), expression.clone())
            .repeated()
            .collect::<Vec<_>>(),
    )
    .then(
        operation(crate::expr::expression_with_as_key())
            .repeated()
            .collect::<Vec<_>>(),
    )
    .then(
        crate::expr::condition_def(crate::parser::word_kw("post-condition"), expression)
            .repeated()
            .collect::<Vec<_>>(),
    )
    .map_with(
        |((((((head, inputs), output), shortcuts), conditions), operations), post_conditions),
         ex| {
            FunctionDef {
                name: head.name,
                dispatch: head.dispatch,
                super_function: head.super_function,
                definition: head.definition,
                transform: head.transform,
                doc_references: head.doc_references,
                annotations: head.annotations,
                inputs: inputs.unwrap_or_default(),
                output,
                shortcuts,
                conditions,
                operations,
                post_conditions,
                span: span_of(ex.span()),
            }
        },
    )
    .labelled("func")
}

/// `reporting rule Name (from T)? : <"...">? docRefs* expr`.
fn rule_def<'src>() -> impl Parser<'src, &'src str, RuleDef, AErr<'src>> + Clone {
    choice((kw("reporting").to(false), kw("eligibility").to(true)))
        .then_ignore(kw("rule"))
        .then(name())
        .then(kw("from").ignore_then(type_call()).or_not())
        .then_ignore(sym(":"))
        .then(definable())
        .then(doc_reference().repeated().collect::<Vec<_>>())
        .then(crate::expr::expression())
        .map_with(
            |(((((eligibility, name), input), definition), doc_references), expression), ex| {
                RuleDef {
                    name,
                    eligibility,
                    input,
                    definition,
                    doc_references,
                    expression,
                    span: span_of(ex.span()),
                }
            },
        )
        .labelled("rule")
}

/// The `real-time | T+1..T+5 | ASATP` timing keyword.
fn report_timing<'src>() -> impl Parser<'src, &'src str, ReportTiming, AErr<'src>> + Clone {
    choice((
        word_kw("real-time").to(ReportTiming::RealTime),
        word_kw("ASATP").to(ReportTiming::Asatp),
        word_kw("T+5").to(ReportTiming::T(5)),
        word_kw("T+4").to(ReportTiming::T(4)),
        word_kw("T+3").to(ReportTiming::T(3)),
        word_kw("T+2").to(ReportTiming::T(2)),
        word_kw("T+1").to(ReportTiming::T(1)),
    ))
    .labelled("report timing")
}

/// `report Body Corpus+ (Segment "ref")* in <timing> from T when R (and R)*
/// with type T (with source S)?`.
///
/// The regulatory reference is a greedy `(name, string)?` sequence — the
/// same shape as a doc reference — re-assembled by shape afterwards: bare
/// names are the body and corpora, names followed by a string are segment
/// references.
fn report_def<'src>() -> impl Parser<'src, &'src str, ReportDef, AErr<'src>> + Clone {
    let item = report_ref_qname().then(string_lit().or_not());
    kw("report")
        .ignore_then(item.repeated().at_least(1).collect::<Vec<_>>())
        .then_ignore(word_kw("in"))
        .then(report_timing())
        .then_ignore(kw("from"))
        .then(type_call())
        .then_ignore(kw("when"))
        .then(
            report_ref_qname()
                .separated_by(kw("and"))
                .at_least(1)
                .collect::<Vec<_>>(),
        )
        .then_ignore(kw("with"))
        .then_ignore(kw("type"))
        .then(report_ref_qname())
        .then(
            kw("with")
                .ignore_then(kw("source"))
                .ignore_then(report_ref_qname())
                .or_not(),
        )
        .try_map(
            |(((((refs, timing), input_type), eligibility_rules), report_type), rule_source),
             span| {
                let mut names = refs.iter().filter(|(_, s)| s.is_none());
                let body = match names.next() {
                    Some((name, _)) => name.clone(),
                    None => return Err(Rich::custom(span, "expected a regulatory body")),
                };
                let mut corpora = Vec::new();
                let mut segments = Vec::new();
                let mut in_segments = false;
                let mut first_name = true;
                for (name, reference) in refs {
                    match reference {
                        Some(reference) => {
                            in_segments = true;
                            segments.push(ReportSegmentRef {
                                segment: name.clone(),
                                reference: reference.clone(),
                            });
                        }
                        None => {
                            if in_segments {
                                return Err(Rich::custom(
                                    span,
                                    "segment references must come last",
                                ));
                            }
                            if first_name {
                                // The first bare name is the body.
                                first_name = false;
                            } else {
                                corpora.push(name.clone());
                            }
                        }
                    }
                }
                Ok((
                    body,
                    corpora,
                    segments,
                    timing,
                    input_type,
                    eligibility_rules,
                    report_type,
                    rule_source,
                ))
            },
        )
        .map_with(
            |(
                body,
                corpora,
                segments,
                timing,
                input_type,
                eligibility_rules,
                report_type,
                rule_source,
            ),
             ex| ReportDef {
                body,
                corpora,
                segments,
                timing,
                input_type,
                eligibility_rules,
                report_type,
                rule_source,
                span: span_of(ex.span()),
            },
        )
        .labelled("report")
}

/// `rule source Name (extends S)? { Data: (+|-) attr [ruleReference R]* }`.
fn rule_source_def<'src>() -> impl Parser<'src, &'src str, RuleSourceDef, AErr<'src>> + Clone {
    let attribute = choice((sym("+").to(true), sym("-").to(false)))
        .then(name().map_with(|attribute, ex| (attribute, span_of(ex.span()))))
        .then(rule_reference().repeated().collect::<Vec<_>>())
        .map_with(
            |((add, attribute), rule_references), ex| ExternalAttributeDef {
                add,
                attribute: attribute.0,
                attribute_span: attribute.1,
                rule_references,
                span: span_of(ex.span()),
            },
        );
    let class = spanned_qname()
        .then_ignore(sym(":"))
        .then(attribute.repeated().collect::<Vec<_>>())
        .map_with(|(data, attributes), ex| ExternalClassDef {
            data,
            attributes,
            span: span_of(ex.span()),
        });
    kw("rule")
        .then_ignore(kw("source"))
        .ignore_then(name())
        .then(kw("extends").ignore_then(spanned_qname()).or_not())
        .then_ignore(sym("{"))
        .then(class.repeated().collect::<Vec<_>>())
        .then_ignore(sym("}"))
        .map_with(|((name, super_source), classes), ex| RuleSourceDef {
            name,
            super_source,
            classes,
            span: span_of(ex.span()),
        })
        .labelled("rule source")
}

/// `schema Name Format <"...">? annotations*` (main-branch grammar; the
/// 9.58.1 oracle has no Schema construct).
fn schema_def<'src>() -> impl Parser<'src, &'src str, SchemaDef, AErr<'src>> + Clone {
    kw("schema")
        .ignore_then(element_name())
        .then(element_name().map_with(|format, ex| (format, span_of(ex.span()))))
        .then(definable())
        .then(annotation_ref().repeated().collect::<Vec<_>>())
        .map_with(
            |(((name, format), definition), annotations), ex| SchemaDef {
                name,
                format: format.0,
                format_span: format.1,
                definition,
                annotations,
                span: span_of(ex.span()),
            },
        )
        .labelled("schema")
}

/// `body Type Name <"...">?`.
fn body_def<'src>() -> impl Parser<'src, &'src str, BodyDef, AErr<'src>> + Clone {
    kw("body")
        .ignore_then(element_name())
        .then(element_name())
        .then(definable())
        .map_with(|((body_type, name), definition), ex| BodyDef {
            name,
            body_type,
            definition,
            span: span_of(ex.span()),
        })
        .labelled("body")
}

/// `corpus Type (Body)? ("display")? Name <"...">?`.
///
/// The optional body/display parts make a PEG-ordered parse commit to the
/// wrong shape (`corpus Reg Ext` would otherwise greedily take `Ext` as a
/// body reference and then fail to find a name). The token sequence after
/// the corpus type is `[qname]? [string]? [qname]`, so it is collected
/// greedily and re-assembled by shape instead.
fn corpus_def<'src>() -> impl Parser<'src, &'src str, CorpusDef, AErr<'src>> + Clone {
    #[derive(Debug, Clone)]
    enum Part {
        Ref(SpannedQName),
        Display(String),
    }
    let part = choice((
        report_ref_qname().map(Part::Ref),
        string_lit().map(Part::Display),
    ));
    kw("corpus")
        .ignore_then(element_name())
        .then(part.repeated().collect::<Vec<_>>())
        .then(definable())
        .try_map(|((corpus_type, parts), definition), span| {
            // Shape: [Ref]? [Display]? [Ref]; the final Ref is the name.
            let parsed = match &parts[..] {
                [Part::Ref(name)] => (None, None, Some(name.clone())),
                [Part::Display(display), Part::Ref(name)] => {
                    (None, Some(display.clone()), Some(name.clone()))
                }
                [Part::Ref(body), Part::Ref(name)] => {
                    (Some(body.clone()), None, Some(name.clone()))
                }
                [Part::Ref(body), Part::Display(display), Part::Ref(name)] => (
                    Some(body.clone()),
                    Some(display.clone()),
                    Some(name.clone()),
                ),
                _ => (None, None, None),
            };
            match parsed {
                (body, display, Some(name)) => {
                    Ok((corpus_type, body, display, name.name.0, definition))
                }
                _ => Err(Rich::custom(span, "expected corpus body and/or name")),
            }
        })
        .map_with(
            |(corpus_type, body, display_name, name, definition), ex| CorpusDef {
                name,
                corpus_type,
                display_name,
                body,
                definition,
                span: span_of(ex.span()),
            },
        )
        .labelled("corpus")
}

/// `segment Name`.
fn segment_decl<'src>() -> impl Parser<'src, &'src str, SegmentDecl, AErr<'src>> + Clone {
    kw("segment")
        .ignore_then(element_name())
        .map_with(|name, ex| SegmentDecl {
            name,
            span: span_of(ex.span()),
        })
        .labelled("segment")
}

/// `metaType Name Type`.
fn meta_type_decl<'src>() -> impl Parser<'src, &'src str, MetaTypeDecl, AErr<'src>> + Clone {
    kw("metaType")
        .ignore_then(element_name())
        .then(type_call())
        .map_with(|(name, type_call), ex| MetaTypeDecl {
            name,
            type_call,
            span: span_of(ex.span()),
        })
        .labelled("metaType")
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
        func_def().map(Element::Function),
        rule_def().map(Element::Rule),
        report_def().map(Element::Report),
        rule_source_def().map(Element::RuleSource),
        data_def(false).map(Element::Data),
        data_def(true).map(Element::Choice),
        enum_def().map(Element::Enumeration),
        annotation_decl().map(Element::Annotation),
        type_alias().map(Element::TypeAlias),
        basic_type().map(Element::BasicType),
        record_type().map(Element::RecordType),
        library_function().map(Element::LibraryFunction),
        schema_def().map(Element::Schema),
        body_def().map(Element::Body),
        corpus_def().map(Element::Corpus),
        segment_decl().map(Element::Segment),
        meta_type_decl().map(Element::MetaType),
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
            root: SpannedQName {
                name: root,
                span: span_of(ex.span()),
            },
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
        let text = "namespace t\ntype A:\n\tx int (1..1)\n\ntype B extends A:\n\toverride x int(digits: 30, max: 100) (1..1)\n";
        let unit = parse_ok(text);
        assert_eq!(unit.elements.len(), 2);
        match &unit.elements[1] {
            Element::Data(d) => {
                // The super-type span must cover the reference text itself.
                let st = d.super_type.as_ref().unwrap();
                assert_eq!(st.name.0, "A");
                assert_eq!(&text[st.span.start..st.span.end], "A");
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
        assert_eq!(unit.configurations[0].root.name.0, "Foo");
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

    /// Phase 4 retired W0001: every construct that used to be skipped now
    /// parses into a modelled element. This test fails if a future grammar
    /// addition is handled by adding a keyword to `UNSUPPORTED_KEYWORDS`
    /// instead of a parser.
    #[test]
    fn no_supported_construct_is_skipped() {
        let text = "\
namespace t
func MyFunc:
	output:
		result int (1..1)
reporting rule MyRule:
	True
eligibility rule MyEligibilityRule from MyType:
	True
body MyBodyType MyBody <\"b\">
corpus MyCorpus MyBody \"c\" MyCorpus
segment MySegment
metaType MyMeta number
report MyBody MyCorpus in T+1 from MyType when MyEligibilityRule with type MyType
type MyType:
	x int (0..1)
rule source MySource {
	MyType:
		+ x
}
";
        let file = SourceFile::new("t.rosetta", text);
        let (unit, diags) = parse(&file);
        assert!(
            diags.iter().all(|d| d.code != "W0001"),
            "no construct may trigger W0001 any more: {diags:?}"
        );
        let unit = unit.unwrap();
        let kinds: Vec<String> = unit
            .elements
            .iter()
            .map(|e| match e {
                Element::Data(_) => "data".to_string(),
                Element::Function(_) => "func".to_string(),
                Element::Rule(_) => "rule".to_string(),
                Element::Report(_) => "report".to_string(),
                Element::RuleSource(_) => "rule source".to_string(),
                Element::Body(_) => "body".to_string(),
                Element::Corpus(_) => "corpus".to_string(),
                Element::Segment(_) => "segment".to_string(),
                Element::MetaType(_) => "metaType".to_string(),
                Element::Unsupported(_) => "UNSUPPORTED".to_string(),
                _ => "other".to_string(),
            })
            .collect();
        assert_eq!(
            kinds,
            vec![
                "func",
                "rule",
                "rule",
                "body",
                "corpus",
                "segment",
                "metaType",
                "report",
                "data",
                "rule source",
            ]
        );
    }

    #[test]
    fn syntax_error_is_reported() {
        let file = SourceFile::new("t.rosetta", "namespace t\ntype Broken\n");
        let (_, diags) = parse(&file);
        assert!(!diags.is_empty());
    }

    #[test]
    fn parses_doc_reference_pair_segments_with_tail_keywords() {
        let unit = parse_ok(
            "namespace t\n\
             body CFTC CFTCBody <\"b\">\n\
             corpus CFTC Part45 <\"p\">\n\
             segment S1\n\
             segment S2\n\
             type Product:\n\
             \t[docReference CFTC Part45 S1 \"r1\" S2 \"r2\" rationale \"why\" provision \"prov text\" reportedField]\n\
             \t[docReference CFTC Part45 rationale_author \"me\" rationale \"why\"]\n\
             \tproductId string (1..1)\n",
        );
        let data = match &unit.elements[4] {
            Element::Data(d) => d,
            other => panic!("expected data, got {other:?}"),
        };
        let doc = &data.doc_references[0];
        assert_eq!(doc.body.0, "CFTC");
        assert_eq!(
            doc.corpora.iter().map(|c| c.0.clone()).collect::<Vec<_>>(),
            vec!["Part45"]
        );
        assert_eq!(
            doc.segments,
            vec![
                ("S1".to_string(), "r1".to_string()),
                ("S2".to_string(), "r2".to_string()),
            ]
        );
        assert_eq!(doc.rationales.len(), 1);
        assert_eq!(doc.rationales[0].rationale.as_deref(), Some("why"));
        assert_eq!(doc.rationales[0].rationale_author, None);
        assert_eq!(doc.structured_provision, None);
        assert_eq!(doc.provision.as_deref(), Some("prov text"));
        assert!(doc.reported_field);

        let doc = &data.doc_references[1];
        assert_eq!(
            doc.segments,
            Vec::<(String, String)>::new(),
            "tail keywords must not be eaten as segment pairs"
        );
        assert_eq!(doc.rationales.len(), 1);
        assert_eq!(doc.rationales[0].rationale.as_deref(), Some("why"));
        assert_eq!(doc.rationales[0].rationale_author.as_deref(), Some("me"));
        assert!(!doc.reported_field);
    }

    #[test]
    fn doc_reference_bare_string_segments_fail_to_parse() {
        let file = SourceFile::new(
            "t.rosetta",
            "namespace t\n\
             body CFTC CFTCBody <\"b\">\n\
             corpus CFTC Part45 <\"p\">\n\
             segment S1\n\
             segment S2\n\
             type Product:\n\
             \t[docReference CFTC Part45 \"S1\" \"S2\"]\n\
             \tproductId string (1..1)\n",
        );
        let (_, diags) = parse(&file);
        assert!(
            diags.iter().any(|d| d.code == "E0001"),
            "bare-string segments must stay a syntax error (oracle-correct): {diags:?}"
        );
    }

    #[test]
    fn parses_doc_reference_for_path() {
        let unit = parse_ok(
            "namespace t\n\
             body CFTC CFTCBody <\"b\">\n\
             corpus CFTC Part45 <\"p\">\n\
             segment S1\n\
             type Product:\n\
             \tproductId string (1..1)\n\
             \t\t[docReference for productId->id CFTC Part45 S1 \"S1\" reportedField]\n",
        );
        match &unit.elements[3] {
            Element::Data(d) => {
                let doc = &d.attributes[0].doc_references[0];
                let path = doc.for_path.as_ref().unwrap();
                assert!(!path.starts_with_item);
                assert_eq!(path.segments.len(), 2);
                assert_eq!(path.segments[0].attribute, "productId");
                assert!(!path.segments[0].deep);
                assert_eq!(path.segments[1].attribute, "id");
                assert_eq!(doc.body.0, "CFTC");
                assert_eq!(doc.segments, vec![("S1".to_string(), "S1".to_string())]);
                assert!(doc.reported_field);
            }
            other => panic!("expected data, got {other:?}"),
        }
    }
}
