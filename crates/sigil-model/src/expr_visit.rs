//! Read-only traversal over [`Expr`] trees: the `ExprVisitor` trait, the
//! `walk` driver, and the `ExprFamily` variant taxonomy.
//!
//! The walker is a depth-first, pre-order traversal that visits every node
//! exactly once and recurses into children in source order (the same order
//! as [`Expr::for_each_child_mut`]). It is lossless by construction: the
//! family dispatch and the child recursion are both exhaustive `match`es
//! with no wildcard arm, so a new `Expr` variant fails to compile until it
//! is classified and traversed here.
//!
//! Protocol per node, before descending:
//!
//! 1. [`ExprVisitor::visit_expr`] — called for every node.
//! 2. Exactly one family hook ([`ExprVisitor::visit_literal`],
//!    [`ExprVisitor::visit_path`], …) chosen by [`Expr::family`].
//! 3. Children, recursively, in source order.
//!
//! Hooks receive the whole node: `walk` already recurses, so implementors
//! that need the variant payload re-`match` the node and read what they
//! need. All trait methods default to no-ops; override only what the
//! downstream consumer (e.g. an expr→Rust transpiler) cares about.

use crate::expr::Expr;

/// The variant family of an [`Expr`]: the coarse grouping a consumer
/// (transpiler, linter, pretty-structure dumper) dispatches on before
/// looking at the individual variant.
///
/// The taxonomy follows the shape of the variant set:
///
/// * [`ExprFamily::Literal`] — leaf constants, including list literals.
/// * [`ExprFamily::Path`] — symbol references, `item`, and the `->` /
///   `->>` feature-call chains.
/// * [`ExprFamily::Arithmetic`], [`ExprFamily::Logical`],
///   [`ExprFamily::Comparison`] — the binary operator nodes. `Comparison`
///   is the boolean-valued and coalescing word/equality operators
///   (`=`, `<>`, `<`, `>`, `contains`, `disjoint`, `default`).
/// * [`ExprFamily::Quantifier`] — existence checks (`exists`, `only
///   exists`, `is absent`).
/// * [`ExprFamily::ListOp`] — operations on lists without an inline
///   function (`count`, `first`, `join`, …).
/// * [`ExprFamily::FunctionOp`] — operations carrying an optional inline
///   function (`then`, `filter`, `extract`, `reduce`, `sort`, `min`,
///   `max`).
/// * [`ExprFamily::Cast`] — type conversions (`to-string`, `to-enum`,
///   `as`, `as-key`, `one-of`, …).
/// * [`ExprFamily::ControlFlow`] — `if` and `switch`.
/// * [`ExprFamily::Meta`] — `with-meta`.
/// * [`ExprFamily::Choice`] — `choice` (`<a> required choice <b, c>`).
/// * [`ExprFamily::Constructor`] — record/choice construction `{ ... }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExprFamily {
    Literal,
    Path,
    Arithmetic,
    Logical,
    Comparison,
    Quantifier,
    ListOp,
    FunctionOp,
    Cast,
    ControlFlow,
    Meta,
    Choice,
    Constructor,
}

impl ExprFamily {
    /// Stable snake-case name (log/serialization friendly).
    pub fn as_str(self) -> &'static str {
        match self {
            ExprFamily::Literal => "literal",
            ExprFamily::Path => "path",
            ExprFamily::Arithmetic => "arithmetic",
            ExprFamily::Logical => "logical",
            ExprFamily::Comparison => "comparison",
            ExprFamily::Quantifier => "quantifier",
            ExprFamily::ListOp => "list-op",
            ExprFamily::FunctionOp => "function-op",
            ExprFamily::Cast => "cast",
            ExprFamily::ControlFlow => "control-flow",
            ExprFamily::Meta => "meta",
            ExprFamily::Choice => "choice",
            ExprFamily::Constructor => "constructor",
        }
    }
}

/// Read-only visitor over an `Expr` tree. Every method defaults to a
/// no-op; [`walk`] drives the traversal and calls `visit_expr` plus the
/// one family hook for each node.
pub trait ExprVisitor {
    /// Called for every node, before its family hook.
    fn visit_expr(&mut self, _expr: &Expr) {}

