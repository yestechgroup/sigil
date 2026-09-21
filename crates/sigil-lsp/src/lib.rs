//! sigil-lsp: a language server for the Rune (`.rosetta`) DSL.
//!
//! The server is transport-agnostic: [`run_server`] speaks the LSP protocol
//! over any [`lsp_server::Connection`] (stdio in `main.rs`, an in-memory
//! pair in tests). Analysis is deliberately simple and robust: on every
//! change the *entire workspace* is re-parsed and re-resolved together
//! (`.rosetta` models are multi-file; a change in one file can affect any
//! other). This is fast enough for real models and always consistent.

pub mod analysis;
pub mod dispatch;
pub mod document;
pub mod features;
pub mod position;
pub mod world;

pub use analysis::Analysis;
pub use dispatch::run_server;
pub use document::Document;
pub use position::PositionEncoding;
pub use world::World;

use lsp_types::{request::Request, TextDocumentIdentifier};

/// Test/debug support: fetch the server's current text for a document.
/// Also serves tests as a FIFO barrier — by the time the response arrives,
/// all effects of earlier notifications (diagnostics included) are visible.
#[derive(Debug)]
pub enum DocumentTextRequest {}

impl Request for DocumentTextRequest {
    type Params = DocumentTextParams;
    type Result = Option<String>;
    const METHOD: &'static str = "sigil/textDocument";
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct DocumentTextParams {
    #[serde(rename = "textDocument")]
    pub text_document: TextDocumentIdentifier,
}
