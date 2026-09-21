//! Expression grammar (a direct transcription of the *Expressions*
//! section of `docs/reference/Rosetta.xtext`).
//!
//! Binding is bottom-up, loosest first:
//!
//! `then` chains → `or` → `and` → equality (`=`, `<>`) → comparison
//! (`>= <= > <`) → additive (`+ -`) → multiplicative (`* /`) → binary word
//! operations (`contains disjoint default join`) → postfix/unary →
//! primary.
//!
//! Each grammar level is a [`chumsky::Recursive`] parser over the same
//! spanned [`SpannedExpr`] output so the mutually recursive rules can be
//! declared up front. Word operators containing hyphens (`one-of`,
//! `to-date-time`, ...) are Xtext keyword tokens rather than `ID`s, so
//! they get a character-based keyword matcher with a right-hand boundary
//! check, and the longer forms are tried first (`to-date-time` before
//! `to-date`), mirroring the lexer's longest-match behaviour.

use chumsky::prelude::*;
use sigil_diag::Span;

use crate::ast::*;
use crate::parser::{
    bool_kw, int_lit_raw, kw, name, number_lit, qname, span_of, string_lit, sym, type_call, ws,
    AErr,
};

type E<'src> = AErr<'src>;

/// Identifiers that are keyword tokens of expression operators. In Xtext
/// these are *not* `ID`s (only the `ValidID` whitelist words are), so a
/// cross-reference can never consume them; the expression parser mirrors
/// that so expressions terminate correctly at word operators.
const EXPRESSION_KEYWORDS: &[&str] = &[
    "then", "and", "or", "contains", "disjoint", "default", "join", "exists", "single", "multiple",
    "is", "absent", "count", "flatten", "distinct", "reverse", "first", "last", "sum", "min",
    "max", "sort", "choice", "switch", "as", "filter", "extract", "reduce", "if", "else", "True",
    "False", "empty", "item", "only", "required", "optional", "any", "all",
];

/// An identifier usable as a symbol / feature / key inside expressions.
fn expr_name<'src>() -> impl Parser<'src, &'src str, String, E<'src>> + Clone {
    name().try_map(|text: String, span| {
        if EXPRESSION_KEYWORDS.contains(&text.as_str()) {
            Err(Rich::custom(span, format!("'{text}' is a keyword here")))
        } else {
            Ok(text)
        }
    })
}

/// Qualified name over [`expr_name`].
fn expr_qname<'src>() -> impl Parser<'src, &'src str, QName, E<'src>> + Clone {
    expr_name()
        .separated_by(sym("."))
        .at_least(1)
        .collect::<Vec<String>>()
        .map(|parts| QName(parts.join(".")))
        .labelled("qualified name")
}

fn node(kind: ExprKind, span: Span) -> SpannedExpr {
    SpannedExpr { kind, span }
}

/// A keyword containing hyphens (e.g. `one-of`, `to-date-time`). These are
/// Xtext keyword tokens, not `ID`s, so the matcher is character-based and
/// requires a non-identifier right boundary.
fn op_kw<'src>(s: &'static str) -> Boxed<'src, 'src, &'src str, (), E<'src>> {
    let boundary = any()
        .filter(|c: &char| c.is_ascii_alphanumeric() || *c == '_')
        .not();
    just(s)
        .to(())
        .then_ignore(boundary)
        .then_ignore(ws())
        .labelled(s.to_string())
        .boxed()
}

/// `Necessity` (`required` | `optional`).
fn necessity<'src>() -> impl Parser<'src, &'src str, AstNecessity, E<'src>> + Clone {
    kw("required")
        .to(AstNecessity::Required)
        .or(kw("optional").to(AstNecessity::Optional))
        .labelled("necessity")
}

/// `CardinalityModifier` (`any` | `all`).
fn card_mod<'src>() -> impl Parser<'src, &'src str, AstCardinalityMod, E<'src>> + Clone {
    kw("any")
        .to(AstCardinalityMod::Any)
        .or(kw("all").to(AstCardinalityMod::All))
        .labelled("cardinality modifier")
}