    /// Literal nodes: boolean, string, number, int, and list literals.
    fn visit_literal(&mut self, _expr: &Expr) {}
    /// Path nodes: symbol references, `item`, feature calls.
    fn visit_path(&mut self, _expr: &Expr) {}
    /// `+ - * /` operations.
    fn visit_arithmetic_op(&mut self, _expr: &Expr) {}
    /// `and` / `or` operations.
    fn visit_logical_op(&mut self, _expr: &Expr) {}
    /// Binary comparison-style operations: equality, ordering, `contains`,
    /// `disjoint`, `default`.
    fn visit_comparison_op(&mut self, _expr: &Expr) {}
    /// `exists` (with modifier), `only exists`, `is absent`.
    fn visit_quantifier(&mut self, _expr: &Expr) {}
    /// List operations without an inline function (`count`, `first`,
    /// `flatten`, `join`, …).
    fn visit_list_op(&mut self, _expr: &Expr) {}
    /// Operations carrying an optional inline function (`then`, `filter`,
    /// `extract`, `reduce`, `sort`, `min`, `max`).
    fn visit_function_op(&mut self, _expr: &Expr) {}
    /// Type casts: `to-*`, `as`, `as-key`, `one-of`.
    fn visit_cast(&mut self, _expr: &Expr) {}
    /// `if` and `switch`.
    fn visit_control_flow(&mut self, _expr: &Expr) {}
    /// `with-meta { ... }`.
    fn visit_meta(&mut self, _expr: &Expr) {}
    /// `choice` operations.
    fn visit_choice(&mut self, _expr: &Expr) {}
    /// Record/choice constructor expressions `{ ... }`.
    fn visit_constructor(&mut self, _expr: &Expr) {}
}

/// Depth-first, pre-order, read-only traversal: `visitor` sees every node
/// exactly once (`visit_expr` plus one family hook), then traversal
/// descends into the children in source order.
pub fn walk(expr: &Expr, visitor: &mut impl ExprVisitor) {
    visitor.visit_expr(expr);
    match expr.family() {
        ExprFamily::Literal => visitor.visit_literal(expr),
        ExprFamily::Path => visitor.visit_path(expr),
        ExprFamily::Arithmetic => visitor.visit_arithmetic_op(expr),
        ExprFamily::Logical => visitor.visit_logical_op(expr),
        ExprFamily::Comparison => visitor.visit_comparison_op(expr),
        ExprFamily::Quantifier => visitor.visit_quantifier(expr),
        ExprFamily::ListOp => visitor.visit_list_op(expr),
        ExprFamily::FunctionOp => visitor.visit_function_op(expr),
        ExprFamily::Cast => visitor.visit_cast(expr),
        ExprFamily::ControlFlow => visitor.visit_control_flow(expr),
        ExprFamily::Meta => visitor.visit_meta(expr),
        ExprFamily::Choice => visitor.visit_choice(expr),
        ExprFamily::Constructor => visitor.visit_constructor(expr),
    }
    expr.for_each_child(&mut |child| walk(child, visitor));
}

#[cfg(test)]
mod tests {
    use super::{walk, ExprFamily, ExprVisitor};
    use crate::expr::{
        ArithmeticOperator, CardinalityModifier, ConstructorPair, EqualityOperator, ExistsModifier,
        Expr, InlineFunction, LogicalOperator, Necessity, SwitchCase, SwitchGuard, WithMetaEntry,
    };
    use crate::TypeRef;

    // ---- fixtures -------------------------------------------------------------

