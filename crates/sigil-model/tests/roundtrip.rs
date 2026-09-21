//! Round-trip property test: print a generated IR expression, re-parse the
//! text with the real parser, and require the lowered IR to be identical
//! (compared via the normalized JSON, which ignores spans).
//!
//! The generator respects the printer's known context constraints:
//! * `AsKey` is only produced as constructor values or wrapped around the
//!   whole expression (`as-key` exists only in `ExpressionWithAsKey`
//!   positions).
//! * A separator-less `join` always carries the generated `""` literal.
//! * A conditional without `else` always carries the generated empty list
//!   literal.
//! * A paren-less `only exists` has exactly one argument.

use proptest::prelude::*;
use proptest::sample::select;
use sigil_model::expr::{
    CardinalityModifier, ComparisonOperator, EqualityOperator, ExistsModifier, Expr,
    LogicalOperator, SwitchCase, SwitchGuard,
};

const SYMBOLS: &[&str] = &["a", "b", "c", "val", "thing"];
const FEATURES: &[&str] = &["f1", "f2"];
const KEYS: &[&str] = &["k1", "k2"];
const TYPE_NAMES: &[&str] = &["Gate", "Thing"];

fn leaf() -> impl Strategy<Value = Expr> {
    prop_oneof![
        Just(Expr::BooleanLiteral { value: true }),
        Just(Expr::BooleanLiteral { value: false }),
        "[a-z]{0,8}".prop_map(|s| Expr::StringLiteral { value: s }),
        "(1|2|10|42)\\.(0|5|25)".prop_map(|text| Expr::NumberLiteral { text }),
        "(1|2|7|100)".prop_map(|text| Expr::IntLiteral { text }),
        Just(Expr::empty_list()),
        Just(Expr::ImplicitVariable),
        select(SYMBOLS).prop_map(|symbol| Expr::SymbolReference {
            symbol: (*symbol).to_string(),
            explicit_arguments: false,
            raw_args: vec![],
        }),
    ]
}

fn symbol() -> impl Strategy<Value = Expr> {
    select(SYMBOLS).prop_map(|symbol| Expr::SymbolReference {
        symbol: (*symbol).to_string(),
        explicit_arguments: false,
        raw_args: vec![],
    })
}

/// Arguments of `only exists`: symbols, `item`, or plain `->feature`
/// chains (the grammar forbids `->>` inside only-exists arguments).
fn only_exists_arg() -> impl Strategy<Value = Expr> {
    prop_oneof![
        symbol(),
        Just(Expr::ImplicitVariable),
        (symbol(), select(FEATURES)).prop_map(|(receiver, feature)| Expr::FeatureCall {
            receiver: Box::new(receiver),
            feature: Some((*feature).to_string()),
        }),
    ]
}

fn arb_expr() -> BoxedStrategy<Expr> {
    arb_expr_depth(3).boxed()
}

