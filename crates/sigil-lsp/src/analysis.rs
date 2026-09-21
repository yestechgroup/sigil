//! Workspace analysis: parse + resolve every document together.

use std::collections::BTreeMap;

use lsp_types::DiagnosticSeverity;
use sigil_diag::SourceFile;

use crate::document::Document;
use crate::position::PositionEncoding;

/// The result of analyzing the whole workspace. Rebuilt from scratch on
/// every change — models are multi-file, so a change in one file can
/// invalidate resolution results in any other.
pub struct Analysis {
    pub resolution: sigil_resolve::Resolution,
    /// Syntax diagnostics per file (keyed by URI, i.e. the `SourceFile`
    /// name used for parsing).
    pub syntax: BTreeMap<String, Vec<sigil_diag::Diagnostic>>,
    /// The built-in library's source text, keyed by `builtin:<file name>`,
    /// so hover/definition can map spans inside `com.rosetta.model` files.
    pub builtin_docs: BTreeMap<String, Document>,
}

impl Analysis {
    pub fn empty() -> Self {
        Analysis {
            resolution: sigil_resolve::resolve(Vec::new()),
            syntax: BTreeMap::new(),
            builtin_docs: builtin_documents(),
        }
    }

    pub fn compute(docs: &BTreeMap<String, Document>) -> Self {
        let mut syntax = BTreeMap::new();
        let mut models = Vec::new();
        for (uri, doc) in docs {
            let source = SourceFile::new(uri.clone(), doc.text.clone());
            let (unit, diags) = sigil_syntax::parse(&source);
            syntax.insert(uri.clone(), diags);
            if let Some(unit) = unit {
                models.push(sigil_syntax::lower(uri, &unit));
            }
        }
        Analysis {
            resolution: sigil_resolve::resolve(models),
            syntax,
            builtin_docs: builtin_documents(),
        }
    }

    /// Convert the analysis results for one document into LSP diagnostics.
    /// Includes both syntax diagnostics (`E0001`, `W0001`, ...) and
    /// resolution diagnostics (`E01xx`).
    pub fn diagnostics_for(
        &self,
        uri: &str,
        doc: &Document,
        encoding: PositionEncoding,
    ) -> Vec<lsp_types::Diagnostic> {
        let mut out = Vec::new();
        for d in self.syntax.get(uri).map(Vec::as_slice).unwrap_or(&[]) {
            out.push(to_lsp(d, doc, encoding));
        }
        for d in &self.resolution.diagnostics {
            if d.file == uri {
                if let Some(span) = d.span {
                    out.push(lsp_types::Diagnostic {
                        range: doc.range(span, encoding),
                        severity: Some(severity(d.severity)),
                        code: Some(lsp_types::NumberOrString::String(d.code.to_string())),
                        code_description: None,
                        source: Some("sigil".to_string()),
                        message: d.message.clone(),
                        related_information: None,
                        tags: None,
                        data: None,
                    });
                }
            }
        }
        out
    }
}

fn to_lsp(
    d: &sigil_diag::Diagnostic,
    doc: &Document,
    encoding: PositionEncoding,
) -> lsp_types::Diagnostic {
    lsp_types::Diagnostic {
        range: doc.range(d.span, encoding),
        severity: Some(severity(d.severity)),
        code: Some(lsp_types::NumberOrString::String(d.code.to_string())),
        code_description: None,
        source: Some("sigil".to_string()),
        message: d.message.clone(),
        related_information: None,
        tags: None,
        data: None,
    }
}

fn severity(s: sigil_diag::Severity) -> DiagnosticSeverity {
    match s {
        sigil_diag::Severity::Error => DiagnosticSeverity::ERROR,
        sigil_diag::Severity::Warning => DiagnosticSeverity::WARNING,
    }
}

/// Pseudo-URI prefix for built-in library files (they are not workspace
/// documents but hover/definition can point into them).
pub const BUILTIN_SCHEME: &str = "builtin:";

fn builtin_documents() -> BTreeMap<String, Document> {
    sigil_resolve::builtin_sources()
        .into_iter()
        .map(|(name, text)| (format!("{BUILTIN_SCHEME}{name}"), Document::new(text, 0)))
        .collect()
}