    fn b(value: bool) -> Expr {
        Expr::BooleanLiteral { value }
    }
    fn s(value: &str) -> Expr {
        Expr::StringLiteral {
            value: value.to_string(),
        }
    }
    fn num(text: &str) -> Expr {
        Expr::NumberLiteral {
            text: text.to_string(),
        }
    }
    fn int(text: &str) -> Expr {
        Expr::IntLiteral {
            text: text.to_string(),
        }
    }
    fn sym(symbol: &str) -> Expr {
        Expr::SymbolReference {
            symbol: symbol.to_string(),
            explicit_arguments: false,
            raw_args: Vec::new(),
        }
    }
    fn sym_call(symbol: &str, args: Vec<Expr>) -> Expr {
        Expr::SymbolReference {
            symbol: symbol.to_string(),
            explicit_arguments: true,
            raw_args: args,
        }
    }
    fn implicit() -> Expr {
        Expr::ImplicitVariable
    }
    fn box_(expr: Expr) -> Box<Expr> {
        Box::new(expr)
    }
    fn func(parameters: &[&str], body: Expr) -> Option<InlineFunction> {
        Some(InlineFunction {
            parameters: parameters.iter().map(|p| (*p).to_string()).collect(),
            body: box_(body),
        })
    }

    /// One hand-built tree per `Expr` variant (a few variants twice, and a
    /// few extra levels of nesting to exercise multi-level traversal).
    fn forest() -> Vec<Expr> {
        vec![
            b(true),
            s("hello"),
            num("1.5"),
            int("42"),
            Expr::ListLiteral {
                elements: vec![int("1"), int("2"), int("3")],
            },
            sym("trade"),
            sym_call("f", vec![int("1"), s("x")]),
            implicit(),
            Expr::FeatureCall {
                receiver: box_(sym("trade")),
                feature: Some("id".to_string()),
            },
            Expr::DeepFeatureCall {
                receiver: box_(sym("trade")),
                feature: None,
            },
            Expr::ArithmeticOperation {
                operator: ArithmeticOperator::Add,
                left: box_(int("1")),
                right: box_(int("2")),
            },
            Expr::LogicalOperation {
                operator: LogicalOperator::And,
                left: box_(b(true)),
                right: box_(b(false)),
            },
            Expr::EqualityOperation {
                operator: EqualityOperator::Eq,
                card_mod: CardinalityModifier::None,
                left: box_(sym("a")),
                right: box_(sym("b")),
            },
            Expr::ComparisonOperation {
                operator: crate::expr::ComparisonOperator::Gt,
                card_mod: CardinalityModifier::Any,
                left: box_(sym("a")),
                right: box_(int("5")),
            },
            Expr::ContainsExpression {
                left: box_(sym("xs")),
                right: box_(int("1")),
            },
            Expr::DisjointExpression {
                left: box_(sym("l")),
                right: box_(sym("r")),
            },
            Expr::DefaultOperation {
                left: box_(sym("opt")),
                right: box_(int("0")),
            },
            Expr::JoinOperation {
                left: box_(sym("xs")),
                right: box_(s(",")),
                explicit_separator: true,
            },
            Expr::ConditionalExpression {
                if_: box_(Expr::LogicalOperation {
                    operator: LogicalOperator::And,
                    left: box_(b(true)),
                    right: box_(b(false)),
                }),
                ifthen: box_(Expr::ArithmeticOperation {
                    operator: ArithmeticOperator::Add,
                    left: box_(int("1")),
                    right: box_(int("2")),
                }),
                elsethen: box_(Expr::ListLiteral {
                    elements: vec![s("a"), s("b")],
                }),
                full: true,
            },
            Expr::OnlyExistsExpression {
                args: vec![sym("a"), sym("b")],
                has_parentheses: true,
            },
            Expr::ExistsExpression {
                modifier: ExistsModifier::Single,
                argument: box_(sym("xs")),
            },
            Expr::AbsentExpression {
                argument: box_(sym("x")),
            },
            Expr::OnlyElement {
                argument: box_(sym("xs")),
            },
            Expr::CountOperation {
                argument: box_(sym("xs")),
            },
            Expr::FlattenOperation {
                argument: box_(sym("xs")),
            },
            Expr::DistinctOperation {
                argument: box_(sym("xs")),
            },
            Expr::ReverseOperation {
                argument: box_(sym("xs")),
            },
            Expr::FirstOperation {
                argument: box_(sym("xs")),
            },
            Expr::LastOperation {
                argument: box_(sym("xs")),
            },
            Expr::SumOperation {
                argument: box_(sym("xs")),
            },
            Expr::AsKeyOperation {
                argument: box_(sym("x")),
            },
            Expr::OneOfOperation {
                argument: box_(sym("c")),
            },
            Expr::ChoiceOperation {
                necessity: Necessity::Required,
                attributes: vec!["a".to_string(), "b".to_string()],
                argument: box_(implicit()),
            },
            Expr::ToStringOperation {
                argument: box_(int("7")),
            },
            Expr::ToNumberOperation {
                argument: box_(s("1")),
            },
            Expr::ToIntOperation {
                argument: box_(num("1.0")),
            },
            Expr::ToTimeOperation {
                argument: box_(s("10:00")),
            },
            Expr::ToEnumOperation {
                enumeration: "E".to_string(),
                argument: box_(s("V")),
            },
            Expr::ToDateOperation {
                argument: box_(s("2024-01-01")),
            },
            Expr::ToDateTimeOperation {
                argument: box_(s("2024-01-01T00:00")),
            },
            Expr::ToZonedDateTimeOperation {
                argument: box_(s("2024-01-01T00:00[UTC]")),
            },
            Expr::SwitchOperation {
                argument: box_(sym("status")),
                cases: vec![
                    SwitchCase {
                        guard: Some(SwitchGuard::Literal(box_(Expr::ListLiteral {
                            elements: vec![int("1"), int("2")],
                        }))),
                        expression: box_(s("low")),
                    },
                    SwitchCase {
                        guard: Some(SwitchGuard::Reference("E_VALUE".to_string())),
                        expression: box_(Expr::ArithmeticOperation {
                            operator: ArithmeticOperator::Multiply,
                            left: box_(int("2")),
                            right: box_(int("3")),
                        }),
                    },
                    SwitchCase {
                        guard: None,
                        expression: box_(Expr::DefaultOperation {
                            left: box_(sym("x")),
                            right: box_(int("0")),
                        }),
                    },
                ],
            },
            Expr::WithMetaOperation {
                argument: box_(sym("x")),
                entries: vec![WithMetaEntry {
                    key: "scheme".to_string(),
                    value: box_(s("S")),
                }],
            },
            Expr::AsOperation {
                type_: "T".to_string(),
                argument: box_(sym("x")),
            },
            Expr::ThenOperation {
                argument: box_(sym("xs")),
                function: func(&["x"], sym("x")),
            },
            Expr::FilterOperation {
                argument: box_(sym("xs")),
                function: func(&["x"], b(true)),
            },
            Expr::MapOperation {
                argument: box_(sym("xs")),
                function: func(&["x"], sym("x")),
            },
            Expr::ReduceOperation {
                argument: box_(sym("xs")),
                function: func(&["a", "b"], sym("a")),
            },
            // `function: None` flavor, and an empty-parameter body:
            Expr::SortOperation {
                argument: box_(sym("xs")),
                function: None,
            },
            Expr::MinOperation {
                argument: box_(sym("xs")),
                function: func(&[], implicit()),
            },
            Expr::MaxOperation {
                argument: box_(sym("xs")),
                function: func(&["m"], sym("m")),
            },
            Expr::ConstructorExpression {
                type_call: TypeRef::unresolved("Trade"),
                values: vec![ConstructorPair {
                    key: "id".to_string(),
                    value: box_(int("1")),
                }],
                implicit_empty: false,
            },
        ]
    }