fn arb_expr_depth(depth: u32) -> BoxedStrategy<Expr> {
    if depth == 0 {
        return leaf().boxed();
    }
    let sub = arb_expr_depth(depth - 1);
    let light_sub = arb_expr_depth((depth - 1).min(1));
    prop_oneof![
        leaf(),
        sub.clone().prop_map(|argument| Expr::ExistsExpression {
            modifier: ExistsModifier::None,
            argument: Box::new(argument),
        }),
        (Just(ExistsModifier::Single), sub.clone()).prop_map(|(modifier, argument)| {
            Expr::ExistsExpression {
                modifier,
                argument: Box::new(argument),
            }
        }),
        (Just(ExistsModifier::Multiple), sub.clone()).prop_map(|(modifier, argument)| {
            Expr::ExistsExpression {
                modifier,
                argument: Box::new(argument),
            }
        }),
        sub.clone().prop_map(|argument| Expr::AbsentExpression {
            argument: Box::new(argument),
        }),
        sub.clone().prop_map(|argument| Expr::CountOperation {
            argument: Box::new(argument),
        }),
        sub.clone().prop_map(|argument| Expr::DistinctOperation {
            argument: Box::new(argument),
        }),
        sub.clone().prop_map(|argument| Expr::SumOperation {
            argument: Box::new(argument),
        }),
        sub.clone().prop_map(|argument| Expr::FirstOperation {
            argument: Box::new(argument),
        }),
        sub.clone().prop_map(|argument| Expr::OneOfOperation {
            argument: Box::new(argument),
        }),
        sub.clone().prop_map(|argument| Expr::ToStringOperation {
            argument: Box::new(argument),
        }),
        (sub.clone(), prop::option::of(select(FEATURES))).prop_map(|(receiver, feature)| {
            Expr::FeatureCall {
                receiver: Box::new(receiver),
                feature: feature.map(|f| f.to_string()),
            }
        }),
        (sub.clone(), select(FEATURES)).prop_map(|(receiver, feature)| Expr::DeepFeatureCall {
            receiver: Box::new(receiver),
            feature: Some((*feature).to_string()),
        }),
        (sub.clone(), sub.clone()).prop_map(|(left, right)| Expr::LogicalOperation {
            operator: LogicalOperator::And,
            left: Box::new(left),
            right: Box::new(right),
        }),
        (sub.clone(), sub.clone()).prop_map(|(left, right)| Expr::LogicalOperation {
            operator: LogicalOperator::Or,
            left: Box::new(left),
            right: Box::new(right),
        }),
        (sub.clone(), sub.clone()).prop_map(|(left, right)| Expr::EqualityOperation {
            operator: EqualityOperator::Eq,
            card_mod: CardinalityModifier::None,
            left: Box::new(left),
            right: Box::new(right),
        }),
        (sub.clone(), sub.clone()).prop_map(|(left, right)| Expr::ComparisonOperation {
            operator: ComparisonOperator::Ge,
            card_mod: CardinalityModifier::None,
            left: Box::new(left),
            right: Box::new(right),
        }),
        (sub.clone(), sub.clone()).prop_map(|(left, right)| Expr::ContainsExpression {
            left: Box::new(left),
            right: Box::new(right),
        }),
        // Separator-less join always carries the generated "" literal.
        sub.clone().prop_map(|left| Expr::JoinOperation {
            left: Box::new(left),
            right: Box::new(Expr::generated_empty_separator()),
            explicit_separator: false,
        }),
        (sub.clone(), light_sub.clone()).prop_map(|(left, right)| Expr::JoinOperation {
            left: Box::new(left),
            right: Box::new(right),
            explicit_separator: true,
        }),
        // A conditional without `else` always carries the generated empty
        // list literal.
        (sub.clone(), light_sub.clone()).prop_map(|(if_, ifthen)| {
            Expr::ConditionalExpression {
                if_: Box::new(if_),
                ifthen: Box::new(ifthen),
                elsethen: Box::new(Expr::empty_list()),
                full: false,
            }
        }),
        (sub.clone(), light_sub.clone(), light_sub.clone()).prop_map(|(if_, ifthen, elsethen)| {
            Expr::ConditionalExpression {
                if_: Box::new(if_),
                ifthen: Box::new(ifthen),
                elsethen: Box::new(elsethen),
                full: true,
            }
        }),
        // Paren-less only-exists: exactly one argument.
        only_exists_arg().prop_map(|arg| Expr::OnlyExistsExpression {
            args: vec![arg],
            has_parentheses: false,
        }),
        prop::collection::vec(only_exists_arg(), 1..=3).prop_map(|args| {
            Expr::OnlyExistsExpression {
                args,
                has_parentheses: true,
            }
        }),
        // Functional operations, with and without inline functions.
        (sub.clone(), prop::option::of(inline_fn(light_sub.clone()))).prop_map(
            |(argument, function)| Expr::FilterOperation {
                argument: Box::new(argument),
                function,
            },
        ),
        (sub.clone(), prop::option::of(inline_fn(light_sub.clone()))).prop_map(
            |(argument, function)| Expr::ReduceOperation {
                argument: Box::new(argument),
                function,
            },
        ),
        (sub.clone(), prop::option::of(inline_fn(light_sub.clone()))).prop_map(
            |(argument, function)| Expr::SortOperation {
                argument: Box::new(argument),
                function,
            },
        ),
        // A body-less `then` is degenerate (it only parses at the very end
        // of an expression) and is not generated.
        (sub.clone(), then_fn(light_sub.clone())).prop_map(|(argument, function)| {
            Expr::ThenOperation {
                argument: Box::new(argument),
                function: Some(function),
            }
        }),
        // Switch: literal and reference guards, plus default.
        (
            sub.clone(),
            prop::collection::vec(switch_case(light_sub.clone()), 1..=2)
        )
            .prop_map(|(argument, cases)| Expr::SwitchOperation {
                argument: Box::new(argument),
                cases,
            },),
        // Constructor expressions (values may use `as-key`).
        (
            select(TYPE_NAMES),
            prop::collection::vec(constructor_pair(light_sub.clone()), 0..=2)
        )
            .prop_map(|(name, values)| Expr::ConstructorExpression {
                type_call: sigil_model::TypeRef::unresolved(name),
                values: values
                    .into_iter()
                    .map(|(key, value)| sigil_model::expr::ConstructorPair {
                        key,
                        value: Box::new(value),
                    })
                    .collect(),
                implicit_empty: false,
            },),
    ]
    .boxed()
}