/// `SwitchCaseOrDefault`.
fn switch_case<'src>(
    expression: impl Parser<'src, &'src str, SpannedExpr, E<'src>> + Clone,
) -> impl Parser<'src, &'src str, AstSwitchCase, E<'src>> + Clone {
    let guard = choice((
        bool_kw()
            .map_with(|v, ex| node(ExprKind::Boolean(v), span_of(ex.span())))
            .map(|expr| AstSwitchGuard::Literal(Box::new(expr))),
        string_lit()
            .map_with(|v, ex| node(ExprKind::Str(v), span_of(ex.span())))
            .map(|expr| AstSwitchGuard::Literal(Box::new(expr))),
        number_lit()
            .map_with(|v, ex| node(ExprKind::Number(v), span_of(ex.span())))
            .map(|expr| AstSwitchGuard::Literal(Box::new(expr))),
        int_lit_raw()
            .map_with(|v, ex| node(ExprKind::Int(v), span_of(ex.span())))
            .map(|expr| AstSwitchGuard::Literal(Box::new(expr))),
        expr_qname().map(AstSwitchGuard::Reference),
    ));
    choice((
        kw("default")
            .ignore_then(expression.clone())
            .map(|expression| AstSwitchCase {
                guard: None,
                expression,
            }),
        guard
            .then_ignore(kw("then"))
            .then(expression)
            .map(|(guard, expression)| AstSwitchCase {
                guard: Some(guard),
                expression,
            }),
    ))
    .labelled("switch case")
}

/// `InlineFunction`: `(params)? '[' body ']'` — in the 9.58.1 grammar the
/// closure parameters sit *before* the bracket (`a, b [a + b]`); a bracket
/// directly after the operator is the parameter-less form (`[a + b]`).
fn explicit_inline_function<'src>(
    expression: impl Parser<'src, &'src str, SpannedExpr, E<'src>> + Clone,
) -> impl Parser<'src, &'src str, AstInlineFunction, E<'src>> + Clone {
    // Closure parameters are plain `ID`s: keywords (`filter`, `sort`, ...)
    // can never be parameter names.
    expr_name()
        .separated_by(sym(","))
        .at_least(1)
        .collect::<Vec<_>>()
        .then_ignore(sym("["))
        .map(Some)
        .or(sym("[").to(None))
        .then(expression)
        .then_ignore(sym("]"))
        .map(|(parameters, body)| AstInlineFunction {
            parameters: parameters.unwrap_or_default(),
            body: Box::new(body),
        })
        .labelled("inline function")
}

/// `condition RosettaNamed? ':' RosettaDefinable? (References|Annotations)* Expression`
/// (or, with the `post-condition` head, `PostCondition`).
///
/// The doc-reference/annotation fragments are consumed *before* the
/// expression, mirroring the grammar's syntactic predicate
/// (`=>(References|Annotations)*`) that resolves the `[` ambiguity in
/// favour of annotations.
pub fn condition_def<'src, P>(
    head: impl Parser<'src, &'src str, (), E<'src>> + Clone,
    expression: P,
) -> impl Parser<'src, &'src str, ConditionDef, E<'src>> + Clone
where
    P: Parser<'src, &'src str, SpannedExpr, E<'src>> + Clone,
{
    let fragment = choice((
        crate::parser::doc_reference().map(|d| (vec![d], Vec::new())),
        crate::parser::annotation_ref().map(|a| (Vec::new(), vec![a])),
    ));
    head.ignore_then(name().or_not())
        .then_ignore(sym(":"))
        .then(
            sym("<")
                .ignore_then(string_lit())
                .then_ignore(sym(">"))
                .or_not(),
        )
        .then(fragment.repeated().collect::<Vec<_>>())
        .then(expression)
        .map_with(|(((name, definition), fragments), expression), ex| {
            let mut doc_references = Vec::new();
            let mut annotations = Vec::new();
            for (docs, annos) in fragments {
                doc_references.extend(docs);
                annotations.extend(annos);
            }
            ConditionDef {
                name,
                definition,
                doc_references,
                annotations,
                expression,
                span: span_of(ex.span()),
            }
        })
        .labelled("condition")
}

/// `ExpressionWithAsKey`: an expression with an optional trailing `as-key`.
pub fn expression_with_as_key<'src>() -> impl Parser<'src, &'src str, SpannedExpr, E<'src>> + Clone
{
    expression()
        .then(op_kw("as-key").to(()).or_not())
        .map(|(expr, as_key)| {
            let span = expr.span;
            let kind = if as_key.is_some() {
                ExprKind::AsKey {
                    argument: Box::new(expr),
                }
            } else {
                expr.kind
            };
            SpannedExpr { kind, span }
        })
        .labelled("expression")
}

/// Parse a standalone expression (used by tests and tools; the grammar is
/// identical to condition bodies).
pub fn parse_expression_str(text: &str) -> Result<SpannedExpr, Vec<sigil_diag::Diagnostic>> {
    let file = sigil_diag::SourceFile::new("<expr>", text);
    let inner = std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(move || {
            let out = expression().then_ignore(end()).parse(file.text.as_str());
            out.into_result().map_err(|errors| {
                errors
                    .into_iter()
                    .map(|e| {
                        let span = Span::new(e.span().start, e.span().end);
                        sigil_diag::Diagnostic::error("E0001", format!("{e}"), &file, span)
                    })
                    .collect::<Vec<_>>()
            })
        })
        .expect("failed to spawn parser thread")
        .join()
        .expect("parser thread panicked");
    inner
}