    // ---- independent implementation #1: tag enumeration -----------------------

    /// Variant tag of a node. Exhaustive with no wildcard: a new variant
    /// breaks compilation of the fixture-coverage check.
    fn tag(expr: &Expr) -> &'static str {
        match expr {
            Expr::BooleanLiteral { .. } => "BooleanLiteral",
            Expr::StringLiteral { .. } => "StringLiteral",
            Expr::NumberLiteral { .. } => "NumberLiteral",
            Expr::IntLiteral { .. } => "IntLiteral",
            Expr::ListLiteral { .. } => "ListLiteral",
            Expr::SymbolReference { .. } => "SymbolReference",
            Expr::ImplicitVariable => "ImplicitVariable",
            Expr::FeatureCall { .. } => "FeatureCall",
            Expr::DeepFeatureCall { .. } => "DeepFeatureCall",
            Expr::ArithmeticOperation { .. } => "ArithmeticOperation",
            Expr::LogicalOperation { .. } => "LogicalOperation",
            Expr::EqualityOperation { .. } => "EqualityOperation",
            Expr::ComparisonOperation { .. } => "ComparisonOperation",
            Expr::ContainsExpression { .. } => "ContainsExpression",
            Expr::DisjointExpression { .. } => "DisjointExpression",
            Expr::DefaultOperation { .. } => "DefaultOperation",
            Expr::JoinOperation { .. } => "JoinOperation",
            Expr::ConditionalExpression { .. } => "ConditionalExpression",
            Expr::OnlyExistsExpression { .. } => "OnlyExistsExpression",
            Expr::ExistsExpression { .. } => "ExistsExpression",
            Expr::AbsentExpression { .. } => "AbsentExpression",
            Expr::OnlyElement { .. } => "OnlyElement",
            Expr::CountOperation { .. } => "CountOperation",
            Expr::FlattenOperation { .. } => "FlattenOperation",
            Expr::DistinctOperation { .. } => "DistinctOperation",
            Expr::ReverseOperation { .. } => "ReverseOperation",
            Expr::FirstOperation { .. } => "FirstOperation",
            Expr::LastOperation { .. } => "LastOperation",
            Expr::SumOperation { .. } => "SumOperation",
            Expr::AsKeyOperation { .. } => "AsKeyOperation",
            Expr::OneOfOperation { .. } => "OneOfOperation",
            Expr::ChoiceOperation { .. } => "ChoiceOperation",
            Expr::ToStringOperation { .. } => "ToStringOperation",
            Expr::ToNumberOperation { .. } => "ToNumberOperation",
            Expr::ToIntOperation { .. } => "ToIntOperation",
            Expr::ToTimeOperation { .. } => "ToTimeOperation",
            Expr::ToEnumOperation { .. } => "ToEnumOperation",
            Expr::ToDateOperation { .. } => "ToDateOperation",
            Expr::ToDateTimeOperation { .. } => "ToDateTimeOperation",
            Expr::ToZonedDateTimeOperation { .. } => "ToZonedDateTimeOperation",
            Expr::SwitchOperation { .. } => "SwitchOperation",
            Expr::WithMetaOperation { .. } => "WithMetaOperation",
            Expr::AsOperation { .. } => "AsOperation",
            Expr::ThenOperation { .. } => "ThenOperation",
            Expr::FilterOperation { .. } => "FilterOperation",
            Expr::MapOperation { .. } => "MapOperation",
            Expr::ReduceOperation { .. } => "ReduceOperation",
            Expr::SortOperation { .. } => "SortOperation",
            Expr::MinOperation { .. } => "MinOperation",
            Expr::MaxOperation { .. } => "MaxOperation",
            Expr::ConstructorExpression { .. } => "ConstructorExpression",
        }
    }

    /// The expected family of every variant. The array length pins the
    /// variant count: adding a variant without a classification row breaks
    /// compilation.
    const EXPECTED_FAMILY: [(&str, ExprFamily); 51] = [
        ("BooleanLiteral", ExprFamily::Literal),
        ("StringLiteral", ExprFamily::Literal),
        ("NumberLiteral", ExprFamily::Literal),
        ("IntLiteral", ExprFamily::Literal),
        ("ListLiteral", ExprFamily::Literal),
        ("SymbolReference", ExprFamily::Path),
        ("ImplicitVariable", ExprFamily::Path),
        ("FeatureCall", ExprFamily::Path),
        ("DeepFeatureCall", ExprFamily::Path),
        ("ArithmeticOperation", ExprFamily::Arithmetic),
        ("LogicalOperation", ExprFamily::Logical),
        ("EqualityOperation", ExprFamily::Comparison),
        ("ComparisonOperation", ExprFamily::Comparison),
        ("ContainsExpression", ExprFamily::Comparison),
        ("DisjointExpression", ExprFamily::Comparison),
        ("DefaultOperation", ExprFamily::Comparison),
        ("JoinOperation", ExprFamily::ListOp),
        ("ConditionalExpression", ExprFamily::ControlFlow),
        ("OnlyExistsExpression", ExprFamily::Quantifier),
        ("ExistsExpression", ExprFamily::Quantifier),
        ("AbsentExpression", ExprFamily::Quantifier),
        ("OnlyElement", ExprFamily::ListOp),
        ("CountOperation", ExprFamily::ListOp),
        ("FlattenOperation", ExprFamily::ListOp),
        ("DistinctOperation", ExprFamily::ListOp),
        ("ReverseOperation", ExprFamily::ListOp),
        ("FirstOperation", ExprFamily::ListOp),
        ("LastOperation", ExprFamily::ListOp),
        ("SumOperation", ExprFamily::ListOp),
        ("AsKeyOperation", ExprFamily::Cast),
        ("OneOfOperation", ExprFamily::Cast),
        ("ChoiceOperation", ExprFamily::Choice),
        ("ToStringOperation", ExprFamily::Cast),
        ("ToNumberOperation", ExprFamily::Cast),
        ("ToIntOperation", ExprFamily::Cast),
        ("ToTimeOperation", ExprFamily::Cast),
        ("ToEnumOperation", ExprFamily::Cast),
        ("ToDateOperation", ExprFamily::Cast),
        ("ToDateTimeOperation", ExprFamily::Cast),
        ("ToZonedDateTimeOperation", ExprFamily::Cast),
        ("SwitchOperation", ExprFamily::ControlFlow),
        ("WithMetaOperation", ExprFamily::Meta),
        ("AsOperation", ExprFamily::Cast),
        ("ThenOperation", ExprFamily::FunctionOp),
        ("FilterOperation", ExprFamily::FunctionOp),
        ("MapOperation", ExprFamily::FunctionOp),
        ("ReduceOperation", ExprFamily::FunctionOp),
        ("SortOperation", ExprFamily::FunctionOp),
        ("MinOperation", ExprFamily::FunctionOp),
        ("MaxOperation", ExprFamily::FunctionOp),
        ("ConstructorExpression", ExprFamily::Constructor),
    ];

    // ---- independent implementation #2: manual node counter -------------------

    /// Node count by an exhaustive manual recursion written independently
    /// of `walk`/`for_each_child` (no wildcard: a new variant breaks
    /// compilation until this counter handles it).
    fn count_nodes_manual(expr: &Expr) -> usize {
        match expr {
            Expr::BooleanLiteral { .. }
            | Expr::StringLiteral { .. }
            | Expr::NumberLiteral { .. }
            | Expr::IntLiteral { .. }
            | Expr::ImplicitVariable => 1,
            Expr::ListLiteral { elements } => {
                1 + elements.iter().map(count_nodes_manual).sum::<usize>()
            }
            Expr::SymbolReference { raw_args, .. } => {
                1 + raw_args.iter().map(count_nodes_manual).sum::<usize>()
            }
            Expr::FeatureCall { receiver, .. } | Expr::DeepFeatureCall { receiver, .. } => {
                1 + count_nodes_manual(receiver)
            }
            Expr::ArithmeticOperation { left, right, .. }
            | Expr::LogicalOperation { left, right, .. }
            | Expr::EqualityOperation { left, right, .. }
            | Expr::ComparisonOperation { left, right, .. }
            | Expr::ContainsExpression { left, right }
            | Expr::DisjointExpression { left, right }
            | Expr::DefaultOperation { left, right }
            | Expr::JoinOperation { left, right, .. } => {
                1 + count_nodes_manual(left) + count_nodes_manual(right)
            }
            Expr::ConditionalExpression {
                if_,
                ifthen,
                elsethen,
                ..
            } => {
                1 + count_nodes_manual(if_)
                    + count_nodes_manual(ifthen)
                    + count_nodes_manual(elsethen)
            }
            Expr::OnlyExistsExpression { args, .. } => {
                1 + args.iter().map(count_nodes_manual).sum::<usize>()
            }
            Expr::ExistsExpression { argument, .. }
            | Expr::AbsentExpression { argument }
            | Expr::OnlyElement { argument }
            | Expr::CountOperation { argument }
            | Expr::FlattenOperation { argument }
            | Expr::DistinctOperation { argument }
            | Expr::ReverseOperation { argument }
            | Expr::FirstOperation { argument }
            | Expr::LastOperation { argument }
            | Expr::SumOperation { argument }
            | Expr::AsKeyOperation { argument }
            | Expr::OneOfOperation { argument }
            | Expr::ChoiceOperation { argument, .. }
            | Expr::ToStringOperation { argument }
            | Expr::ToNumberOperation { argument }
            | Expr::ToIntOperation { argument }
            | Expr::ToTimeOperation { argument }
            | Expr::ToEnumOperation { argument, .. }
            | Expr::ToDateOperation { argument }
            | Expr::ToDateTimeOperation { argument }
            | Expr::ToZonedDateTimeOperation { argument }
            | Expr::WithMetaOperation { argument, .. }
            | Expr::AsOperation { argument, .. } => 1 + count_nodes_manual(argument),
            Expr::SwitchOperation { argument, cases } => {
                1 + count_nodes_manual(argument)
                    + cases
                        .iter()
                        .map(|c| {
                            count_nodes_manual(&c.expression)
                                + match &c.guard {
                                    Some(SwitchGuard::Literal(lit)) => count_nodes_manual(lit),
                                    Some(SwitchGuard::Reference(_)) | None => 0,
                                }
                        })
                        .sum::<usize>()
            }
            Expr::ThenOperation { argument, function }
            | Expr::FilterOperation { argument, function }
            | Expr::MapOperation { argument, function }
            | Expr::ReduceOperation { argument, function }
            | Expr::SortOperation { argument, function }
            | Expr::MinOperation { argument, function }
            | Expr::MaxOperation { argument, function } => {
                1 + count_nodes_manual(argument)
                    + function.as_ref().map_or(0, |f| count_nodes_manual(&f.body))
            }
            Expr::ConstructorExpression { values, .. } => {
                1 + values
                    .iter()
                    .map(|p| count_nodes_manual(&p.value))
                    .sum::<usize>()
            }
        }
    }

    // ---- the recording visitor ------------------------------------------------

    #[derive(Default)]
    struct Recorder {
        /// Pre-order sequence of variant tags (`visit_expr` calls).
        order: Vec<&'static str>,
        /// One `(tag, family)` pair per family-hook dispatch. The family
        /// recorded here is hard-coded per hook, so it cross-checks
        /// `walk`'s dispatch against the expected table.
        families: Vec<(&'static str, ExprFamily)>,
    }

    impl Recorder {
        fn walked(forest: &[Expr]) -> Self {
            let mut recorder = Recorder::default();
            for expr in forest {
                walk(expr, &mut recorder);
            }
            recorder
        }
    }

    impl ExprVisitor for Recorder {
        fn visit_expr(&mut self, expr: &Expr) {
            self.order.push(tag(expr));
        }
        fn visit_literal(&mut self, expr: &Expr) {
            self.families.push((tag(expr), ExprFamily::Literal));
        }
        fn visit_path(&mut self, expr: &Expr) {
            self.families.push((tag(expr), ExprFamily::Path));
        }
        fn visit_arithmetic_op(&mut self, expr: &Expr) {
            self.families.push((tag(expr), ExprFamily::Arithmetic));
        }
        fn visit_logical_op(&mut self, expr: &Expr) {
            self.families.push((tag(expr), ExprFamily::Logical));
        }
        fn visit_comparison_op(&mut self, expr: &Expr) {
            self.families.push((tag(expr), ExprFamily::Comparison));
        }
        fn visit_quantifier(&mut self, expr: &Expr) {
            self.families.push((tag(expr), ExprFamily::Quantifier));
        }
        fn visit_list_op(&mut self, expr: &Expr) {
            self.families.push((tag(expr), ExprFamily::ListOp));
        }
        fn visit_function_op(&mut self, expr: &Expr) {
            self.families.push((tag(expr), ExprFamily::FunctionOp));
        }
        fn visit_cast(&mut self, expr: &Expr) {
            self.families.push((tag(expr), ExprFamily::Cast));
        }
        fn visit_control_flow(&mut self, expr: &Expr) {
            self.families.push((tag(expr), ExprFamily::ControlFlow));
        }
        fn visit_meta(&mut self, expr: &Expr) {
            self.families.push((tag(expr), ExprFamily::Meta));
        }
        fn visit_choice(&mut self, expr: &Expr) {
            self.families.push((tag(expr), ExprFamily::Choice));
        }
        fn visit_constructor(&mut self, expr: &Expr) {
            self.families.push((tag(expr), ExprFamily::Constructor));
        }
    }

    // ---- tests ----------------------------------------------------------------

    /// Acceptance criterion: `walk`'s visit count must equal the count of
    /// the independent manual recursion, and every node must get exactly
    /// one family hook.
    #[test]
    fn walk_agrees_with_manual_node_count() {
        let forest = forest();
        let manual_total: usize = forest.iter().map(count_nodes_manual).sum();
        let recorder = Recorder::walked(&forest);
        assert_eq!(recorder.order.len(), manual_total);
        assert_eq!(recorder.families.len(), manual_total);
        assert!(manual_total > forest.len());
    }

    /// The fixture covers every variant, and each node's family-hook
    /// dispatch matches the expected classification table.
    #[test]
    fn walk_covers_every_variant_with_expected_family() {
        let trees = forest();
        let recorder = Recorder::walked(&trees);
        for &(expected_tag, expected_family) in EXPECTED_FAMILY.iter() {
            assert!(
                recorder.order.contains(&expected_tag),
                "fixture never visits {expected_tag}"
            );
            assert!(
                recorder
                    .families
                    .iter()
                    .any(|&(t, f)| t == expected_tag && f == expected_family),
                "{expected_tag} never dispatched as {expected_family:?}"
            );
        }
        for &(t, f) in recorder.families.iter() {
            assert!(
                EXPECTED_FAMILY.iter().any(|&(t2, f2)| t2 == t && f2 == f),
                "unexpected classification: {t} dispatched as {f:?}"
            );
        }
        // Direct spot checks of the classification helper itself:
        assert_eq!(b(true).family(), ExprFamily::Literal);
        assert_eq!(sym("x").family(), ExprFamily::Path);
        assert_eq!(implicit().family(), ExprFamily::Path);
        assert_eq!(
            forest()
                .iter()
                .find(|e| tag(e) == "SwitchOperation")
                .map(Expr::family),
            Some(ExprFamily::ControlFlow)
        );
        assert_eq!(
            forest()
                .iter()
                .find(|e| tag(e) == "ConstructorExpression")
                .map(Expr::family),
            Some(ExprFamily::Constructor)
        );
    }

    /// Traversal is depth-first, pre-order: node before children, children
    /// in source order.
    #[test]
    fn walk_is_depth_first_preorder() {
        let expr = Expr::ConditionalExpression {
            if_: box_(Expr::LogicalOperation {
                operator: LogicalOperator::And,
                left: box_(b(true)),
                right: box_(b(false)),
            }),
            ifthen: box_(int("1")),
            elsethen: box_(s("x")),
            full: true,
        };
        let mut recorder = Recorder::default();
        walk(&expr, &mut recorder);
        assert_eq!(
            recorder.order,
            vec![
                "ConditionalExpression",
                "LogicalOperation",
                "BooleanLiteral",
                "BooleanLiteral",
                "IntLiteral",
                "StringLiteral",
            ]
        );
        assert_eq!(recorder.order.len(), count_nodes_manual(&expr));
    }

    /// Family names are stable snake-case strings.
    #[test]
    fn family_as_str_is_stable() {
        assert_eq!(ExprFamily::ListOp.as_str(), "list-op");
        assert_eq!(ExprFamily::FunctionOp.as_str(), "function-op");
    }
}