fn switch_case(body: BoxedStrategy<Expr>) -> impl Strategy<Value = SwitchCase> {
    // Guards are `RosettaLiteral`s (bool/string/number/int) or references:
    // `empty`, `item`, ... are not valid guards.
    let guard_literal = prop_oneof![
        Just(Expr::BooleanLiteral { value: true }),
        Just(Expr::BooleanLiteral { value: false }),
        "[a-z]{0,8}".prop_map(|s| Expr::StringLiteral { value: s }),
        r"(1|2|10|42)\.(0|5|25)".prop_map(|text| Expr::NumberLiteral { text }),
        "(1|2|7|100)".prop_map(|text| Expr::IntLiteral { text }),
    ];
    prop_oneof![
        (guard_literal, body.clone()).prop_map(|(guard, expression)| SwitchCase {
            guard: Some(SwitchGuard::Literal(Box::new(guard))),
            expression: Box::new(expression),
        }),
        (select(SYMBOLS), body.clone()).prop_map(|(target, expression)| SwitchCase {
            guard: Some(SwitchGuard::Reference(target.to_string())),
            expression: Box::new(expression),
        }),
        body.prop_map(|expression| SwitchCase {
            guard: None,
            expression: Box::new(expression),
        }),
    ]
}

fn inline_fn(
    body: impl Strategy<Value = Expr>,
) -> impl Strategy<Value = sigil_model::expr::InlineFunction> {
    (
        prop::option::of(prop::collection::vec(select(FEATURES), 1..=2)),
        body,
    )
        .prop_map(|(parameters, body)| sigil_model::expr::InlineFunction {
            parameters: parameters
                .unwrap_or_default()
                .iter()
                .map(|s| s.to_string())
                .collect(),
            body: Box::new(body),
        })
}

fn then_fn(
    body: impl Strategy<Value = Expr>,
) -> impl Strategy<Value = sigil_model::expr::InlineFunction> {
    body.prop_map(|body| sigil_model::expr::InlineFunction {
        parameters: vec![],
        body: Box::new(body),
    })
}

fn constructor_pair(body: impl Strategy<Value = Expr>) -> impl Strategy<Value = (String, Expr)> {
    (select(KEYS), body).prop_map(|(key, value)| ((*key).to_string(), value))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn printed_expressions_reparse_to_the_same_ir(expr in arb_expr()) {
        let text = expr.print();
        let parsed = sigil_syntax::parse_expression_str(&text)
            .unwrap_or_else(|diags| panic!("printed expression did not re-parse: {text}\n{diags:?}"));
        let lowered = sigil_syntax::lower_expression(&parsed);
        prop_assert_eq!(expr.to_json(), lowered.to_json(), "round-trip failed for: {}", text);
    }
}