/// One postfix step (`UnaryOperation` continuation alternatives). Each
/// step carries the span of its own text so the applied node can cover
/// `argument + step`.
#[derive(Clone)]
enum Step {
    Feature { feature: Option<String>, deep: bool },
    Exists(Option<AstExistsMod>),
    Absent,
    Simple(AstUnaryOp),
    Choice(AstNecessity, Vec<String>),
    ToEnum(QName),
    Switch(Vec<AstSwitchCase>),
    WithMeta(Vec<(String, SpannedExpr)>),
    As(QName),
    Functional(AstFunctionalOp, Option<AstInlineFunction>),
}

#[derive(Debug, Clone, Copy)]
enum AstUnaryOp {
    OnlyElement,
    Count,
    Flatten,
    Distinct,
    Reverse,
    First,
    Last,
    Sum,
    OneOf,
    ToString,
    ToNumber,
    ToInt,
    ToTime,
    ToDate,
    ToDateTime,
    ToZonedDateTime,
}

impl Step {
    fn apply(self, argument: SpannedExpr, step_span: Span) -> SpannedExpr {
        let span = argument.span.merge(step_span);
        let argument = || Box::new(argument);
        let kind = match self {
            Step::Feature { feature, deep } => ExprKind::FeatureCall {
                receiver: argument(),
                feature,
                deep,
            },
            Step::Exists(modifier) => ExprKind::Exists {
                modifier,
                argument: argument(),
            },
            Step::Absent => ExprKind::Absent {
                argument: argument(),
            },
            Step::Simple(op) => {
                use AstUnaryOp::*;
                match op {
                    OnlyElement => ExprKind::OnlyElement {
                        argument: argument(),
                    },
                    Count => ExprKind::Count {
                        argument: argument(),
                    },
                    Flatten => ExprKind::Flatten {
                        argument: argument(),
                    },
                    Distinct => ExprKind::Distinct {
                        argument: argument(),
                    },
                    Reverse => ExprKind::Reverse {
                        argument: argument(),
                    },
                    First => ExprKind::First {
                        argument: argument(),
                    },
                    Last => ExprKind::Last {
                        argument: argument(),
                    },
                    Sum => ExprKind::Sum {
                        argument: argument(),
                    },
                    OneOf => ExprKind::OneOf {
                        argument: argument(),
                    },
                    ToString => ExprKind::ToString {
                        argument: argument(),
                    },
                    ToNumber => ExprKind::ToNumber {
                        argument: argument(),
                    },
                    ToInt => ExprKind::ToInt {
                        argument: argument(),
                    },
                    ToTime => ExprKind::ToTime {
                        argument: argument(),
                    },
                    ToDate => ExprKind::ToDate {
                        argument: argument(),
                    },
                    ToDateTime => ExprKind::ToDateTime {
                        argument: argument(),
                    },
                    ToZonedDateTime => ExprKind::ToZonedDateTime {
                        argument: argument(),
                    },
                }
            }
            Step::Choice(necessity, attributes) => ExprKind::Choice {
                necessity,
                attributes,
                argument: argument(),
            },
            Step::ToEnum(enumeration) => ExprKind::ToEnum {
                enumeration,
                argument: argument(),
            },
            Step::Switch(cases) => ExprKind::Switch {
                argument: argument(),
                cases,
            },
            Step::WithMeta(entries) => ExprKind::WithMeta {
                argument: argument(),
                entries,
            },
            Step::As(type_) => ExprKind::As {
                type_,
                argument: argument(),
            },
            Step::Functional(op, function) => ExprKind::Functional {
                op,
                argument: argument(),
                function,
            },
        };
        SpannedExpr { kind, span }
    }
}

