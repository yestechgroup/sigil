//! Sigil's parser for the Rune (`.rosetta`) DSL.
//!
//! Produces a lossless-enough spanned AST ([`ast`]) from source text. The
//! AST is deliberately decoupled from the semantic model in `sigil-model` so
//! that the parser remains replaceable.

pub mod ast;
mod lower;
mod parser;

pub use lower::lower;

pub use parser::{
    annotation_path, annotation_ref, doc_reference, label_annotation, parse, rule_reference,
    source_unit,
};
pub use sigil_diag::{Diagnostic, SourceFile, Span};
