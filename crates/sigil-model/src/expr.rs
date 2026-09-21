//! The expression IR: a parser-independent representation of Rune
//! expressions mirroring the `RosettaExpression` Ecore package.
//!
//! Expressions are stored inside the semantic model (conditions today,
//! funcs/rules later). The tree carries no spans; callers that need source
//! locations keep their own side table. Two stable projections live here:
//!
//! * [`Expr::to_json`] — the normalized JSON shape compared differentially
//!   against the Java oracle and pinned by the conformance corpus.
//! * [`print`] — a pretty-printer that reproduces valid `.rosetta` syntax,
//!   parenthesizing by precedence so that `parse(print(e)) == e`.

use serde::Serialize;
use serde_json::json;

/// `CardinalityModifier` (`none` is the Ecore default, never written).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardinalityModifier {
    None,
    Any,
    All,
}

impl CardinalityModifier {
    pub fn as_str(self) -> &'static str {
        match self {
            CardinalityModifier::None => "none",
            CardinalityModifier::Any => "any",
            CardinalityModifier::All => "all",
        }
    }
}

/// `ExistsModifier`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExistsModifier {
    None,
    Single,
    Multiple,
}

impl ExistsModifier {
    pub fn as_str(self) -> &'static str {
        match self {
            ExistsModifier::None => "none",
            ExistsModifier::Single => "single",
            ExistsModifier::Multiple => "multiple",
        }
    }
}

/// `Necessity` (choice operations).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Necessity {
    Optional,
    Required,
}