/// The full expression grammar; returns the top-level `Expression`
/// (`ThenOperation`) parser.
pub fn expression<'src>() -> impl Parser<'src, &'src str, SpannedExpr, E<'src>> + Clone {
    let mut then_p = Recursive::declare();
    let mut or_p = Recursive::declare();
    let mut and_p = Recursive::declare();
    let mut eq_p = Recursive::declare();
    let mut cmp_p = Recursive::declare();
    let mut add_p = Recursive::declare();
    let mut mul_p = Recursive::declare();
    let mut word_p = Recursive::declare();
    let mut unary_p = Recursive::declare();
    let mut primary_p = Recursive::declare();

    // ThenOperation := OrOperation ( 'then' ImplicitInlineFunction? )*
    then_p.define(or_p.clone().foldl(
        kw("then").ignore_then(or_p.clone().or_not()).repeated(),
        |argument: SpannedExpr, body: Option<SpannedExpr>| {
            let span = body
                .as_ref()
                .map_or(argument.span, |b| argument.span.merge(b.span));
            SpannedExpr {
                kind: ExprKind::Functional {
                    op: AstFunctionalOp::Then,
                    argument: Box::new(argument),
                    function: body.map(|body| AstInlineFunction {
                        parameters: Vec::new(),
                        body: Box::new(body),
                    }),
                },
                span,
            }
        },
    ));

    // OrOperation / AndOperation. Both levels also have the grammar's
    // "without left parameter" form (`or x`), whose missing left side is
    // the generated implicit variable.
    or_p.define(
        binary_level(and_p.clone(), kw("or"), |span, left, right| SpannedExpr {
            kind: ExprKind::Logical {
                op: AstLogicOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            },
            span,
        })
        .or(binary_level_no_left(
            kw("or"),
            and_p.clone(),
            |span, (), left, right| SpannedExpr {
                kind: ExprKind::Logical {
                    op: AstLogicOp::Or,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            },
        )),
    );
    and_p.define(
        binary_level(eq_p.clone(), kw("and"), |span, left, right| SpannedExpr {
            kind: ExprKind::Logical {
                op: AstLogicOp::And,
                left: Box::new(left),
                right: Box::new(right),
            },
            span,
        })
        .or(binary_level_no_left(
            kw("and"),
            eq_p.clone(),
            |span, (), left, right| SpannedExpr {
                kind: ExprKind::Logical {
                    op: AstLogicOp::And,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            },
        )),
    );

    // EqualityOperation / ComparisonOperation (optional any/all modifier).
    let eq_op = choice((sym("<>").to(AstEqOp::NotEq), sym("=").to(AstEqOp::Eq)));
    let cmp_op = choice((
        sym(">=").to(AstCmpOp::Ge),
        sym("<=").to(AstCmpOp::Le),
        sym(">").to(AstCmpOp::Gt),
        sym("<").to(AstCmpOp::Lt),
    ));
    eq_p.define(
        modifiable_level(
            cmp_p.clone(),
            eq_op.clone(),
            |span, op, card_mod, left, right| SpannedExpr {
                kind: ExprKind::Equality {
                    op,
                    card_mod,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            },
        )
        .or(binary_level_no_left(
            card_mod().or_not().then(eq_op.clone()),
            cmp_p.clone(),
            |span, (card_mod, op), left, right| SpannedExpr {
                kind: ExprKind::Equality {
                    op,
                    card_mod,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            },
        )),
    );
    cmp_p.define(
        modifiable_level(
            add_p.clone(),
            cmp_op.clone(),
            |span, op, card_mod, left, right| SpannedExpr {
                kind: ExprKind::Comparison {
                    op,
                    card_mod,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            },
        )
        .or(binary_level_no_left(
            card_mod().or_not().then(cmp_op.clone()),
            add_p.clone(),
            |span, (card_mod, op), left, right| SpannedExpr {
                kind: ExprKind::Comparison {
                    op,
                    card_mod,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            },
        )),
    );

    // AdditiveOperation / MultiplicativeOperation.
    add_p.define(binary_level_with_op(
        mul_p.clone(),
        choice((
            sym("+").to(AstArithOp::Add),
            sym("-").to(AstArithOp::Subtract),
        )),
        |span, op, left, right| SpannedExpr {
            kind: ExprKind::Arithmetic {
                op,
                left: Box::new(left),
                right: Box::new(right),
            },
            span,
        },
    ));
    let mul_op = choice((
        sym("*").to(AstArithOp::Multiply),
        sym("/").to(AstArithOp::Divide),
    ));
    mul_p.define(
        binary_level_with_op(word_p.clone(), mul_op.clone(), |span, op, left, right| {
            SpannedExpr {
                kind: ExprKind::Arithmetic {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            }
        })
        .or(binary_level_no_left(
            mul_op,
            word_p.clone(),
            |span, op, left, right| SpannedExpr {
                kind: ExprKind::Arithmetic {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            },
        )),
    );

    // BinaryOperation: contains / disjoint / default / join (join's
    // separator is optional; lowering inserts the generated `""` literal
    // the EMF model uses).
    let word_step = choice((
        kw("contains")
            .ignore_then(unary_p.clone())
            .map(|right: SpannedExpr| (WordOp::Contains, Some(right))),
        kw("disjoint")
            .ignore_then(unary_p.clone())
            .map(|right: SpannedExpr| (WordOp::Disjoint, Some(right))),
        kw("default")
            .ignore_then(unary_p.clone())
            .map(|right: SpannedExpr| (WordOp::Default, Some(right))),
        // The separator is optional; the with-separator form must be tried
        // first (the grammar's `=>` syntactic predicate).
        kw("join")
            .ignore_then(unary_p.clone())
            .map(|right: SpannedExpr| (WordOp::Join, Some(right)))
            .or(kw("join").map(|_| (WordOp::Join, None))),
    ));
    word_p.define(
        unary_p
            .clone()
            .foldl(
                word_step.clone().repeated(),
                |left, (op, right): (WordOp, Option<SpannedExpr>)| {
                    let span = right
                        .as_ref()
                        .map_or(left.span, |r| left.span.merge(r.span));
                    make_word_node(op, left, right, span)
                },
            )
            .or(empty().to(missing_left()).foldl(
                word_step.repeated().at_least(1),
                |left, (op, right): (WordOp, Option<SpannedExpr>)| {
                    let span = right
                        .as_ref()
                        .map_or(left.span, |r| left.span.merge(r.span));
                    make_word_node(op, left, right, span)
                },
            )),
    );

    // UnaryOperation: primary followed by postfix steps.
    let feature_step = choice((sym("->>").to(true), sym("->").to(false)))
        .then(expr_name().or_not())
        .map(|(deep, feature)| Step::Feature { feature, deep });
    let exists_step = kw("single")
        .to(AstExistsMod::Single)
        .or(kw("multiple").to(AstExistsMod::Multiple))
        .or_not()
        .then_ignore(kw("exists"))
        .map(Step::Exists);
    let simple = |op: AstUnaryOp, keyword: &'static str| {
        // Hyphenated operators are keyword tokens, not identifiers.
        if keyword.contains('-') {
            op_kw(keyword).to(Step::Simple(op)).boxed()
        } else {
            kw(keyword).to(Step::Simple(op)).boxed()
        }
    };
    let functional_step = |op: AstFunctionalOp, keyword: &'static str, implicit: bool| {
        let explicit: Boxed<'src, 'src, &'src str, Option<AstInlineFunction>, E<'src>> =
            explicit_inline_function(then_p.clone()).map(Some).boxed();
        let function: Boxed<'src, 'src, &'src str, Option<AstInlineFunction>, E<'src>> = if implicit
        {
            explicit
                .or(or_p.clone().map(|body: SpannedExpr| {
                    Some(AstInlineFunction {
                        parameters: Vec::new(),
                        body: Box::new(body),
                    })
                }))
                .boxed()
        } else {
            explicit
        };
        op_kw(keyword)
            .ignore_then(function.or_not().map(Option::flatten))
            .map(move |function| Step::Functional(op, function))
    };
    let mut arms: Vec<Boxed<'src, 'src, &'src str, Step, E<'src>>> = Vec::new();
    let mut push = |parser: Boxed<'src, 'src, &'src str, Step, E<'src>>| arms.push(parser);
    push(feature_step.boxed());
    push(exists_step.boxed());
    push(kw("is").ignore_then(kw("absent")).to(Step::Absent).boxed());
    push(simple(AstUnaryOp::OnlyElement, "only-element").boxed());
    push(simple(AstUnaryOp::Count, "count").boxed());
    push(simple(AstUnaryOp::Flatten, "flatten").boxed());
    push(simple(AstUnaryOp::Distinct, "distinct").boxed());
    push(simple(AstUnaryOp::Reverse, "reverse").boxed());
    push(simple(AstUnaryOp::First, "first").boxed());
    push(simple(AstUnaryOp::Last, "last").boxed());
    push(simple(AstUnaryOp::Sum, "sum").boxed());
    push(simple(AstUnaryOp::OneOf, "one-of").boxed());
    push(
        necessity()
            .then_ignore(kw("choice"))
            .then(name().separated_by(sym(",")).collect::<Vec<_>>())
            .map(|(necessity, attributes)| Step::Choice(necessity, attributes))
            .boxed(),
    );
    push(simple(AstUnaryOp::ToString, "to-string").boxed());
    push(simple(AstUnaryOp::ToNumber, "to-number").boxed());
    push(simple(AstUnaryOp::ToInt, "to-int").boxed());
    push(simple(AstUnaryOp::ToTime, "to-time").boxed());
    push(
        op_kw("to-zoned-date-time")
            .to(Step::Simple(AstUnaryOp::ToZonedDateTime))
            .boxed(),
    );
    push(
        op_kw("to-date-time")
            .to(Step::Simple(AstUnaryOp::ToDateTime))
            .boxed(),
    );
    push(
        op_kw("to-date")
            .to(Step::Simple(AstUnaryOp::ToDate))
            .boxed(),
    );
    push(
        op_kw("to-enum")
            .ignore_then(qname())
            .map(Step::ToEnum)
            .boxed(),
    );
    push(
        op_kw("switch")
            .ignore_then(
                switch_case(then_p.clone())
                    .separated_by(sym(","))
                    .collect::<Vec<_>>(),
            )
            .map(Step::Switch)
            .boxed(),
    );
    push(
        op_kw("with-meta")
            .ignore_then(
                sym("{")
                    .ignore_then(
                        name()
                            .then_ignore(sym(":"))
                            .then(then_p.clone())
                            .separated_by(sym(","))
                            .collect::<Vec<_>>(),
                    )
                    .then_ignore(sym("}"))
                    .or_not(),
            )
            .map(|entries| Step::WithMeta(entries.unwrap_or_default()))
            .boxed(),
    );
    push(kw("as").ignore_then(expr_qname()).map(Step::As).boxed());
    push(functional_step(AstFunctionalOp::Sort, "sort", false).boxed());
    push(functional_step(AstFunctionalOp::Min, "min", false).boxed());
    push(functional_step(AstFunctionalOp::Max, "max", false).boxed());
    push(functional_step(AstFunctionalOp::Reduce, "reduce", true).boxed());
    push(functional_step(AstFunctionalOp::Filter, "filter", true).boxed());
    push(functional_step(AstFunctionalOp::Extract, "extract", true).boxed());
    // Left-to-right priority via a right fold of `or` (Xtext alternatives
    // are ordered).
    let postfix_step = arms
        .into_iter()
        .rev()
        .reduce(|acc, arm| arm.or(acc).boxed())
        .expect("postfix steps")
        .map_with(|step, ex| (step, span_of(ex.span())));
    // The grammar's "without left parameter" unary form: an operator chain
    // with no receiver (`filter x`, `sort [...]`, `exists`). The missing
    // argument is the generated implicit variable, and further postfix
    // steps chain onto it like any other argument.
    let unary_no_left = empty()
        .to(missing_left())
        .foldl(
            postfix_step.clone().repeated().at_least(1),
            |argument, (step, step_span)| step.apply(argument, step_span),
        )
        .labelled("operator-first expression");
    unary_p.define(
        primary_p
            .clone()
            .foldl(postfix_step.repeated(), |argument, (step, step_span)| {
                step.apply(argument, step_span)
            })
            .or(unary_no_left),
    );

    primary_p.define(primary(then_p.clone().boxed(), or_p.clone().boxed()));

    then_p.clone()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WordOp {
    Contains,
    Disjoint,
    Default,
    Join,
}

/// Build one word-binary node (`contains` / `disjoint` / `default` /
/// `join`, the separator of a join being optional).
fn make_word_node(
    op: WordOp,
    left: SpannedExpr,
    right: Option<SpannedExpr>,
    span: Span,
) -> SpannedExpr {
    let kind = match op {
        WordOp::Contains => ExprKind::Contains {
            left: Box::new(left),
            right: Box::new(right.unwrap()),
        },
        WordOp::Disjoint => ExprKind::Disjoint {
            left: Box::new(left),
            right: Box::new(right.unwrap()),
        },
        WordOp::Default => ExprKind::Default {
            left: Box::new(left),
            right: Box::new(right.unwrap()),
        },
        WordOp::Join => ExprKind::Join {
            left: Box::new(left),
            right: right.map(Box::new),
        },
    };
    SpannedExpr { kind, span }
}

/// The synthesized receiver of the grammar's "without left parameter"
/// alternatives: the EMF derived state fills a generated implicit `item`.
fn missing_left() -> SpannedExpr {
    SpannedExpr {
        kind: ExprKind::Item,
        span: Span::default(),
    }
}

/// The "without left parameter" shape of a binary level: `op right
/// (op right)*`, with the implicit variable standing in as the left side.
fn binary_level_no_left<'src, P, O, T, F>(
    op: O,
    next: P,
    make: F,
) -> impl Parser<'src, &'src str, SpannedExpr, E<'src>> + Clone
where
    P: Parser<'src, &'src str, SpannedExpr, E<'src>> + Clone,
    O: Parser<'src, &'src str, T, E<'src>> + Clone,
    F: Fn(Span, T, SpannedExpr, SpannedExpr) -> SpannedExpr + Clone + 'src,
{
    op.clone()
        .then(next.clone())
        .map_with({
            let make = make.clone();
            move |(op_value, right), ex| {
                let left = missing_left();
                make(span_of(ex.span()), op_value, left, right)
            }
        })
        .foldl(
            ws().ignore_then(op.then(next)).repeated(),
            move |left, (op_value, right)| {
                let span = left.span.merge(right.span);
                make(span, op_value, left, right)
            },
        )
        .labelled("operator-first expression")
}

/// `A (op A)*` with a fixed operator (or / and).
fn binary_level<'src, P, F>(
    next: P,
    op: impl Parser<'src, &'src str, (), E<'src>> + Clone,
    make: F,
) -> impl Parser<'src, &'src str, SpannedExpr, E<'src>> + Clone
where
    P: Parser<'src, &'src str, SpannedExpr, E<'src>> + Clone,
    F: Fn(Span, SpannedExpr, SpannedExpr) -> SpannedExpr + Clone,
{
    next.clone().foldl(
        ws().ignore_then(op.clone().ignore_then(next)).repeated(),
        move |left, right| {
            let span = left.span.merge(right.span);
            make(span, left, right)
        },
    )
}

/// `A (op A)*` where the operator carries its own identity (+/-, */).
fn binary_level_with_op<'src, P, O, F>(
    next: P,
    op: O,
    make: F,
) -> impl Parser<'src, &'src str, SpannedExpr, E<'src>> + Clone
where
    P: Parser<'src, &'src str, SpannedExpr, E<'src>> + Clone,
    O: Parser<'src, &'src str, AstArithOp, E<'src>> + Clone,
    F: Fn(Span, AstArithOp, SpannedExpr, SpannedExpr) -> SpannedExpr + Clone,
{
    next.clone().foldl(
        ws().ignore_then(op.then(next)).repeated(),
        move |left, (op, right)| {
            let span = left.span.merge(right.span);
            make(span, op, left, right)
        },
    )
}

/// `A ((cardMod)? op A)*` (equality / comparison).
fn modifiable_level<'src, P, O, OpT, F>(
    next: P,
    op: O,
    make: F,
) -> impl Parser<'src, &'src str, SpannedExpr, E<'src>> + Clone
where
    P: Parser<'src, &'src str, SpannedExpr, E<'src>> + Clone,
    O: Parser<'src, &'src str, OpT, E<'src>> + Clone,
    F: Fn(Span, OpT, Option<AstCardinalityMod>, SpannedExpr, SpannedExpr) -> SpannedExpr + Clone,
{
    next.clone().foldl(
        ws().ignore_then(card_mod().or_not().then(op).then(next))
            .repeated(),
        move |left, ((card_mod, op), right)| {
            let span = left.span.merge(right.span);
            make(span, op, card_mod, left, right)
        },
    )
}

/// `RosettaOnlyExistsElement`: a symbol reference or `item`, followed by
/// `->feature` chains.
fn only_exists_segment<'src>() -> impl Parser<'src, &'src str, SpannedExpr, E<'src>> + Clone {
    choice((kw("item").to(None), expr_qname().map(Some)))
        .then(
            sym("->")
                .ignore_then(expr_name())
                .repeated()
                .collect::<Vec<_>>(),
        )
        .map_with(|(root, features), ex| {
            let span = span_of(ex.span());
            let mut expr = match root {
                Some(name) => SpannedExpr {
                    kind: ExprKind::Symbol {
                        name,
                        explicit_args: false,
                        args: Vec::new(),
                    },
                    span,
                },
                None => SpannedExpr {
                    kind: ExprKind::Item,
                    span,
                },
            };
            for feature in features {
                expr = SpannedExpr {
                    kind: ExprKind::FeatureCall {
                        receiver: Box::new(expr),
                        feature: Some(feature),
                        deep: false,
                    },
                    span,
                };
            }
            expr
        })
        .labelled("only-exists argument")
}

/// `PrimaryExpression`.
fn primary<'src>(
    expression: Boxed<'src, 'src, &'src str, SpannedExpr, E<'src>>,
    or_expression: Boxed<'src, 'src, &'src str, SpannedExpr, E<'src>>,
) -> Boxed<'src, 'src, &'src str, SpannedExpr, E<'src>> {
    // `ExpressionWithAsKey` (constructor values).
    let with_as_key = expression
        .clone()
        .then(op_kw("as-key").to(()).or_not())
        .map(|(expr, as_key)| {
            let span = expr.span;
            let kind = if as_key.is_some() {
                ExprKind::AsKey {
                    argument: Box::new(expr),
                }
            } else {
                expr.kind
            };
            SpannedExpr { kind, span }
        });

    // `RosettaCalcOnlyExists`.
    let only_exists = choice((
        only_exists_segment()
            .separated_by(sym(","))
            .at_least(1)
            .collect::<Vec<_>>()
            .delimited_by(sym("("), sym(")"))
            .then_ignore(kw("only"))
            .then_ignore(kw("exists"))
            .map_with(|args, ex| {
                node(
                    ExprKind::OnlyExists {
                        args,
                        has_parentheses: true,
                    },
                    span_of(ex.span()),
                )
            }),
        only_exists_segment()
            .then_ignore(kw("only"))
            .then_ignore(kw("exists"))
            .map_with(|arg, ex| {
                node(
                    ExprKind::OnlyExists {
                        args: vec![arg],
                        has_parentheses: false,
                    },
                    span_of(ex.span()),
                )
            }),
    ))
    .labelled("only-exists");

    // `RosettaCalcConditionalExpression`: branches are `OrOperation`s.
    let conditional = kw("if")
        .ignore_then(or_expression.clone())
        .then_ignore(kw("then"))
        .then(or_expression.clone())
        .then(kw("else").ignore_then(or_expression).or_not())
        .map_with(|((if_, ifthen), elsethen), ex| {
            node(
                ExprKind::Conditional {
                    if_: Box::new(if_),
                    ifthen: Box::new(ifthen),
                    elsethen: elsethen.map(Box::new),
                },
                span_of(ex.span()),
            )
        })
        .labelled("conditional");

    // `ConstructorExpression`.
    let constructor_value = expr_name()
        .then_ignore(sym(":"))
        .then(with_as_key)
        .map(|(key, value)| (key, value));
    let constructor = type_call()
        .then_ignore(sym("{"))
        .then(
            choice((
                constructor_value
                    .separated_by(sym(","))
                    .at_least(1)
                    .collect::<Vec<_>>()
                    .then(
                        sym(",")
                            .ignore_then(op_kw("...").to(true).or_not())
                            .map(|dots| dots.unwrap_or(false))
                            .or_not(),
                    )
                    .map(|(values, trailing)| (values, trailing.unwrap_or(false))),
                op_kw("...").to((Vec::new(), true)),
            ))
            .or_not(),
        )
        .then_ignore(sym("}"))
        .map_with(|(type_call, body), ex| {
            let (values, implicit_empty) = body.unwrap_or((Vec::new(), false));
            node(
                ExprKind::Constructor {
                    type_call,
                    values,
                    implicit_empty,
                },
                span_of(ex.span()),
            )
        })
        .labelled("constructor");

    // `RosettaReferenceOrFunctionCall`.
    let symbol = expr_qname()
        .then(
            sym("(")
                .ignore_then(
                    expression
                        .clone()
                        .separated_by(sym(","))
                        .collect::<Vec<_>>(),
                )
                .then_ignore(sym(")")),
        )
        .map_with(|(name, args), ex| {
            node(
                ExprKind::Symbol {
                    name,
                    explicit_args: true,
                    args,
                },
                span_of(ex.span()),
            )
        })
        .or(expr_qname().map_with(|name, ex| {
            node(
                ExprKind::Symbol {
                    name,
                    explicit_args: false,
                    args: Vec::new(),
                },
                span_of(ex.span()),
            )
        }))
        .labelled("symbol reference");

    // `ListLiteral`.
    let list = sym("[")
        .ignore_then(
            expression
                .clone()
                .separated_by(sym(","))
                .collect::<Vec<_>>(),
        )
        .then_ignore(sym("]"))
        .map_with(|elements, ex| node(ExprKind::List(elements), span_of(ex.span())))
        .labelled("list literal");

    choice((
        only_exists,
        conditional,
        string_lit().map_with(|v, ex| node(ExprKind::Str(v), span_of(ex.span()))),
        bool_kw().map_with(|v, ex| node(ExprKind::Boolean(v), span_of(ex.span()))),
        number_lit().map_with(|v, ex| node(ExprKind::Number(v), span_of(ex.span()))),
        int_lit_raw().map_with(|v, ex| node(ExprKind::Int(v), span_of(ex.span()))),
        kw("empty").map_with(|(), ex| node(ExprKind::Empty, span_of(ex.span()))),
        constructor,
        // `item` is a keyword (the implicit variable) and must win over a
        // symbol reference.
        kw("item").map_with(|(), ex| node(ExprKind::Item, span_of(ex.span()))),
        symbol,
        list,
        sym("(")
            .ignore_then(expression.clone())
            .then_ignore(sym(")"))
            .labelled("parenthesized expression"),
    ))
    .boxed()
}
