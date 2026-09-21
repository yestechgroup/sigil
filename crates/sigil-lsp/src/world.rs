//! Server state: the workspace root, open documents, and the current
//! analysis.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::analysis::Analysis;
use crate::document::Document;
use crate::position::PositionEncoding;

/// All server state. Documents are keyed by URI string and stored in a
/// `BTreeMap` so analysis order is deterministic (sorted by URI).
pub struct World {
    root: Option<PathBuf>,
    encoding: PositionEncoding,
    docs: BTreeMap<String, Document>,
    analysis: Analysis,
}

impl World {
    pub fn new(root: Option<PathBuf>) -> Self {
        World {
            root,
            encoding: PositionEncoding::Utf16,
            docs: BTreeMap::new(),
            analysis: Analysis::empty(),
        }
    }

    pub fn root(&self) -> Option<&PathBuf> {
        self.root.as_ref()
    }

    pub fn set_root(&mut self, root: Option<PathBuf>) {
        if self.root.is_none() {
            self.root = root;
        }
    }

    pub fn encoding(&self) -> PositionEncoding {
        self.encoding
    }

    pub fn set_encoding(&mut self, encoding: PositionEncoding) {
        self.encoding = encoding;
    }

    // ---- documents ------------------------------------------------------

    pub fn document(&self, uri: &str) -> Option<&Document> {
        self.docs.get(uri)
    }

    pub fn docs(&self) -> &BTreeMap<String, Document> {
        &self.docs
    }

    /// Test/debug support: the server's current text for a document.
    pub fn snapshot_text(&self, uri: &str) -> Option<String> {
        self.docs.get(uri).map(|d| d.text.clone())
    }

    pub fn open_document(&mut self, uri: &str, text: String, version: i32) {
        self.docs
            .insert(uri.to_string(), Document::new(text, version));
    }

    /// Apply a didChange batch. Returns false when the document is unknown.
    pub fn change_document(
        &mut self,
        uri: &str,
        version: i32,
        changes: impl IntoIterator<Item = lsp_types::TextDocumentContentChangeEvent>,
    ) -> bool {
        match self.docs.get_mut(uri) {
            Some(doc) => {
                let encoding = self.encoding;
                doc.apply_content_changes(version, changes, encoding);
                true
            }
            None => false,
        }
    }

    /// Apply a didClose: the document leaves the workspace (it is not
    /// re-read from disk — analysis covers exactly the open documents plus
    /// files scanned at initialize that have not been closed).
    pub fn close_document(&mut self, uri: &str) -> bool {
        self.docs.remove(uri).is_some()
    }

    // ---- workspace ------------------------------------------------------

    /// Load every `*.rosetta` file under the workspace root (skipping hidden
    /// and `target` directories) that is not already open. Files scanned
    /// here participate in analysis so that cross-file references resolve
    /// before the other file is opened in an editor.
    pub fn scan_workspace(&mut self) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let mut found = Vec::new();
        collect_rosetta_files(&root, &mut found, 0);
        for path in found {
            let Some(uri) = path_to_uri(&path) else {
                continue;
            };
            if self.docs.contains_key(&uri) {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(&path) {
                self.docs.insert(uri, Document::new(text, 0));
            }
        }
    }

    /// Re-parse and re-resolve every document in the workspace together.
    /// Full-workspace reanalysis keeps results consistent across files; the
    /// built-in `com.rosetta.model` library is included by `sigil_resolve`.
    pub fn reanalyze(&mut self) {
        self.analysis = Analysis::compute(&self.docs);
    }

    pub fn analysis(&self) -> &Analysis {
        &self.analysis
    }
}

fn collect_rosetta_files(dir: &std::path::Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 16 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with('.') || name == "target" {
            continue;
        }
        if path.is_dir() {
            collect_rosetta_files(&path, out, depth + 1);
        } else if name.ends_with(".rosetta") {
            out.push(path);
        }
    }
}

/// Best-effort `file://` URI for a path. No percent-encoding: workspace
/// paths with spaces or non-ASCII will not round-trip (documented
/// limitation).
fn path_to_uri(path: &std::path::Path) -> Option<String> {
    let text = path.to_str()?;
    if text.starts_with("file://") {
        return Some(text.to_string());
    }
    Some(format!("file://{text}"))
}