impl Necessity {
    pub fn as_str(self) -> &'static str {
        match self {
            Necessity::Optional => "optional",
            Necessity::Required => "required",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArithmeticOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
}

impl ArithmeticOperator {
    pub fn as_str(self) -> &'static str {
        match self {
            ArithmeticOperator::Add => "+",
            ArithmeticOperator::Subtract => "-",
            ArithmeticOperator::Multiply => "*",
            ArithmeticOperator::Divide => "/",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicalOperator {
    And,
    Or,
}

impl LogicalOperator {
    pub fn as_str(self) -> &'static str {
        match self {
            LogicalOperator::And => "and",
            LogicalOperator::Or => "or",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EqualityOperator {
    Eq,
    NotEq,
}

impl EqualityOperator {
    pub fn as_str(self) -> &'static str {
        match self {
            EqualityOperator::Eq => "=",
            EqualityOperator::NotEq => "<>",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComparisonOperator {
    Ge,
    Le,
    Gt,
    Lt,
}

impl ComparisonOperator {
    pub fn as_str(self) -> &'static str {
        match self {
            ComparisonOperator::Ge => ">=",
            ComparisonOperator::Le => "<=",
            ComparisonOperator::Gt => ">",
            ComparisonOperator::Lt => "<",
        }
    }
}

/// An inline (closure) function: `a, b [body]` (explicit) or the implicit
/// bare form `then body` / `filter body`.
#[derive(Debug, Clone, PartialEq)]
pub struct InlineFunction {
    pub parameters: Vec<String>,
    pub body: Box<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SwitchGuard {
    /// `"literal"` guards keep the parsed literal.
    Literal(Box<Expr>),
    /// Reference guards (enum values, choice options, data types) keep the
    /// reference as written.
    Reference(String),
}

/// One arm of a `switch`: either `default <expr>` or `<guard> then <expr>`.
#[derive(Debug, Clone, PartialEq)]
pub struct SwitchCase {
    pub guard: Option<SwitchGuard>,
    pub expression: Box<Expr>,
}

impl SwitchCase {
    pub fn is_default(&self) -> bool {
        self.guard.is_none()
    }
}

/// `k: v` entry of a `with-meta { ... }` operation.
#[derive(Debug, Clone, PartialEq)]
pub struct WithMetaEntry {
    pub key: String,
    pub value: Box<Expr>,
}

/// `k: v` pair of a constructor expression.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstructorPair {
    pub key: String,
    pub value: Box<Expr>,
}

/// The expression tree. Variant names and JSON shapes mirror the
/// `RosettaExpression` EClasses; see `docs/compatibility.md`.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    BooleanLiteral {
        value: bool,
    },
    StringLiteral {
        value: String,
    },
    /// BigDecimal literals keep their source text verbatim.
    NumberLiteral {
        text: String,
    },
    /// Integer literals keep their source text verbatim (sign included).
    IntLiteral {
        text: String,
    },
    /// `[a, b]` — the keyword `empty` is the same node with no elements.
    ListLiteral {
        elements: Vec<Expr>,
    },
    SymbolReference {
        symbol: String,
        explicit_arguments: bool,
        raw_args: Vec<Expr>,
    },
    /// The implicit variable `item`.
    ImplicitVariable,
    FeatureCall {
        receiver: Box<Expr>,
        /// `None` for a bare `->` projection (feature optional in the grammar).
        feature: Option<String>,
    },
    DeepFeatureCall {
        receiver: Box<Expr>,
        feature: Option<String>,
    },
    ArithmeticOperation {
        operator: ArithmeticOperator,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    LogicalOperation {
        operator: LogicalOperator,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    EqualityOperation {
        operator: EqualityOperator,
        card_mod: CardinalityModifier,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    ComparisonOperation {
        operator: ComparisonOperator,
        card_mod: CardinalityModifier,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    ContainsExpression {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    DisjointExpression {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    DefaultOperation {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// `a join` (no explicit separator: the EMF model fills a generated
    /// `""` literal) or `a join sep`.
    JoinOperation {
        left: Box<Expr>,
        right: Box<Expr>,
        explicit_separator: bool,
    },
    /// `if c then a else b`; a missing `else` is a `full == false`
    /// conditional whose `elsethen` is the generated empty list literal,
    /// exactly as the EMF derived state models it.
    ConditionalExpression {
        if_: Box<Expr>,
        ifthen: Box<Expr>,
        elsethen: Box<Expr>,
        full: bool,
    },
    OnlyExistsExpression {
        args: Vec<Expr>,
        has_parentheses: bool,
    },
    ExistsExpression {
        modifier: ExistsModifier,
        argument: Box<Expr>,
    },
    AbsentExpression {
        argument: Box<Expr>,
    },
    OnlyElement {
        argument: Box<Expr>,
    },
    CountOperation {
        argument: Box<Expr>,
    },
    FlattenOperation {
        argument: Box<Expr>,
    },
    DistinctOperation {
        argument: Box<Expr>,
    },
    ReverseOperation {
        argument: Box<Expr>,
    },
    FirstOperation {
        argument: Box<Expr>,
    },
    LastOperation {
        argument: Box<Expr>,
    },
    SumOperation {
        argument: Box<Expr>,
    },
    AsKeyOperation {
        argument: Box<Expr>,
    },
    OneOfOperation {
        argument: Box<Expr>,
    },
    ChoiceOperation {
        necessity: Necessity,
        attributes: Vec<String>,
        argument: Box<Expr>,
    },
    ToStringOperation {
        argument: Box<Expr>,
    },
    ToNumberOperation {
        argument: Box<Expr>,
    },
    ToIntOperation {
        argument: Box<Expr>,
    },
    ToTimeOperation {
        argument: Box<Expr>,
    },
    ToEnumOperation {
        enumeration: String,
        argument: Box<Expr>,
    },
    ToDateOperation {
        argument: Box<Expr>,
    },
    ToDateTimeOperation {
        argument: Box<Expr>,
    },
    ToZonedDateTimeOperation {
        argument: Box<Expr>,
    },
    SwitchOperation {
        argument: Box<Expr>,
        cases: Vec<SwitchCase>,
    },
    WithMetaOperation {
        argument: Box<Expr>,
        entries: Vec<WithMetaEntry>,
    },
    AsOperation {
        type_: String,
        argument: Box<Expr>,
    },
    /// `a then b` — the body is an *implicit* inline function (bare
    /// expression, no brackets).
    ThenOperation {
        argument: Box<Expr>,
        function: Option<InlineFunction>,
    },
    FilterOperation {
        argument: Box<Expr>,
        function: Option<InlineFunction>,
    },
    /// The `extract` operator (`MapOperation` in the Ecore model).
    MapOperation {
        argument: Box<Expr>,
        function: Option<InlineFunction>,
    },
    ReduceOperation {
        argument: Box<Expr>,
        function: Option<InlineFunction>,
    },
    SortOperation {
        argument: Box<Expr>,
        function: Option<InlineFunction>,
    },
    MinOperation {
        argument: Box<Expr>,
        function: Option<InlineFunction>,
    },
    MaxOperation {
        argument: Box<Expr>,
        function: Option<InlineFunction>,
    },
    ConstructorExpression {
        type_call: super::TypeRef,
        values: Vec<ConstructorPair>,
        implicit_empty: bool,
    },
}

/// Serializing an `Expr` yields the same normalized JSON as
/// [`Expr::to_json`].
impl Serialize for Expr {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_json().serialize(serializer)
    }
}

impl Expr {
    /// Visit every direct sub-expression, in source order. Tooling
    /// (resolution, linting) walks the tree with this instead of
    /// duplicating the exhaustive match.
    pub fn for_each_child_mut<F: FnMut(&mut Expr)>(&mut self, f: &mut F) {
        match self {
            Expr::BooleanLiteral { .. }
            | Expr::StringLiteral { .. }
            | Expr::NumberLiteral { .. }
            | Expr::IntLiteral { .. }
            | Expr::ImplicitVariable => {}
            Expr::ListLiteral { elements } => {
                for e in elements {
                    f(e);
                }
            }
            Expr::SymbolReference { raw_args, .. } => {
                for a in raw_args {
                    f(a);
                }
            }
            Expr::FeatureCall { receiver, .. } | Expr::DeepFeatureCall { receiver, .. } => {
                f(receiver)
            }
            Expr::ArithmeticOperation { left, right, .. }
            | Expr::LogicalOperation { left, right, .. }
            | Expr::EqualityOperation { left, right, .. }
            | Expr::ComparisonOperation { left, right, .. }
            | Expr::ContainsExpression { left, right }
            | Expr::DisjointExpression { left, right }
            | Expr::DefaultOperation { left, right }
            | Expr::JoinOperation { left, right, .. } => {
                f(left);
                f(right);
            }
            Expr::ConditionalExpression {
                if_,
                ifthen,
                elsethen,
                ..
            } => {
                f(if_);
                f(ifthen);
                f(elsethen);
            }
            Expr::OnlyExistsExpression { args, .. } => {
                for a in args {
                    f(a);
                }
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
            | Expr::AsOperation { argument, .. } => f(argument),
            Expr::SwitchOperation { argument, cases } => {
                f(argument);
                for case in cases {
                    if let Some(SwitchGuard::Literal(literal)) = &mut case.guard {
                        f(literal);
                    }
                    f(&mut case.expression);
                }
            }
            Expr::ThenOperation { argument, function }
            | Expr::FilterOperation { argument, function }
            | Expr::MapOperation { argument, function }
            | Expr::ReduceOperation { argument, function }
            | Expr::SortOperation { argument, function }
            | Expr::MinOperation { argument, function }
            | Expr::MaxOperation { argument, function } => {
                f(argument);
                if let Some(function) = function {
                    f(&mut function.body);
                }
            }
            Expr::ConstructorExpression { values, .. } => {
                for pair in values {
                    f(&mut pair.value);
                }
            }
        }
    }

    /// The parameters of the inline function this node carries, if any
    /// (`filter a, b [...]`). Callers push these as a scope frame when
    /// descending into the body.
    pub fn inline_parameters(&self) -> Option<Vec<String>> {
        match self {
            Expr::ThenOperation { function, .. }
            | Expr::FilterOperation { function, .. }
            | Expr::MapOperation { function, .. }
            | Expr::ReduceOperation { function, .. }
            | Expr::SortOperation { function, .. }
            | Expr::MinOperation { function, .. }
            | Expr::MaxOperation { function, .. } => {
                function.as_ref().map(|f| f.parameters.clone())
            }
            _ => None,
        }
    }
}

impl Expr {
    /// The empty list literal (`empty` / `[]`).
    pub fn empty_list() -> Expr {
        Expr::ListLiteral {
            elements: Vec::new(),
        }
    }

    /// The generated `""` literal the EMF model inserts as a join
    /// separator when none is written.
    pub fn generated_empty_separator() -> Expr {
        Expr::StringLiteral {
            value: String::new(),
        }
    }

    /// Normalized JSON: the compatibility artifact. The Java oracle dumper
    /// (`tools/oracle-dumper`) emits the same shape; the mapping between
    /// EClass names and these tags is documented in
    /// `scripts/oracle_compare.py`.
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            Expr::BooleanLiteral { value } => json!({ "kind": "Boolean", "value": value }),
            Expr::StringLiteral { value } => json!({ "kind": "String", "value": value }),
            Expr::NumberLiteral { text } => json!({ "kind": "Number", "text": text }),
            Expr::IntLiteral { text } => json!({ "kind": "Int", "text": text }),
            Expr::ListLiteral { elements } => {
                json!({ "kind": "List", "elements": elements.iter().map(Expr::to_json).collect::<Vec<_>>() })
            }
            Expr::SymbolReference {
                symbol,
                explicit_arguments,
                raw_args,
            } => json!({
                "kind": "SymbolReference",
                "symbol": symbol,
                "explicit": explicit_arguments,
                "args": raw_args.iter().map(Expr::to_json).collect::<Vec<_>>(),
            }),
            Expr::ImplicitVariable => json!({ "kind": "ImplicitVariable" }),
            Expr::FeatureCall { receiver, feature } => json!({
                "kind": "FeatureCall",
                "receiver": receiver.to_json(),
                "feature": feature,
            }),
            Expr::DeepFeatureCall { receiver, feature } => json!({
                "kind": "DeepFeatureCall",
                "receiver": receiver.to_json(),
                "feature": feature,
            }),
            Expr::ArithmeticOperation {
                operator,
                left,
                right,
            } => json!({
                "kind": "Binary",
                "op": operator.as_str(),
                "left": left.to_json(),
                "right": right.to_json(),
            }),
            Expr::LogicalOperation {
                operator,
                left,
                right,
            } => json!({
                "kind": "Binary",
                "op": operator.as_str(),
                "left": left.to_json(),
                "right": right.to_json(),
            }),
            Expr::EqualityOperation {
                operator,
                card_mod,
                left,
                right,
            } => json!({
                "kind": "Binary",
                "op": operator.as_str(),
                "cardMod": card_mod.as_str(),
                "left": left.to_json(),
                "right": right.to_json(),
            }),
            Expr::ComparisonOperation {
                operator,
                card_mod,
                left,
                right,
            } => json!({
                "kind": "Binary",
                "op": operator.as_str(),
                "cardMod": card_mod.as_str(),
                "left": left.to_json(),
                "right": right.to_json(),
            }),
            Expr::ContainsExpression { left, right } => json!({
                "kind": "Binary", "op": "contains",
                "left": left.to_json(), "right": right.to_json(),
            }),
            Expr::DisjointExpression { left, right } => json!({
                "kind": "Binary", "op": "disjoint",
                "left": left.to_json(), "right": right.to_json(),
            }),
            Expr::DefaultOperation { left, right } => json!({
                "kind": "Binary", "op": "default",
                "left": left.to_json(), "right": right.to_json(),
            }),
            Expr::JoinOperation {
                left,
                right,
                explicit_separator,
            } => json!({
                "kind": "Join",
                "left": left.to_json(),
                "right": right.to_json(),
                "explicitSeparator": explicit_separator,
            }),
            Expr::ConditionalExpression {
                if_,
                ifthen,
                elsethen,
                full,
            } => json!({
                "kind": "Conditional",
                "if": if_.to_json(),
                "then": ifthen.to_json(),
                "else": elsethen.to_json(),
                "full": full,
            }),
            Expr::OnlyExistsExpression {
                args,
                has_parentheses,
            } => json!({
                "kind": "OnlyExists",
                "args": args.iter().map(Expr::to_json).collect::<Vec<_>>(),
                "parentheses": has_parentheses,
            }),
            Expr::ExistsExpression { modifier, argument } => json!({
                "kind": "Exists",
                "modifier": modifier.as_str(),
                "argument": argument.to_json(),
            }),
            Expr::AbsentExpression { argument } => {
                json!({ "kind": "Absent", "argument": argument.to_json() })
            }
            Expr::OnlyElement { argument } => {
                json!({ "kind": "OnlyElement", "argument": argument.to_json() })
            }
            Expr::CountOperation { argument } => {
                json!({ "kind": "Count", "argument": argument.to_json() })
            }
            Expr::FlattenOperation { argument } => {
                json!({ "kind": "Flatten", "argument": argument.to_json() })
            }
            Expr::DistinctOperation { argument } => {
                json!({ "kind": "Distinct", "argument": argument.to_json() })
            }
            Expr::ReverseOperation { argument } => {
                json!({ "kind": "Reverse", "argument": argument.to_json() })
            }
            Expr::FirstOperation { argument } => {
                json!({ "kind": "First", "argument": argument.to_json() })
            }
            Expr::LastOperation { argument } => {
                json!({ "kind": "Last", "argument": argument.to_json() })
            }
            Expr::SumOperation { argument } => {
                json!({ "kind": "Sum", "argument": argument.to_json() })
            }
            Expr::AsKeyOperation { argument } => {
                json!({ "kind": "AsKey", "argument": argument.to_json() })
            }
            Expr::OneOfOperation { argument } => {
                json!({ "kind": "OneOf", "argument": argument.to_json() })
            }
            Expr::ChoiceOperation {
                necessity,
                attributes,
                argument,
            } => json!({
                "kind": "Choice",
                "necessity": necessity.as_str(),
                "attributes": attributes,
                "argument": argument.to_json(),
            }),
            Expr::ToStringOperation { argument } => {
                json!({ "kind": "ToString", "argument": argument.to_json() })
            }
            Expr::ToNumberOperation { argument } => {
                json!({ "kind": "ToNumber", "argument": argument.to_json() })
            }
            Expr::ToIntOperation { argument } => {
                json!({ "kind": "ToInt", "argument": argument.to_json() })
            }
            Expr::ToTimeOperation { argument } => {
                json!({ "kind": "ToTime", "argument": argument.to_json() })
            }
            Expr::ToEnumOperation {
                enumeration,
                argument,
            } => json!({
                "kind": "ToEnum",
                "enumeration": enumeration,
                "argument": argument.to_json(),
            }),
            Expr::ToDateOperation { argument } => {
                json!({ "kind": "ToDate", "argument": argument.to_json() })
            }
            Expr::ToDateTimeOperation { argument } => {
                json!({ "kind": "ToDateTime", "argument": argument.to_json() })
            }
            Expr::ToZonedDateTimeOperation { argument } => {
                json!({ "kind": "ToZonedDateTime", "argument": argument.to_json() })
            }
            Expr::SwitchOperation { argument, cases } => json!({
                "kind": "Switch",
                "argument": argument.to_json(),
                "cases": cases.iter().map(|c| {
                    let mut json = serde_json::Map::new();
                    match &c.guard {
                        Some(SwitchGuard::Literal(expr)) => {
                            json.insert("guard".into(), json!({ "kind": "Literal", "value": expr.to_json() }));
                        }
                        Some(SwitchGuard::Reference(name)) => {
                            json.insert("guard".into(), json!({ "kind": "Reference", "target": name }));
                        }
                        None => {
                            json.insert("default".into(), json!(true));
                        }
                    }
                    json.insert("expression".into(), c.expression.to_json());
                    serde_json::Value::Object(json)
                }).collect::<Vec<_>>(),
            }),
            Expr::WithMetaOperation { argument, entries } => json!({
                "kind": "WithMeta",
                "argument": argument.to_json(),
                "entries": entries.iter().map(|e| json!({
                    "key": e.key,
                    "value": e.value.to_json(),
                })).collect::<Vec<_>>(),
            }),
            Expr::AsOperation { type_, argument } => json!({
                "kind": "As",
                "type": type_,
                "argument": argument.to_json(),
            }),
            Expr::ThenOperation { argument, function } => json!({
                "kind": "Then",
                "argument": argument.to_json(),
                "function": inline_function_json(function.as_ref()),
            }),
            Expr::FilterOperation { argument, function } => json!({
                "kind": "Filter",
                "argument": argument.to_json(),
                "function": inline_function_json(function.as_ref()),
            }),
            Expr::MapOperation { argument, function } => json!({
                "kind": "Map",
                "argument": argument.to_json(),
                "function": inline_function_json(function.as_ref()),
            }),
            Expr::ReduceOperation { argument, function } => json!({
                "kind": "Reduce",
                "argument": argument.to_json(),
                "function": inline_function_json(function.as_ref()),
            }),
            Expr::SortOperation { argument, function } => json!({
                "kind": "Sort",
                "argument": argument.to_json(),
                "function": inline_function_json(function.as_ref()),
            }),
            Expr::MinOperation { argument, function } => json!({
                "kind": "Min",
                "argument": argument.to_json(),
                "function": inline_function_json(function.as_ref()),
            }),
            Expr::MaxOperation { argument, function } => json!({
                "kind": "Max",
                "argument": argument.to_json(),
                "function": inline_function_json(function.as_ref()),
            }),
            Expr::ConstructorExpression {
                type_call,
                values,
                implicit_empty,
            } => json!({
                "kind": "Constructor",
                "type": {
                    "name": type_call.name,
                    "arguments": type_call.arguments,
                },
                "values": values.iter().map(|p| json!({
                    "key": p.key,
                    "value": p.value.to_json(),
                })).collect::<Vec<_>>(),
                "implicitEmpty": implicit_empty,
            }),
        }
    }
}

fn inline_function_json(function: Option<&InlineFunction>) -> serde_json::Value {
    match function {
        None => serde_json::Value::Null,
        Some(f) => json!({
            "parameters": f.parameters,
            "body": f.body.to_json(),
        }),
    }
}

// ---- printer ---------------------------------------------------------------

/// Binding powers, loosest first. A node may be printed bare in any
/// context whose required precedence is <= its own; tighter contexts get
/// parentheses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Precedence {
    /// Nodes whose rendered text would swallow a following full expression
    /// (switch cases, with-meta and constructor values): only faithful at
    /// the very root of a printed expression.
    Top,
    /// `then` chains and `as-key` — only valid at expression top level.
    Then,
    Or,
    And,
    Equality,
    Comparison,
    Additive,
    Multiplicative,
    /// `contains`, `disjoint`, `default`, `join`.
    WordBinary,
    /// Postfix/unary operations and feature calls.
    Postfix,
    Primary,
}

impl Expr {
    /// The tightest precedence level at which this node may appear without
    /// being reinterpreted differently on re-parse.
    fn precedence(&self) -> Precedence {
        match self {
            // A function-less `filter` / `extract` / `reduce` / `then`
            // would greedily capture any following expression as its
            // implicit function body, so it is bare only at the root.
            Expr::FilterOperation { function: None, .. }
            | Expr::MapOperation { function: None, .. }
            | Expr::ReduceOperation { function: None, .. }
            | Expr::ThenOperation { function: None, .. } => Precedence::Top,
            Expr::ThenOperation { .. } | Expr::AsKeyOperation { .. } => Precedence::Then,
            // `if .. then .. else ..` is a primary, but its unparenthesized
            // form is only faithful at the very top of an expression:
            // postfix operators would otherwise bind inside its branches.
            Expr::ConditionalExpression { .. } => Precedence::Then,
            Expr::LogicalOperation {
                operator: LogicalOperator::Or,
                ..
            } => Precedence::Or,
            Expr::LogicalOperation {
                operator: LogicalOperator::And,
                ..
            } => Precedence::And,
            Expr::EqualityOperation { .. } => Precedence::Equality,
            Expr::ComparisonOperation { .. } => Precedence::Comparison,
            Expr::ArithmeticOperation {
                operator: ArithmeticOperator::Add | ArithmeticOperator::Subtract,
                ..
            } => Precedence::Additive,
            Expr::ArithmeticOperation { .. } => Precedence::Multiplicative,
            Expr::ContainsExpression { .. }
            | Expr::DisjointExpression { .. }
            | Expr::DefaultOperation { .. }
            | Expr::JoinOperation { .. } => Precedence::WordBinary,
            // Switch cases, with-meta values and constructor values are
            // full expressions: printed bare they would swallow any
            // postfix operator that follows the node, so they are only
            // printed bare at the very top of an expression.
            Expr::ConstructorExpression { .. }
            | Expr::SwitchOperation { .. }
            | Expr::WithMetaOperation { .. } => Precedence::Top,
            // A bare `->` projection must only be printed bare at the very
            // top of an expression: followed by more postfix steps it would
            // greedily capture the operator word as its feature name
            // (`a-> filter [..]` parses as `a->filter [..]`).
            Expr::FeatureCall { feature: None, .. }
            | Expr::DeepFeatureCall { feature: None, .. } => Precedence::Then,
            _ => Precedence::Postfix,
        }
    }

    /// Render this expression as valid `.rosetta` text, parenthesizing
    /// wherever the surface grammar would otherwise parse it differently.
    pub fn print(&self) -> String {
        let mut out = String::new();
        self.write(Precedence::Then, &mut out);
        out
    }

    fn write(&self, min: Precedence, out: &mut String) {
        if self.precedence() < min {
            out.push('(');
            self.write_unparenthesized(Precedence::Top, out);
            out.push(')');
            return;
        }
        self.write_unparenthesized(min, out);
    }

    fn write_unparenthesized(&self, _min: Precedence, out: &mut String) {
        match self {
            Expr::BooleanLiteral { value } => out.push_str(if *value { "True" } else { "False" }),
            Expr::StringLiteral { value } => {
                out.push('"');
                for c in value.chars() {
                    match c {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\t' => out.push_str("\\t"),
                        '\r' => out.push_str("\\r"),
                        other => out.push(other),
                    }
                }
                out.push('"');
            }
            Expr::NumberLiteral { text } => out.push_str(text),
            Expr::IntLiteral { text } => out.push_str(text),
            Expr::ListLiteral { elements } => {
                if elements.is_empty() {
                    out.push_str("empty");
                    return;
                }
                out.push('[');
                for (i, e) in elements.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    e.write(Precedence::Then, out);
                }
                out.push(']');
            }
            Expr::SymbolReference {
                symbol,
                explicit_arguments,
                raw_args,
            } => {
                out.push_str(symbol);
                if *explicit_arguments {
                    out.push('(');
                    for (i, a) in raw_args.iter().enumerate() {
                        if i > 0 {
                            out.push_str(", ");
                        }
                        a.write(Precedence::Then, out);
                    }
                    out.push(')');
                }
            }
            Expr::ImplicitVariable => out.push_str("item"),
            Expr::FeatureCall {
                receiver,
                feature: None,
            } => {
                out.push('(');
                receiver.write(Precedence::Postfix, out);
                out.push_str("->)");
            }
            Expr::FeatureCall {
                receiver,
                feature: Some(feature),
            } => {
                receiver.write(Precedence::Postfix, out);
                out.push_str("->");
                out.push_str(feature);
            }
            Expr::DeepFeatureCall {
                receiver,
                feature: None,
            } => {
                out.push('(');
                receiver.write(Precedence::Postfix, out);
                out.push_str("->>)");
            }
            Expr::DeepFeatureCall {
                receiver,
                feature: Some(feature),
            } => {
                receiver.write(Precedence::Postfix, out);
                out.push_str("->>");
                out.push_str(feature);
            }
            Expr::ArithmeticOperation {
                operator,
                left,
                right,
            } => {
                let level = Self::precedence_of(self);
                left.write(level, out);
                out.push(' ');
                out.push_str(operator.as_str());
                out.push(' ');
                right.write(level.successor(), out);
            }
            Expr::LogicalOperation {
                operator,
                left,
                right,
            } => {
                left.write(Self::precedence_of(self), out);
                out.push(' ');
                out.push_str(operator.as_str());
                out.push(' ');
                right.write(Self::precedence_of(self).successor(), out);
            }
            Expr::EqualityOperation {
                operator,
                card_mod,
                left,
                right,
            } => {
                left.write(Self::precedence_of(self), out);
                out.push(' ');
                match card_mod {
                    CardinalityModifier::None => {}
                    other => {
                        out.push_str(other.as_str());
                        out.push(' ');
                    }
                }
                out.push_str(operator.as_str());
                out.push(' ');
                right.write(Self::precedence_of(self).successor(), out);
            }
            Expr::ComparisonOperation {
                operator,
                card_mod,
                left,
                right,
            } => {
                left.write(Self::precedence_of(self), out);
                out.push(' ');
                match card_mod {
                    CardinalityModifier::None => {}
                    other => {
                        out.push_str(other.as_str());
                        out.push(' ');
                    }
                }
                out.push_str(operator.as_str());
                out.push(' ');
                right.write(Self::precedence_of(self).successor(), out);
            }
            Expr::ContainsExpression { left, right }
            | Expr::DisjointExpression { left, right }
            | Expr::DefaultOperation { left, right } => {
                let op = match self {
                    Expr::ContainsExpression { .. } => "contains",
                    Expr::DisjointExpression { .. } => "disjoint",
                    _ => "default",
                };
                left.write(Self::precedence_of(self), out);
                out.push(' ');
                out.push_str(op);
                out.push(' ');
                right.write(Self::precedence_of(self).successor(), out);
            }
            Expr::JoinOperation {
                left,
                right,
                explicit_separator,
            } => {
                left.write(Precedence::WordBinary, out);
                out.push_str(" join");
                if *explicit_separator {
                    out.push(' ');
                    right.write(Precedence::WordBinary.successor(), out);
                }
            }
            Expr::ConditionalExpression {
                if_,
                ifthen,
                elsethen,
                full,
            } => {
                out.push_str("if ");
                if_.write(Precedence::Or, out);
                out.push_str(" then ");
                ifthen.write(Precedence::Or, out);
                if *full {
                    out.push_str(" else ");
                    elsethen.write(Precedence::Or, out);
                }
            }
            Expr::OnlyExistsExpression {
                args,
                has_parentheses,
            } => {
                let parens = *has_parentheses || args.len() > 1;
                if parens {
                    out.push('(');
                }
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    a.write(Precedence::Postfix, out);
                }
                if parens {
                    out.push(')');
                }
                out.push_str(" only exists");
            }
            Expr::ExistsExpression { modifier, argument } => {
                argument.write(Precedence::Postfix, out);
                out.push(' ');
                if *modifier != ExistsModifier::None {
                    out.push_str(modifier.as_str());
                    out.push(' ');
                }
                out.push_str("exists");
            }
            Expr::AbsentExpression { argument } => {
                argument.write(Precedence::Postfix, out);
                out.push_str(" is absent");
            }
            Expr::OnlyElement { argument } => {
                argument.write(Precedence::Postfix, out);
                out.push_str(" only-element");
            }
            Expr::CountOperation { argument } => {
                argument.write(Precedence::Postfix, out);
                out.push_str(" count");
            }
            Expr::FlattenOperation { argument } => {
                argument.write(Precedence::Postfix, out);
                out.push_str(" flatten");
            }
            Expr::DistinctOperation { argument } => {
                argument.write(Precedence::Postfix, out);
                out.push_str(" distinct");
            }
            Expr::ReverseOperation { argument } => {
                argument.write(Precedence::Postfix, out);
                out.push_str(" reverse");
            }
            Expr::FirstOperation { argument } => {
                argument.write(Precedence::Postfix, out);
                out.push_str(" first");
            }
            Expr::LastOperation { argument } => {
                argument.write(Precedence::Postfix, out);
                out.push_str(" last");
            }
            Expr::SumOperation { argument } => {
                argument.write(Precedence::Postfix, out);
                out.push_str(" sum");
            }
            Expr::AsKeyOperation { argument } => {
                argument.write(Precedence::Then, out);
                out.push_str(" as-key");
            }
            Expr::OneOfOperation { argument } => {
                argument.write(Precedence::Postfix, out);
                out.push_str(" one-of");
            }
            Expr::ChoiceOperation {
                necessity,
                attributes,
                argument,
            } => {
                argument.write(Precedence::Postfix, out);
                out.push(' ');
                out.push_str(necessity.as_str());
                out.push_str(" choice ");
                out.push_str(&attributes.join(", "));
            }
            Expr::ToStringOperation { argument } => {
                argument.write(Precedence::Postfix, out);
                out.push_str(" to-string");
            }
            Expr::ToNumberOperation { argument } => {
                argument.write(Precedence::Postfix, out);
                out.push_str(" to-number");
            }
            Expr::ToIntOperation { argument } => {
                argument.write(Precedence::Postfix, out);
                out.push_str(" to-int");
            }
            Expr::ToTimeOperation { argument } => {
                argument.write(Precedence::Postfix, out);
                out.push_str(" to-time");
            }
            Expr::ToEnumOperation {
                enumeration,
                argument,
            } => {
                argument.write(Precedence::Postfix, out);
                out.push_str(" to-enum ");
                out.push_str(enumeration);
            }
            Expr::ToDateOperation { argument } => {
                argument.write(Precedence::Postfix, out);
                out.push_str(" to-date");
            }
            Expr::ToDateTimeOperation { argument } => {
                argument.write(Precedence::Postfix, out);
                out.push_str(" to-date-time");
            }
            Expr::ToZonedDateTimeOperation { argument } => {
                argument.write(Precedence::Postfix, out);
                out.push_str(" to-zoned-date-time");
            }
            Expr::SwitchOperation { argument, cases } => {
                argument.write(Precedence::Postfix, out);
                out.push_str(" switch ");
                for (i, case) in cases.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    match &case.guard {
                        None => out.push_str("default "),
                        Some(SwitchGuard::Literal(lit)) => {
                            lit.write(Precedence::Postfix, out);
                            out.push_str(" then ");
                        }
                        Some(SwitchGuard::Reference(name)) => {
                            out.push_str(name);
                            out.push_str(" then ");
                        }
                    }
                    case.expression.write(Precedence::Then, out);
                }
            }
            Expr::WithMetaOperation { argument, entries } => {
                argument.write(Precedence::Postfix, out);
                out.push_str(" with-meta");
                if !entries.is_empty() {
                    out.push_str(" { ");
                    for (i, e) in entries.iter().enumerate() {
                        if i > 0 {
                            out.push_str(", ");
                        }
                        out.push_str(&e.key);
                        out.push_str(": ");
                        e.value.write(Precedence::Then, out);
                    }
                    out.push_str(" }");
                }
            }
            Expr::AsOperation { type_, argument } => {
                argument.write(Precedence::Postfix, out);
                out.push_str(" as ");
                out.push_str(type_);
            }
            Expr::ThenOperation { argument, function } => {
                // `then` chains are left-associative and their bodies are
                // implicit inline functions (no brackets).
                argument.write(Precedence::Then, out);
                out.push_str(" then");
                if let Some(f) = function {
                    out.push(' ');
                    f.body.write(Precedence::Or, out);
                }
            }
            Expr::FilterOperation { argument, function }
            | Expr::MapOperation { argument, function }
            | Expr::ReduceOperation { argument, function }
            | Expr::SortOperation { argument, function }
            | Expr::MinOperation { argument, function }
            | Expr::MaxOperation { argument, function } => {
                let op = match self {
                    Expr::FilterOperation { .. } => "filter",
                    Expr::MapOperation { .. } => "extract",
                    Expr::ReduceOperation { .. } => "reduce",
                    Expr::SortOperation { .. } => "sort",
                    Expr::MinOperation { .. } => "min",
                    _ => "max",
                };
                argument.write(Precedence::Postfix, out);
                out.push(' ');
                out.push_str(op);
                if let Some(f) = function {
                    out.push(' ');
                    if !f.parameters.is_empty() {
                        out.push_str(&f.parameters.join(", "));
                        out.push(' ');
                    }
                    out.push('[');
                    f.body.write(Precedence::Then, out);
                    out.push(']');
                }
            }
            Expr::ConstructorExpression {
                type_call,
                values,
                implicit_empty,
            } => {
                out.push_str(&type_call.name);
                if !type_call.arguments.is_empty() {
                    out.push('(');
                    for (i, a) in type_call.arguments.iter().enumerate() {
                        if i > 0 {
                            out.push_str(", ");
                        }
                        out.push_str(&a.parameter);
                        out.push_str(": ");
                        out.push_str(&super::argument_value_text(&a.argument_value));
                    }
                    out.push(')');
                }
                out.push_str(" { ");
                for (i, p) in values.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&p.key);
                    out.push_str(": ");
                    p.value.write(Precedence::Then, out);
                }
                if *implicit_empty {
                    if !values.is_empty() {
                        out.push_str(", ");
                    }
                    out.push_str("...");
                }
                out.push_str(" }");
            }
        }
    }

    fn precedence_of(expr: &Expr) -> Precedence {
        expr.precedence()
    }
}

impl Precedence {
    /// The precedence a *right-hand operand* must satisfy to be printed
    /// bare under a left-associative operator at this level.
    fn successor(self) -> Precedence {
        match self {
            Precedence::Top => Precedence::Top,
            Precedence::Then => Precedence::Or,
            Precedence::Or => Precedence::And,
            Precedence::And => Precedence::Equality,
            Precedence::Equality => Precedence::Comparison,
            Precedence::Comparison => Precedence::Additive,
            Precedence::Additive => Precedence::Multiplicative,
            Precedence::Multiplicative => Precedence::WordBinary,
            Precedence::WordBinary => Precedence::Postfix,
            Precedence::Postfix => Precedence::Primary,
            Precedence::Primary => Precedence::Primary,
        }
    }
}
