//! Language feature implementations (documentSymbol, definition, hover,
//! completion, references, rename, workspace/symbol).
//!
//! All queries read from [`Analysis`](crate::Analysis) (the resolved model,
//! which carries byte spans) plus the documents' line indexes for position
//! mapping.

use std::collections::HashMap;

use sigil_diag::Span;
use sigil_model::{Attribute, EnumValue, SemanticElement, TypeRef};
use sigil_syntax::ast::{Element, SourceUnit};

use crate::document::Document;
use crate::position::PositionEncoding;
use crate::world::World;

// ---- shared span helpers -------------------------------------------------

/// Find the exact sub-span of `name` inside `span` (used because most AST
/// nodes carry the name as a plain `String` without its own span). The
/// match must be delimited by non-identifier characters; falls back to the
/// whole span.
pub(crate) fn find_name_region(text: &str, span: Span, name: &str) -> Span {
    if name.is_empty() || span.end > text.len() || span.start > span.end {
        return span;
    }
    let hay = &text[span.start..span.end.min(text.len())];
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    let bytes = name.len();
    let mut from = 0;
    while let Some(rel) = hay[from..].find(name) {
        let start = from + rel;
        let end = start + bytes;
        let before_ok = start == 0 || !hay[..start].chars().next_back().is_some_and(ident);
        let after_ok = end == hay.len() || !hay[end..].chars().next().is_some_and(ident);
        if before_ok && after_ok {
            return Span::new(span.start + start, span.start + end);
        }
        from = start + 1;
    }
    span
}

/// Trim trailing whitespace from a span: parser spans absorb it, and
/// whole-node hit-testing must not bleed onto following blank lines.
fn trim_span_end(text: &str, span: Span) -> Span {
    let mut end = span.end.min(text.len());
    while end > span.start && text[..end].ends_with([' ', '\t', '\n', '\r']) {
        end -= 1;
    }
    Span::new(span.start, end)
}

/// The sub-span covering the whole (possibly qualified, possibly
/// `^`-escaped) name of a type reference: `number(digits: 30)` ->
/// `number`, `a.b.^enum` -> `a.b.^enum`. The reference's `name` is the
/// *unescaped* qualified name, so the region is re-derived from the text
/// rather than from `name.len()`.
fn type_ref_name_span(r: &TypeRef, text: &str) -> Span {
    let end = r.span.end.min(text.len());
    let bytes = text.as_bytes();
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut i = r.span.start.min(end);
    if bytes.get(i) == Some(&b'^') {
        i += 1; // caret escape: part of the token, not of the name
    }
    while i < end {
        match bytes[i] {
            c if ident(c) => i += 1,
            b'.' => {
                // part of the qualified name only if a segment follows
                let mut j = i + 1;
                if bytes.get(j) == Some(&b'^') {
                    j += 1;
                }
                if bytes.get(j).is_some_and(|&c| ident(c)) {
                    i = j;
                } else {
                    break;
                }
            }
            _ => break,
        }
    }
    Span::new(r.span.start.min(end), i)
}

fn range_of(doc: &Document, span: Span, enc: PositionEncoding) -> lsp_types::Range {
    doc.range(span, enc)
}

fn parse_unit(uri: &str, text: &str) -> Option<SourceUnit> {
    let source = sigil_diag::SourceFile::new(uri, text.to_string());
    sigil_syntax::parse(&source).0
}

// ---- model traversal -------------------------------------------------------

/// One interesting node of the resolved model, carrying borrowed references
/// plus the flat element ids needed to map to declarations.
#[derive(Clone, Copy)]
enum Node<'a> {
    Element(usize),
    Attribute(&'a Attribute, usize),
    EnumValue(&'a EnumValue, usize),
    TypeRef(&'a TypeRef),
    AnnotationRef(&'a sigil_model::AnnotationRef),
}

/// A candidate hit: a span that may contain the cursor. `priority == 0`
/// marks *name* spans (exact identifier regions); `1` marks whole-node
/// spans.
struct Candidate<'a> {
    priority: u8,
    span: Span,
    node: Node<'a>,
}

/// Collect every node candidate of the resolved model for one file, with
/// both its exact name span (when computable) and its full span.
fn candidates<'a>(world: &'a World, uri: &str) -> Vec<Candidate<'a>> {
    let analysis = world.analysis();
    let resolution = &analysis.resolution;
    let Some(text) = world.document(uri).map(|d| d.text.clone()) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    let mut flat = 0usize;
    let push_type_ref = |r: &'a TypeRef, out: &mut Vec<Candidate<'a>>| {
        out.push(Candidate {
            priority: 0,
            span: type_ref_name_span(r, &text),
            node: Node::TypeRef(r),
        });
        out.push(Candidate {
            priority: 1,
            span: trim_span_end(&text, r.span),
            node: Node::TypeRef(r),
        });
    };
    let push_annotations = |annos: &'a [sigil_model::AnnotationRef],
                            out: &mut Vec<Candidate<'a>>| {
        for a in annos {
            out.push(Candidate {
                priority: 0,
                span: find_name_region(&text, a.span, &a.annotation),
                node: Node::AnnotationRef(a),
            });
            out.push(Candidate {
                priority: 1,
                span: trim_span_end(&text, a.span),
                node: Node::AnnotationRef(a),
            });
        }
    };

    for file in &resolution.files {
        if file.name == uri {
            for config in &file.configurations {
                push_type_ref(&config.root, &mut out);
            }
            for element in &file.elements {
                let this = flat;
                match element {
                    SemanticElement::Data(d) => {
                        out.push(Candidate {
                            priority: 0,
                            span: find_name_region(&text, d.span, &d.name),
                            node: Node::Element(this),
                        });
                        if let Some(s) = &d.super_type {
                            push_type_ref(s, &mut out);
                        }
                        push_annotations(&d.annotations, &mut out);
                        for a in &d.attributes {
                            attribute_candidates(a, this, &text, &mut out);
                        }
                    }
                    SemanticElement::Enumeration(e) => {
                        out.push(Candidate {
                            priority: 0,
                            span: find_name_region(&text, e.span, &e.name),
                            node: Node::Element(this),
                        });
                        if let Some(s) = &e.super_type {
                            push_type_ref(s, &mut out);
                        }
                        push_annotations(&e.annotations, &mut out);
                        for v in &e.values {
                            out.push(Candidate {
                                priority: 0,
                                span: find_name_region(&text, v.span, &v.name),
                                node: Node::EnumValue(v, this),
                            });
                            push_annotations(&v.annotations, &mut out);
                        }
                    }
                    SemanticElement::Annotation(a) => {
                        out.push(Candidate {
                            priority: 0,
                            span: find_name_region(&text, a.span, &a.name),
                            node: Node::Element(this),
                        });
                        for attr in &a.attributes {
                            attribute_candidates(attr, this, &text, &mut out);
                        }
                    }
                    SemanticElement::TypeAlias(t) => {
                        out.push(Candidate {
                            priority: 0,
                            span: find_name_region(&text, t.span, &t.name),
                            node: Node::Element(this),
                        });
                        push_annotations(&t.annotations, &mut out);
                        for p in &t.parameters {
                            push_type_ref(&p.type_ref, &mut out);
                        }
                        push_type_ref(&t.type_ref, &mut out);
                    }
                    SemanticElement::BasicType(b) => {
                        out.push(Candidate {
                            priority: 0,
                            span: find_name_region(&text, b.span, &b.name),
                            node: Node::Element(this),
                        });
                        for p in &b.parameters {
                            push_type_ref(&p.type_ref, &mut out);
                        }
                    }
                    SemanticElement::RecordType(r) => {
                        out.push(Candidate {
                            priority: 0,
                            span: find_name_region(&text, r.span, &r.name),
                            node: Node::Element(this),
                        });
                        for f in &r.features {
                            push_type_ref(&f.type_ref, &mut out);
                        }
                    }
                    SemanticElement::LibraryFunction(f) => {
                        out.push(Candidate {
                            priority: 0,
                            span: find_name_region(&text, f.span, &f.name),
                            node: Node::Element(this),
                        });
                        for p in &f.parameters {
                            push_type_ref(&p.type_ref, &mut out);
                        }
                        push_type_ref(&f.return_type, &mut out);
                    }
                    SemanticElement::Function(f) => {
                        out.push(Candidate {
                            priority: 0,
                            span: find_name_region(&text, f.span, &f.name),
                            node: Node::Element(this),
                        });
                        push_annotations(&f.annotations, &mut out);
                        if let Some(s) = &f.super_function {
                            push_type_ref(s, &mut out);
                        }
                        for a in f.inputs.iter().chain(f.output.iter()) {
                            attribute_candidates(a, this, &text, &mut out);
                        }
                        for c in f.conditions.iter().chain(f.post_conditions.iter()) {
                            push_annotations(&c.annotations, &mut out);
                        }
                    }
                    SemanticElement::Rule(r) => {
                        out.push(Candidate {
                            priority: 0,
                            span: find_name_region(&text, r.span, &r.name),
                            node: Node::Element(this),
                        });
                        if let Some(i) = &r.input {
                            push_type_ref(i, &mut out);
                        }
                    }
                    SemanticElement::Report(_) => {}
                    SemanticElement::ExternalRuleSource(s) => {
                        out.push(Candidate {
                            priority: 0,
                            span: find_name_region(&text, s.span, &s.name),
                            node: Node::Element(this),
                        });
                        if let Some(s) = &s.super_source {
                            push_type_ref(s, &mut out);
                        }
                        for c in &s.classes {
                            push_type_ref(&c.data, &mut out);
                        }
                    }
                    SemanticElement::Schema(s) => {
                        out.push(Candidate {
                            priority: 0,
                            span: find_name_region(&text, s.span, &s.name),
                            node: Node::Element(this),
                        });
                        push_annotations(&s.annotations, &mut out);
                    }
                    SemanticElement::Body(b) => {
                        out.push(Candidate {
                            priority: 0,
                            span: find_name_region(&text, b.span, &b.name),
                            node: Node::Element(this),
                        });
                    }
                    SemanticElement::Corpus(c) => {
                        out.push(Candidate {
                            priority: 0,
                            span: find_name_region(&text, c.span, &c.name),
                            node: Node::Element(this),
                        });
                    }
                    SemanticElement::Segment(s) => {
                        out.push(Candidate {
                            priority: 0,
                            span: find_name_region(&text, s.span, &s.name),
                            node: Node::Element(this),
                        });
                    }
                    SemanticElement::MetaType(m) => {
                        out.push(Candidate {
                            priority: 0,
                            span: find_name_region(&text, m.span, &m.name),
                            node: Node::Element(this),
                        });
                        push_type_ref(&m.type_ref, &mut out);
                    }
                }
                flat += 1;
            }
            continue; // found the file; skip the flat-count fix-up below
        }
        flat += file.elements.len();
    }
    out
}

fn attribute_candidates<'a>(
    a: &'a Attribute,
    owner: usize,
    text: &str,
    out: &mut Vec<Candidate<'a>>,
) {
    out.push(Candidate {
        priority: 0,
        span: find_name_region(text, a.span, &a.name),
        node: Node::Attribute(a, owner),
    });
    out.push(Candidate {
        priority: 1,
        span: trim_span_end(text, a.span),
        node: Node::Attribute(a, owner),
    });
    out.push(Candidate {
        priority: 0,
        span: type_ref_name_span(&a.type_ref, text),
        node: Node::TypeRef(&a.type_ref),
    });
    out.push(Candidate {
        priority: 1,
        span: trim_span_end(text, a.type_ref.span),
        node: Node::TypeRef(&a.type_ref),
    });
    for anno in &a.annotations {
        out.push(Candidate {
            priority: 0,
            span: find_name_region(text, anno.span, &anno.annotation),
            node: Node::AnnotationRef(anno),
        });
        out.push(Candidate {
            priority: 1,
            span: trim_span_end(text, anno.span),
            node: Node::AnnotationRef(anno),
        });
    }
}

// The walker above tracks the flat element counter as it goes; resolve()
// lays elements out flat in file order, which the counter mirrors.

/// One interesting node of the resolved model, carrying borrowed references
/// plus the flat element ids needed to map to declarations.
#[derive(Clone, Debug)]
enum Selected {
    Element(usize),
    Attribute { owner: usize, name: String },
    EnumValue { owner: usize, name: String },
    TypeRef { resolved: Option<usize> },
    AnnotationRef { resolved: Option<usize> },
}

fn select_at(world: &World, uri: &str, offset: usize) -> Option<(Span, Selected)> {
    let candidates = candidates(world, uri);
    let best = candidates
        .iter()
        .filter(|c| {
            // Name regions (priority 0) include the cursor sitting just
            // after the identifier; whole-node spans (priority 1) do not,
            // because parser spans tend to absorb trailing whitespace and
            // would otherwise bleed onto the following blank line.
            let at_start_or_within = c.span.start <= offset && offset < c.span.end;
            let at_inclusive_end = c.priority == 0 && offset == c.span.end;
            at_start_or_within || at_inclusive_end
        })
        .min_by_key(|c| (c.priority, c.span.end.saturating_sub(c.span.start)))?;
    let selected = match best.node {
        Node::Element(flat) => Selected::Element(flat),
        Node::Attribute(a, owner) => Selected::Attribute {
            owner,
            name: a.name.clone(),
        },
        Node::EnumValue(v, owner) => Selected::EnumValue {
            owner,
            name: v.name.clone(),
        },
        Node::TypeRef(r) => Selected::TypeRef {
            resolved: r.resolved,
        },
        Node::AnnotationRef(a) => Selected::AnnotationRef {
            resolved: a.annotation_resolved,
        },
    };
    Some((best.span, selected))
}

/// Location of an element's declaration by flat id, mapped into the file's
/// document (workspace document or builtin pseudo-document).
fn element_location(world: &World, flat: usize) -> Option<lsp_types::Location> {
    let resolution = &world.analysis().resolution;
    let span = *resolution.element_spans.get(flat)?;
    let mut count = 0usize;
    for file in &resolution.files {
        if flat < count + file.elements.len() {
            return Some(location_in(world, &file.name, span));
        }
        count += file.elements.len();
    }
    None
}

fn location_in(world: &World, file_name: &str, span: Span) -> lsp_types::Location {
    let enc = world.encoding();
    let analysis = world.analysis();
    let (uri, range) = if let Some(doc) = world.document(file_name) {
        (file_name.to_string(), doc.range(span, enc))
    } else if let Some(doc) = analysis.builtin_docs.get(&format!("builtin:{file_name}")) {
        let uri = format!("{}{}", crate::analysis::BUILTIN_SCHEME, file_name);
        (uri.clone(), doc.range(span, enc))
    } else {
        (file_name.to_string(), lsp_types::Range::default())
    };
    let uri = serde_json::from_value(serde_json::Value::String(uri)).expect("uri round-trips");
    lsp_types::Location { uri, range }
}

// ---- element metadata ------------------------------------------------------

fn element_kind(element: &SemanticElement) -> &'static str {
    match element {
        SemanticElement::Data(d) if d.is_choice => "choice",
        SemanticElement::Data(_) => "type",
        SemanticElement::Enumeration(_) => "enum",
        SemanticElement::Annotation(_) => "annotation",
        SemanticElement::TypeAlias(_) => "typeAlias",
        SemanticElement::BasicType(_) => "basicType",
        SemanticElement::RecordType(_) => "recordType",
        SemanticElement::LibraryFunction(_) => "library function",
        SemanticElement::Function(f) if f.dispatch.is_some() => "dispatch function",
        SemanticElement::Function(_) => "function",
        SemanticElement::Rule(r) if r.eligibility => "eligibility rule",
        SemanticElement::Rule(_) => "reporting rule",
        SemanticElement::Report(_) => "report",
        SemanticElement::ExternalRuleSource(_) => "rule source",
        SemanticElement::Schema(_) => "schema",
        SemanticElement::Body(_) => "body",
        SemanticElement::Corpus(_) => "corpus",
        SemanticElement::Segment(_) => "segment",
        SemanticElement::MetaType(_) => "metaType",
    }
}

fn element_definition(element: &SemanticElement) -> Option<String> {
    match element {
        SemanticElement::Data(d) => d.definition.clone(),
        SemanticElement::Enumeration(e) => e.definition.clone(),
        SemanticElement::Annotation(a) => a.definition.clone(),
        SemanticElement::TypeAlias(t) => t.definition.clone(),
        SemanticElement::BasicType(b) => b.definition.clone(),
        SemanticElement::RecordType(r) => r.definition.clone(),
        SemanticElement::LibraryFunction(f) => f.definition.clone(),
        SemanticElement::Function(f) => f.definition.clone(),
        SemanticElement::Rule(r) => r.definition.clone(),
        SemanticElement::Report(_) => None,
        SemanticElement::ExternalRuleSource(_) => None,
        SemanticElement::Schema(s) => s.definition.clone(),
        SemanticElement::Body(b) => b.definition.clone(),
        SemanticElement::Corpus(c) => c.definition.clone(),
        SemanticElement::Segment(_) => None,
        SemanticElement::MetaType(_) => None,
    }
}

fn element_at(world: &World, flat: usize) -> Option<&SemanticElement> {
    let resolution = &world.analysis().resolution;
    let mut count = 0usize;
    for file in &resolution.files {
        if flat < count + file.elements.len() {
            return file.elements.get(flat - count);
        }
        count += file.elements.len();
    }
    None
}

/// Markdown block describing a resolved target element: `**kind** \`fqn\``
/// followed by its `<"...">` definition when present.
fn element_markdown(world: &World, flat: usize) -> Option<String> {
    let element = element_at(world, flat)?;
    let fqn = &world.analysis().resolution.element_names[flat];
    let mut md = format!("**{}** `{}`", element_kind(element), fqn);
    if let Some(def) = element_definition(element) {
        md.push_str("\n\n---\n\n");
        md.push_str(def.trim());
    }
    Some(md)
}

// ---- documentSymbol --------------------------------------------------------

/// Build the `textDocument/documentSymbol` tree for one document, straight
/// from the syntax AST (which has the full spanned structure).
pub fn document_symbols(world: &World, uri: &str) -> Vec<lsp_types::DocumentSymbol> {
    let Some(doc) = world.document(uri) else {
        return Vec::new();
    };
    let text = doc.text.clone();
    let enc = world.encoding();
    let Some(unit) = parse_unit(uri, &text) else {
        return Vec::new();
    };

    let children: Vec<lsp_types::DocumentSymbol> = unit
        .elements
        .iter()
        .filter_map(|e| element_symbol(e, doc, &text, enc))
        .collect();

    let ns_span = unit.namespace.span;
    vec![lsp_types::DocumentSymbol {
        name: unit.namespace.name.0.clone(),
        detail: Some("namespace".to_string()),
        kind: lsp_types::SymbolKind::NAMESPACE,
        tags: None,
        #[allow(deprecated)]
        deprecated: None,
        range: range_of(doc, ns_span, enc),
        selection_range: range_of(
            doc,
            find_name_region(&text, ns_span, &unit.namespace.name.0),
            enc,
        ),
        children: Some(children),
    }]
}

fn element_symbol(
    element: &Element,
    doc: &Document,
    text: &str,
    enc: PositionEncoding,
) -> Option<lsp_types::DocumentSymbol> {
    let mk = |name: &str,
              detail: &str,
              kind: lsp_types::SymbolKind,
              span: Span,
              children: Vec<lsp_types::DocumentSymbol>| {
        Some(lsp_types::DocumentSymbol {
            name: name.to_string(),
            detail: Some(detail.to_string()),
            kind,
            tags: None,
            #[allow(deprecated)]
            deprecated: None,
            range: range_of(doc, span, enc),
            selection_range: range_of(doc, find_name_region(text, span, name), enc),
            children: if children.is_empty() {
                None
            } else {
                Some(children)
            },
        })
    };
    match element {
        Element::Data(d) => {
            let children: Vec<_> = d
                .attributes
                .iter()
                .map(|a| attribute_symbol(a, doc, text, enc))
                .collect();
            mk(
                &d.name,
                "type",
                lsp_types::SymbolKind::STRUCT,
                d.span,
                children,
            )
        }
        Element::Choice(d) => {
            let children: Vec<_> = d
                .attributes
                .iter()
                .map(|a| attribute_symbol(a, doc, text, enc))
                .collect();
            mk(
                &d.name,
                "choice",
                lsp_types::SymbolKind::STRUCT,
                d.span,
                children,
            )
        }
        Element::Enumeration(e) => {
            let children = e
                .values
                .iter()
                .filter_map(|v| {
                    mk(
                        &v.name,
                        "enum value",
                        lsp_types::SymbolKind::ENUM_MEMBER,
                        v.span,
                        Vec::new(),
                    )
                })
                .collect();
            mk(
                &e.name,
                "enum",
                lsp_types::SymbolKind::ENUM,
                e.span,
                children,
            )
        }
        Element::Annotation(a) => {
            let children: Vec<_> = a
                .attributes
                .iter()
                .map(|a| attribute_symbol(a, doc, text, enc))
                .collect();
            mk(
                &a.name,
                "annotation",
                lsp_types::SymbolKind::INTERFACE,
                a.span,
                children,
            )
        }
        Element::TypeAlias(t) => mk(
            &t.name,
            "typeAlias",
            lsp_types::SymbolKind::TYPE_PARAMETER,
            t.span,
            Vec::new(),
        ),
        Element::BasicType(b) => mk(
            &b.name,
            "basicType",
            lsp_types::SymbolKind::CLASS,
            b.span,
            Vec::new(),
        ),
        Element::RecordType(r) => mk(
            &r.name,
            "recordType",
            lsp_types::SymbolKind::STRUCT,
            r.span,
            Vec::new(),
        ),
        Element::LibraryFunction(f) => mk(
            &f.name,
            "library function",
            lsp_types::SymbolKind::FUNCTION,
            f.span,
            Vec::new(),
        ),
        Element::Function(f) => mk(
            &f.name,
            if f.dispatch.is_some() {
                "dispatch function"
            } else {
                "function"
            },
            lsp_types::SymbolKind::FUNCTION,
            f.span,
            f.inputs
                .iter()
                .chain(f.output.iter())
                .map(|a| attribute_symbol(a, doc, text, enc))
                .collect(),
        ),
        Element::Rule(r) => mk(
            &r.name,
            if r.eligibility {
                "eligibility rule"
            } else {
                "reporting rule"
            },
            lsp_types::SymbolKind::FUNCTION,
            r.span,
            Vec::new(),
        ),
        Element::Report(_) => None,
        Element::RuleSource(s) => mk(
            &s.name,
            "rule source",
            lsp_types::SymbolKind::MODULE,
            s.span,
            Vec::new(),
        ),
        Element::Schema(s) => mk(
            &s.name,
            "schema",
            lsp_types::SymbolKind::OBJECT,
            s.span,
            Vec::new(),
        ),
        Element::Body(b) => mk(
            &b.name,
            "body",
            lsp_types::SymbolKind::PACKAGE,
            b.span,
            Vec::new(),
        ),
        Element::Corpus(c) => mk(
            &c.name,
            "corpus",
            lsp_types::SymbolKind::PACKAGE,
            c.span,
            Vec::new(),
        ),
        Element::Segment(s) => mk(
            &s.name,
            "segment",
            lsp_types::SymbolKind::KEY,
            s.span,
            Vec::new(),
        ),
        Element::MetaType(m) => mk(
            &m.name,
            "metaType",
            lsp_types::SymbolKind::TYPE_PARAMETER,
            m.span,
            Vec::new(),
        ),
        Element::Unsupported(_) => None,
    }
}

fn attribute_symbol(
    attr: &sigil_syntax::ast::AttributeDef,
    doc: &Document,
    text: &str,
    enc: PositionEncoding,
) -> lsp_types::DocumentSymbol {
    lsp_types::DocumentSymbol {
        name: attr.name.clone(),
        detail: Some("attribute".to_string()),
        kind: lsp_types::SymbolKind::FIELD,
        tags: None,
        #[allow(deprecated)]
        deprecated: None,
        range: range_of(doc, attr.span, enc),
        selection_range: range_of(doc, find_name_region(text, attr.span, &attr.name), enc),
        children: None,
    }
}

// ---- definition ------------------------------------------------------------

/// `textDocument/definition`: on a type reference (attribute type, extends,
/// type alias, record feature, configuration root, ...) return the
/// declaring element's range; on an annotation reference the declaring
/// annotation; on a declaration name the declaration itself. Works
/// cross-file and into the built-in library.
pub fn definition(
    world: &World,
    uri: &str,
    position: lsp_types::Position,
) -> Option<lsp_types::Location> {
    let doc = world.document(uri)?;
    let offset = doc.offset(position, world.encoding());
    let (_, selected) = select_at(world, uri, offset)?;
    match selected {
        Selected::TypeRef { resolved, .. } => element_location(world, resolved?),
        Selected::AnnotationRef { resolved, .. } => element_location(world, resolved?),
        Selected::Element(flat) => element_location(world, flat),
        Selected::Attribute { .. } | Selected::EnumValue { .. } => {
            // Definition on a declaration itself: its own name region.
            let range = doc.range(span_of_selected(world, uri, offset), world.encoding());
            Some(lsp_types::Location {
                uri: serde_json::from_value(serde_json::Value::String(uri.to_string()))
                    .expect("uri round-trips"),
                range,
            })
        }
    }
}

/// Re-derive the exact span the cursor is on (name region preferred).
fn span_of_selected(world: &World, uri: &str, offset: usize) -> Span {
    select_at(world, uri, offset)
        .map(|(span, _)| span)
        .unwrap_or_default()
}

// ---- hover -----------------------------------------------------------------

/// `textDocument/hover`: hovered element/attribute/enum value renders its
/// `<"...">` definition (and, for attributes, the resolved type FQN and
/// cardinality); a hovered reference renders the target's FQN and
/// definition. Markdown-formatted.
pub fn hover(world: &World, uri: &str, position: lsp_types::Position) -> Option<lsp_types::Hover> {
    let doc = world.document(uri)?;
    let offset = doc.offset(position, world.encoding());
    let (span, selected) = select_at(world, uri, offset)?;
    let resolution = &world.analysis().resolution;

    let markdown = match selected {
        Selected::TypeRef { resolved, .. } => element_markdown(world, resolved?),
        Selected::AnnotationRef { resolved, .. } => element_markdown(world, resolved?),
        Selected::Element(flat) => element_markdown(world, flat),
        Selected::Attribute { owner, name } => {
            let owner_fqn = &resolution.element_names[owner];
            let owner_simple = owner_fqn.rsplit('.').next().unwrap_or(owner_fqn);
            let element = element_at(world, owner)?;
            let attr = match element {
                SemanticElement::Data(d) => d.attributes.iter().find(|a| a.name == name),
                SemanticElement::Annotation(a) => a.attributes.iter().find(|a| a.name == name),
                _ => None,
            }?;
            let type_fqn = match attr.type_ref.resolved {
                Some(id) => resolution.element_names[id].clone(),
                None => attr.type_ref.name.clone(),
            };
            let mut md = format!(
                "**attribute** `{}.{}`: `{}` {}",
                owner_simple,
                attr.name,
                type_fqn,
                attr.cardinality.to_constraint_string()
            );
            if let Some(def) = &attr.definition {
                md.push_str("\n\n---\n\n");
                md.push_str(def.trim());
            }
            Some(md)
        }
        Selected::EnumValue { owner, name } => {
            let owner_fqn = &resolution.element_names[owner];
            let owner_simple = owner_fqn.rsplit('.').next().unwrap_or(owner_fqn);
            let element = element_at(world, owner)?;
            let value = match element {
                SemanticElement::Enumeration(e) => e.values.iter().find(|v| v.name == name),
                _ => None,
            }?;
            let mut md = format!("**enum value** `{}.{}`", owner_simple, value.name);
            if let Some(display) = &value.display {
                md.push_str(&format!(" — display: \"{display}\""));
            }
            if let Some(def) = &value.definition {
                md.push_str("\n\n---\n\n");
                md.push_str(def.trim());
            }
            Some(md)
        }
    }?;

    Some(lsp_types::Hover {
        contents: lsp_types::HoverContents::Markup(lsp_types::MarkupContent {
            kind: lsp_types::MarkupKind::Markdown,
            value: markdown,
        }),
        range: Some(range_of(doc, span, world.encoding())),
    })
}

// ---- references -------------------------------------------------------------

/// All reference sites in the workspace: (file name, name-region span,
/// resolved flat id). Includes builtin files so references inside
/// `com.rosetta.model` count too.
fn reference_sites(world: &World) -> Vec<(String, Span, usize)> {
    let resolution = &world.analysis().resolution;
    let mut sites = Vec::new();
    let mut flat = 0usize;
    for file in &resolution.files {
        // The text is needed for name-region lookup; use the document when
        // available, else the builtin document.
        let text = world
            .document(&file.name)
            .map(|d| d.text.clone())
            .or_else(|| {
                world
                    .analysis()
                    .builtin_docs
                    .get(&format!("builtin:{}", file.name))
                    .map(|d| d.text.clone())
            });
        if let Some(text) = text {
            for config in &file.configurations {
                if let Some(id) = config.root.resolved {
                    sites.push((
                        file.name.clone(),
                        type_ref_name_span(&config.root, &text),
                        id,
                    ));
                }
            }
            let walk_attributes = |attrs: &[Attribute], sites: &mut Vec<(String, Span, usize)>| {
                for a in attrs {
                    if let Some(id) = a.type_ref.resolved {
                        sites.push((
                            file.name.clone(),
                            type_ref_name_span(&a.type_ref, &text),
                            id,
                        ));
                    }
                    for anno in &a.annotations {
                        if let Some(id) = anno.annotation_resolved {
                            sites.push((
                                file.name.clone(),
                                find_name_region(&text, anno.span, &anno.annotation),
                                id,
                            ));
                        }
                    }
                }
            };
            for element in &file.elements {
                match element {
                    SemanticElement::Data(d) => {
                        if let Some(s) = &d.super_type {
                            if let Some(id) = s.resolved {
                                sites.push((file.name.clone(), type_ref_name_span(s, &text), id));
                            }
                        }
                        walk_attributes(&d.attributes, &mut sites);
                        for anno in &d.annotations {
                            if let Some(id) = anno.annotation_resolved {
                                sites.push((
                                    file.name.clone(),
                                    find_name_region(&text, anno.span, &anno.annotation),
                                    id,
                                ));
                            }
                        }
                    }
                    SemanticElement::Enumeration(e) => {
                        if let Some(s) = &e.super_type {
                            if let Some(id) = s.resolved {
                                sites.push((file.name.clone(), type_ref_name_span(s, &text), id));
                            }
                        }
                        for anno in &e.annotations {
                            if let Some(id) = anno.annotation_resolved {
                                sites.push((
                                    file.name.clone(),
                                    find_name_region(&text, anno.span, &anno.annotation),
                                    id,
                                ));
                            }
                        }
                        for v in &e.values {
                            for anno in &v.annotations {
                                if let Some(id) = anno.annotation_resolved {
                                    sites.push((
                                        file.name.clone(),
                                        find_name_region(&text, anno.span, &anno.annotation),
                                        id,
                                    ));
                                }
                            }
                        }
                    }
                    SemanticElement::Annotation(a) => {
                        walk_attributes(&a.attributes, &mut sites);
                    }
                    SemanticElement::TypeAlias(t) => {
                        if let Some(id) = t.type_ref.resolved {
                            sites.push((
                                file.name.clone(),
                                type_ref_name_span(&t.type_ref, &text),
                                id,
                            ));
                        }
                        for anno in &t.annotations {
                            if let Some(id) = anno.annotation_resolved {
                                sites.push((
                                    file.name.clone(),
                                    find_name_region(&text, anno.span, &anno.annotation),
                                    id,
                                ));
                            }
                        }
                        for p in &t.parameters {
                            if let Some(id) = p.type_ref.resolved {
                                sites.push((
                                    file.name.clone(),
                                    type_ref_name_span(&p.type_ref, &text),
                                    id,
                                ));
                            }
                        }
                    }
                    SemanticElement::BasicType(b) => {
                        for p in &b.parameters {
                            if let Some(id) = p.type_ref.resolved {
                                sites.push((
                                    file.name.clone(),
                                    type_ref_name_span(&p.type_ref, &text),
                                    id,
                                ));
                            }
                        }
                    }
                    SemanticElement::RecordType(r) => {
                        for f in &r.features {
                            if let Some(id) = f.type_ref.resolved {
                                sites.push((
                                    file.name.clone(),
                                    type_ref_name_span(&f.type_ref, &text),
                                    id,
                                ));
                            }
                        }
                    }
                    SemanticElement::LibraryFunction(f) => {
                        for p in &f.parameters {
                            if let Some(id) = p.type_ref.resolved {
                                sites.push((
                                    file.name.clone(),
                                    type_ref_name_span(&p.type_ref, &text),
                                    id,
                                ));
                            }
                        }
                        if let Some(id) = f.return_type.resolved {
                            sites.push((
                                file.name.clone(),
                                type_ref_name_span(&f.return_type, &text),
                                id,
                            ));
                        }
                    }
                    SemanticElement::Function(f) => {
                        if let Some(s) = &f.super_function {
                            if let Some(id) = s.resolved {
                                sites.push((file.name.clone(), type_ref_name_span(s, &text), id));
                            }
                        }
                        walk_attributes(&f.inputs, &mut sites);
                        if let Some(o) = &f.output {
                            walk_attributes(std::slice::from_ref(o), &mut sites);
                        }
                        for anno in &f.annotations {
                            if let Some(id) = anno.annotation_resolved {
                                sites.push((
                                    file.name.clone(),
                                    find_name_region(&text, anno.span, &anno.annotation),
                                    id,
                                ));
                            }
                        }
                    }
                    SemanticElement::Rule(r) => {
                        if let Some(i) = &r.input {
                            if let Some(id) = i.resolved {
                                sites.push((file.name.clone(), type_ref_name_span(i, &text), id));
                            }
                        }
                    }
                    SemanticElement::Report(rep) => {
                        let reg = &rep.regulatory;
                        if let Some(id) = reg.body.resolved {
                            sites.push((
                                file.name.clone(),
                                find_name_region(&text, reg.body.span, &reg.body.name),
                                id,
                            ));
                        }
                        for c in reg.corpora.iter().chain(rep.eligibility_rules.iter()) {
                            if let Some(id) = c.resolved {
                                sites.push((
                                    file.name.clone(),
                                    find_name_region(&text, c.span, &c.name),
                                    id,
                                ));
                            }
                        }
                        for s in &reg.segments {
                            if let Some(id) = s.segment.resolved {
                                sites.push((
                                    file.name.clone(),
                                    find_name_region(&text, s.segment.span, &s.segment.name),
                                    id,
                                ));
                            }
                        }
                        if let Some(id) = rep.input_type.resolved {
                            sites.push((
                                file.name.clone(),
                                type_ref_name_span(&rep.input_type, &text),
                                id,
                            ));
                        }
                        if let Some(id) = rep.report_type.resolved {
                            sites.push((
                                file.name.clone(),
                                find_name_region(
                                    &text,
                                    rep.report_type.span,
                                    &rep.report_type.name,
                                ),
                                id,
                            ));
                        }
                        if let Some(s) = &rep.rule_source {
                            if let Some(id) = s.resolved {
                                sites.push((
                                    file.name.clone(),
                                    find_name_region(&text, s.span, &s.name),
                                    id,
                                ));
                            }
                        }
                    }
                    SemanticElement::ExternalRuleSource(s) => {
                        if let Some(sup) = &s.super_source {
                            if let Some(id) = sup.resolved {
                                sites.push((file.name.clone(), type_ref_name_span(sup, &text), id));
                            }
                        }
                        for c in &s.classes {
                            if let Some(id) = c.data.resolved {
                                sites.push((
                                    file.name.clone(),
                                    type_ref_name_span(&c.data, &text),
                                    id,
                                ));
                            }
                            for a in &c.attributes {
                                for r in &a.rule_references {
                                    if let Some(id) = r.resolved {
                                        sites.push((
                                            file.name.clone(),
                                            find_name_region(&text, a.span, a.attribute.as_str()),
                                            id,
                                        ));
                                    }
                                }
                            }
                        }
                    }
                    SemanticElement::Schema(s) => {
                        for anno in &s.annotations {
                            if let Some(id) = anno.annotation_resolved {
                                sites.push((
                                    file.name.clone(),
                                    find_name_region(&text, anno.span, &anno.annotation),
                                    id,
                                ));
                            }
                        }
                    }
                    SemanticElement::Body(_)
                    | SemanticElement::Corpus(_)
                    | SemanticElement::Segment(_) => {}
                    SemanticElement::MetaType(m) => {
                        if let Some(id) = m.type_ref.resolved {
                            sites.push((
                                file.name.clone(),
                                type_ref_name_span(&m.type_ref, &text),
                                id,
                            ));
                        }
                    }
                }
                flat += 1;
            }
        } else {
            flat += file.elements.len();
        }
    }
    let _ = flat;
    sites
}

/// `textDocument/references`: all sites resolving to the element under the
/// cursor (works when the cursor is on the declaration or on any
/// referencing site).
pub fn references(
    world: &World,
    uri: &str,
    position: lsp_types::Position,
    include_declaration: bool,
) -> Vec<lsp_types::Location> {
    let Some(doc) = world.document(uri) else {
        return Vec::new();
    };
    let offset = doc.offset(position, world.encoding());
    let target = match select_at(world, uri, offset) {
        Some((_, Selected::Element(flat))) => Some(flat),
        Some((_, Selected::TypeRef { resolved, .. })) => resolved,
        Some((_, Selected::AnnotationRef { resolved, .. })) => resolved,
        _ => None,
    };
    let target = match target {
        Some(t) => t,
        None => return Vec::new(),
    };

    let mut out = Vec::new();
    if include_declaration {
        if let Some(loc) = element_location(world, target) {
            out.push(loc);
        }
    }
    for (file, span, id) in reference_sites(world) {
        if id == target {
            out.push(location_in(world, &file, span));
        }
    }
    out.sort_by(|a, b| {
        (
            a.uri.to_string(),
            a.range.start.line,
            a.range.start.character,
        )
            .cmp(&(
                b.uri.to_string(),
                b.range.start.line,
                b.range.start.character,
            ))
    });
    out.dedup_by(|a, b| a.uri == b.uri && a.range == b.range);
    out
}

// ---- rename -----------------------------------------------------------------

/// Rune grammar keyword tokens: Xtext reserves every keyword token, so a
/// plain occurrence would parse as the keyword, not as a name. The words
/// the grammar's `ValidID` rule explicitly re-allows (`condition`,
/// `source`, `version`, `scope`, `ingest`, `enrich`, `projection`) are
/// *not* listed here — they are legal names.
const RUNE_KEYWORDS: &[&str] = &[
    "absent",
    "add",
    "alias",
    "all",
    "and",
    "annotation",
    "any",
    "as",
    "basicType",
    "body",
    "choice",
    "contains",
    "corpus",
    "count",
    "default",
    "disjoint",
    "displayName",
    "docReference",
    "eligibility",
    "else",
    "empty",
    "enum",
    "exists",
    "extends",
    "extract",
    "False",
    "filter",
    "first",
    "flatten",
    "for",
    "from",
    "func",
    "function",
    "if",
    "import",
    "in",
    "inputs",
    "is",
    "item",
    "join",
    "label",
    "last",
    "library",
    "max",
    "metaType",
    "min",
    "multiple",
    "namespace",
    "only",
    "or",
    "output",
    "override",
    "prefix",
    "provision",
    "rationale",
    "reduce",
    "report",
    "reportedField",
    "reporting",
    "reverse",
    "root",
    "rule",
    "ruleReference",
    "schema",
    "segment",
    "set",
    "single",
    "sort",
    "sum",
    "super",
    "switch",
    "then",
    "True",
    "type",
    "typeAlias",
    "when",
    "with",
];

/// Rust keywords (strict, 2018 and 2024 reserved): renamed model elements
/// surface as identifiers in generated bindings, so names like `fn` or
/// `trait` would break downstream code generation.
const RUST_KEYWORDS: &[&str] = &[
    "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "crate",
    "do", "dyn", "else", "enum", "extern", "false", "final", "fn", "for", "gen", "if", "impl",
    "in", "let", "loop", "macro", "match", "mod", "move", "mut", "override", "priv", "pub", "ref",
    "return", "self", "Self", "static", "struct", "super", "trait", "true", "try", "type",
    "typeof", "union", "unsafe", "unsized", "use", "virtual", "where", "while", "yield",
];

/// Reject invalid rename targets: empty names, anything outside the
/// Xtext `ID` shape `[A-Za-z_][A-Za-z0-9_]*` (the `^` escape is source
/// spelling, not part of a name — so a proposed `^enum` is illegal), and
/// Rune/Rust keywords (no auto-escaping: a keyword rename would collide
/// with the grammar and must be spelled by the user if ever needed).
fn validate_new_name(new_name: &str) -> Result<(), String> {
    if new_name.is_empty() {
        return Err("the new name is empty".to_string());
    }
    let valid_shape = {
        let mut chars = new_name.chars();
        chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    };
    if !valid_shape {
        return Err(format!(
            "'{new_name}' is not a valid Rune identifier (expected [A-Za-z_][A-Za-z0-9_]*)"
        ));
    }
    if RUNE_KEYWORDS.contains(&new_name) || RUST_KEYWORDS.contains(&new_name) {
        return Err(format!("'{new_name}' is a reserved keyword"));
    }
    Ok(())
}

/// The exact region a rename replaces: the last segment of a (possibly
/// qualified) name region. A `^` escape is absorbed into the replaced
/// range — whether it precedes the segment (`^enum`) or follows the
/// qualifier dot (`a.b.^enum`) — so a renamed keyword name comes out
/// clean and unescaped.
fn rename_region(text: &str, span: Span) -> Span {
    let end = span.end.min(text.len());
    let hay_start = span.start.min(end);
    let mut start = hay_start + text[hay_start..end].rfind('.').map_or(0, |i| i + 1);
    if start > 0 && text.as_bytes()[start - 1] == b'^' {
        start -= 1;
    }
    Span::new(start, end)
}

/// The simple (last segment) name of an element by flat id.
fn simple_name(world: &World, flat: usize) -> &str {
    let fqn = &world.analysis().resolution.element_names[flat];
    fqn.rsplit('.').next().unwrap_or(fqn)
}

/// The resolution file name and element span of a declaration by flat id
/// (the raw name, unlike [`location_in`]: it may be a `builtin:` file).
fn element_file_and_span(world: &World, flat: usize) -> Option<(String, Span)> {
    let resolution = &world.analysis().resolution;
    let span = *resolution.element_spans.get(flat)?;
    let mut count = 0usize;
    for file in &resolution.files {
        if flat < count + file.elements.len() {
            return Some((file.name.clone(), span));
        }
        count += file.elements.len();
    }
    None
}

/// `textDocument/rename`: rename the element under the cursor across the
/// whole workspace. The site index is the same one `references` uses
/// (every type/annotation reference resolving to the target), plus the
/// declaration's name region. Edits never touch `builtin:` files (the
/// built-in library is read-only), and a rename whose *declaration* lives
/// there is rejected outright.
pub fn rename(
    world: &World,
    uri: &str,
    position: lsp_types::Position,
    new_name: &str,
) -> Result<lsp_types::WorkspaceEdit, String> {
    validate_new_name(new_name)?;
    let doc = world
        .document(uri)
        .ok_or_else(|| format!("unknown document: {uri}"))?;
    let offset = doc.offset(position, world.encoding());
    let target = match select_at(world, uri, offset) {
        Some((_, Selected::Element(flat))) => Some(flat),
        Some((_, Selected::TypeRef { resolved })) => resolved,
        Some((_, Selected::AnnotationRef { resolved })) => resolved,
        Some((_, Selected::Attribute { .. })) => {
            return Err(
                "attributes cannot be renamed: their references are not tracked yet".to_string(),
            );
        }
        Some((_, Selected::EnumValue { .. })) => {
            return Err(
                "enum values cannot be renamed: their references are not tracked yet".to_string(),
            );
        }
        None => return Err("no renameable symbol at this position".to_string()),
    };
    let target = target.ok_or("unresolved references cannot be renamed")?;

    // The declaration must live in a workspace document, never `builtin:`.
    let (decl_file, decl_span) =
        element_file_and_span(world, target).ok_or("the target has no declaration span")?;
    if world.document(&decl_file).is_none() {
        return Err(format!(
            "'{}' is declared in the read-only built-in library ({decl_file})",
            simple_name(world, target)
        ));
    }
    let decl_text = world
        .document(&decl_file)
        .expect("checked above")
        .text
        .clone();
    let decl_region = rename_region(
        &decl_text,
        find_name_region(&decl_text, decl_span, simple_name(world, target)),
    );

    let mut sites = vec![(decl_file, decl_region)];
    for (file, span, id) in reference_sites(world) {
        if id != target || world.document(&file).is_none() {
            continue; // another target, or a read-only builtin site
        }
        sites.push((file, span));
    }

    // `WorkspaceEdit.changes` is keyed by `Uri` per the LSP schema.
    #[allow(clippy::mutable_key_type)]
    let mut changes: HashMap<lsp_types::Uri, Vec<lsp_types::TextEdit>> = HashMap::new();
    for (file, span) in sites {
        let doc = world.document(&file).expect("workspace document");
        let range = doc.range(rename_region(&doc.text, span), world.encoding());
        let uri: lsp_types::Uri = serde_json::from_value(serde_json::Value::String(file.clone()))
            .expect("uri round-trips");
        changes.entry(uri).or_default().push(lsp_types::TextEdit {
            range,
            new_text: new_name.to_string(),
        });
    }
    for edits in changes.values_mut() {
        edits.sort_by_key(|e| (e.range.start.line, e.range.start.character));
        edits.dedup_by(|a, b| a.range == b.range);
    }
    Ok(lsp_types::WorkspaceEdit {
        changes: Some(changes),
        document_changes: None,
        change_annotations: None,
    })
}

// ---- workspace/symbol -------------------------------------------------------

/// `workspace/symbol`: case-insensitive substring search over the
/// fully-qualified names of every resolved element.
pub fn workspace_symbols(world: &World, query: &str) -> Vec<lsp_types::SymbolInformation> {
    let query = query.to_lowercase();
    let resolution = &world.analysis().resolution;
    let mut out = Vec::new();
    let mut flat = 0usize;
    for file in &resolution.files {
        for element in &file.elements {
            let fqn = &resolution.element_names[flat];
            let name = fqn.rsplit('.').next().unwrap_or(fqn);
            if query.is_empty() || fqn.to_lowercase().contains(&query) {
                let kind = match element {
                    SemanticElement::Data(d) if d.is_choice => lsp_types::SymbolKind::STRUCT,
                    SemanticElement::Data(_) => lsp_types::SymbolKind::STRUCT,
                    SemanticElement::Enumeration(_) => lsp_types::SymbolKind::ENUM,
                    SemanticElement::Annotation(_) => lsp_types::SymbolKind::INTERFACE,
                    SemanticElement::TypeAlias(_) => lsp_types::SymbolKind::TYPE_PARAMETER,
                    SemanticElement::BasicType(_) => lsp_types::SymbolKind::CLASS,
                    SemanticElement::RecordType(_) => lsp_types::SymbolKind::STRUCT,
                    SemanticElement::LibraryFunction(_) => lsp_types::SymbolKind::FUNCTION,
                    SemanticElement::Function(_) | SemanticElement::Rule(_) => {
                        lsp_types::SymbolKind::FUNCTION
                    }
                    SemanticElement::Report(_) => lsp_types::SymbolKind::OBJECT,
                    SemanticElement::ExternalRuleSource(_) => lsp_types::SymbolKind::MODULE,
                    SemanticElement::Schema(_) => lsp_types::SymbolKind::OBJECT,
                    SemanticElement::Body(_) | SemanticElement::Corpus(_) => {
                        lsp_types::SymbolKind::PACKAGE
                    }
                    SemanticElement::Segment(_) => lsp_types::SymbolKind::KEY,
                    SemanticElement::MetaType(_) => lsp_types::SymbolKind::TYPE_PARAMETER,
                };
                out.push(lsp_types::SymbolInformation {
                    name: name.to_string(),
                    kind,
                    tags: None,
                    #[allow(deprecated)]
                    deprecated: None,
                    location: location_in(world, &file.name, element.span()),
                    container_name: Some(file.namespace.clone()),
                });
            }
            flat += 1;
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

// ---- completion --------------------------------------------------------------

const TOP_LEVEL_KEYWORDS: &[&str] = &[
    "annotation",
    "basicType",
    "choice",
    "enum",
    "import",
    "isEvent root",
    "isProduct root",
    "library function",
    "namespace",
    "override namespace",
    "recordType",
    "type",
    "typeAlias",
];

fn keyword_item(kw: &str) -> lsp_types::CompletionItem {
    lsp_types::CompletionItem {
        label: kw.to_string(),
        kind: Some(lsp_types::CompletionItemKind::KEYWORD),
        insert_text: Some(kw.to_string()),
        ..lsp_types::CompletionItem::default()
    }
}

/// What the cursor is completing, derived from the current line prefix and
/// the enclosing element.
enum CompletionContext {
    /// Inside `[` before/while typing the annotation name.
    AnnotationName { partial: String },
    /// Inside `[annotation ` — completing one of its attributes.
    AnnotationAttribute {
        annotation_fqn: String,
        partial: String,
    },
    /// After an attribute name — completing a type reference.
    TypeRef { partial: String },
    /// Inside an enum body, after a value name — only `displayName` fits.
    EnumValueSuffix,
    /// Top level — element keywords.
    TopLevel { partial: String },
    /// Nothing useful to offer.
    None,
}

fn completion_context(world: &World, uri: &str, offset: usize) -> CompletionContext {
    let Some(doc) = world.document(uri) else {
        return CompletionContext::None;
    };
    let text = &doc.text;
    let line_start = text[..offset.min(text.len())]
        .rfind('\n')
        .map(|i| i + 1)
        .unwrap_or(0);
    let prefix = &text[line_start..offset.min(text.len())];
    let trimmed = prefix.trim_start();

    // 1. Inside an annotation bracket `[ ... ]` (no closing bracket yet).
    if let Some(bracket) = trimmed.find('[') {
        let after = &trimmed[bracket + 1..];
        if !after.contains(']') && !after.contains('"') {
            if after.trim().is_empty() {
                return CompletionContext::AnnotationName {
                    partial: String::new(),
                };
            }
            match after.split_once(char::is_whitespace) {
                // `[metadata sc|` — completing one of metadata's attributes.
                Some((first, rest)) => {
                    let visible = visible_names(world, uri, true);
                    if let Some(v) = visible.iter().find(|v| v.name == first || v.fqn == first) {
                        return CompletionContext::AnnotationAttribute {
                            annotation_fqn: v.fqn.clone(),
                            partial: rest.trim_start().to_string(),
                        };
                    }
                    return CompletionContext::None;
                }
                // `[met|` — still typing the annotation name.
                None => {
                    return CompletionContext::AnnotationName {
                        partial: after.to_string(),
                    };
                }
            }
        }
    }

    // Enclosing element decides between enum-body / type-body / top level.
    // This is deliberately *parse-free*: mid-typing the document usually
    // does not fully parse, so the syntax AST cannot be relied on here.
    match enclosing_by_text(text, offset) {
        Enclosing::EnumBody => {
            // After a complete value name (possibly trailing space) the
            // only useful next token is `displayName`.
            if trimmed.split_whitespace().next().is_some()
                && (trimmed.ends_with(char::is_whitespace) || last_two_words(trimmed).is_some())
            {
                return CompletionContext::EnumValueSuffix;
            }
            return CompletionContext::None;
        }
        Enclosing::TypeBody => {
            if let Some((first, partial)) = attribute_type_prefix(trimmed) {
                return CompletionContext::TypeRef { partial }.guard(first);
            }
            return CompletionContext::None;
        }
        Enclosing::TopLevel => {}
    }

    // Top level: keywords filtered by the word being typed.
    let partial = trailing_word(trimmed).unwrap_or_default();
    CompletionContext::TopLevel { partial }
}

/// Where the cursor sits, derived from the last zero-indent element
/// keyword line above the cursor (`type`/`choice`/`annotation` open a
/// body, `enum` opens an enum body, everything else is top level).
#[derive(Debug)]
enum Enclosing {
    TypeBody,
    EnumBody,
    TopLevel,
}

fn enclosing_by_text(text: &str, offset: usize) -> Enclosing {
    const BODY_KEYWORDS: &[&str] = &["type", "choice", "annotation"];
    const TOP_KEYWORDS: &[&str] = &[
        "type",
        "choice",
        "enum",
        "annotation",
        "typeAlias",
        "basicType",
        "recordType",
        "library",
        "func",
        "import",
        "namespace",
        "isEvent",
        "isProduct",
        "override",
        "schema",
        "body",
        "corpus",
        "segment",
        "metaType",
        "report",
        "rule",
        "reporting",
    ];
    let mut enclosing = Enclosing::TopLevel;
    let mut consumed = 0usize;
    for line in text.split_inclusive('\n') {
        if consumed >= offset {
            break;
        }
        if !line.starts_with([' ', '\t']) {
            let first = line.split_whitespace().next().unwrap_or("");
            if TOP_KEYWORDS.contains(&first) {
                if BODY_KEYWORDS.contains(&first) {
                    enclosing = Enclosing::TypeBody;
                } else if first == "enum" {
                    enclosing = Enclosing::EnumBody;
                } else {
                    enclosing = Enclosing::TopLevel;
                }
            }
        }
        consumed += line.len();
    }
    enclosing
}

impl CompletionContext {
    /// Validate the first word of an attribute line (reject element
    /// keywords, which start new elements rather than reference types).
    fn guard(self, first: &str) -> CompletionContext {
        const ELEMENT_KEYWORDS: &[&str] = &[
            "type",
            "choice",
            "enum",
            "annotation",
            "typeAlias",
            "basicType",
            "recordType",
            "library",
            "func",
            "condition",
            "import",
            "namespace",
        ];
        if ELEMENT_KEYWORDS.contains(&first) {
            CompletionContext::None
        } else {
            self
        }
    }
}

/// `(first word, trailing partial)` for attribute lines like
/// `party Par|` or `override party |`.
fn attribute_type_prefix(trimmed: &str) -> Option<(&str, String)> {
    let trimmed = trimmed.strip_prefix("override ").unwrap_or(trimmed);
    let (first, rest) = trimmed.split_once(char::is_whitespace)?;
    let first = first.trim_end();
    if first.is_empty()
        || !first
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
    {
        return None;
    }
    let partial = trailing_word(rest).unwrap_or_default();
    // The partial must be the *rest of the line* (no cardinality yet).
    if rest.trim_end() != partial {
        return None;
    }
    Some((first, partial))
}

fn last_two_words(trimmed: &str) -> Option<(&str, &str)> {
    let mut words = trimmed.split_whitespace();
    let first = words.next()?;
    let second = words.next()?;
    Some((first, second))
}

fn trailing_word(s: &str) -> Option<String> {
    let partial: String = s
        .chars()
        .rev()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.')
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    Some(partial)
}

/// `textDocument/completion`: context-aware suggestions from the resolver's
/// scope chain — type names in type-reference positions, annotation
/// attributes inside `[annotation ...`, element keywords at top level.
/// Visible names for completion. When the cursor's file failed to parse
/// (mid-typing, the common completion moment) it is absent from the
/// resolution, so fall back to a lenient scope: built-in library plus
/// elements whose namespace matches the namespace declared in the text.
fn visible_names(world: &World, uri: &str, annotation: bool) -> Vec<sigil_resolve::VisibleName> {
    let resolution = &world.analysis().resolution;
    let strict = if annotation {
        resolution.visible_annotations(uri)
    } else {
        resolution.visible_types(uri)
    };
    if resolution.files.iter().any(|f| f.name == uri) {
        return strict;
    }
    let ns = world.document(uri).and_then(|d| namespace_of_text(&d.text));
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut flat = 0usize;
    for file in &resolution.files {
        for element in &file.elements {
            let wanted = if annotation {
                element.is_annotation()
            } else {
                element.is_type()
            };
            if wanted
                && (file.namespace == sigil_resolve::LIB_NAMESPACE
                    || ns.as_deref() == Some(file.namespace.as_str()))
            {
                let simple = element.name().to_string();
                if seen.insert(simple.clone()) {
                    out.push(sigil_resolve::VisibleName {
                        name: simple,
                        fqn: resolution.element_names[flat].clone(),
                        element: sigil_resolve::ElementId(flat),
                    });
                }
            }
            flat += 1;
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The namespace declared in a (possibly broken) document, for the
/// lenient completion fallback.
fn namespace_of_text(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("namespace") {
            let name: String = rest
                .trim()
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.')
                .collect();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

pub fn completion(
    world: &World,
    uri: &str,
    position: lsp_types::Position,
) -> Option<lsp_types::CompletionList> {
    let doc = world.document(uri)?;
    let offset = doc.offset(position, world.encoding());
    let resolution = &world.analysis().resolution;

    let mut items: Vec<lsp_types::CompletionItem> = Vec::new();
    match completion_context(world, uri, offset) {
        CompletionContext::AnnotationName { partial } => {
            for v in visible_names(world, uri, true) {
                if starts_with(&v.name, &partial) {
                    items.push(lsp_types::CompletionItem {
                        label: v.name,
                        kind: Some(lsp_types::CompletionItemKind::INTERFACE),
                        detail: Some(v.fqn),
                        ..lsp_types::CompletionItem::default()
                    });
                }
            }
        }
        CompletionContext::AnnotationAttribute {
            annotation_fqn,
            partial,
        } => {
            for (attr, definition) in resolution.annotation_attributes(&annotation_fqn) {
                if starts_with(&attr, &partial) {
                    items.push(lsp_types::CompletionItem {
                        label: attr,
                        kind: Some(lsp_types::CompletionItemKind::FIELD),
                        detail: Some(annotation_fqn.to_string()),
                        documentation: definition.map(lsp_types::Documentation::String),
                        ..lsp_types::CompletionItem::default()
                    });
                }
            }
        }
        CompletionContext::TypeRef { partial } => {
            for v in visible_names(world, uri, false) {
                if starts_with(&v.name, &partial) {
                    items.push(lsp_types::CompletionItem {
                        label: v.name,
                        kind: Some(lsp_types::CompletionItemKind::CLASS),
                        detail: Some(v.fqn),
                        ..lsp_types::CompletionItem::default()
                    });
                }
            }
        }
        CompletionContext::EnumValueSuffix => {
            items.push(keyword_item("displayName"));
        }
        CompletionContext::TopLevel { partial } => {
            for kw in TOP_LEVEL_KEYWORDS {
                if starts_with(kw, &partial) {
                    items.push(keyword_item(kw));
                }
            }
        }
        CompletionContext::None => return Some(empty_list()),
    }

    items.sort_by(|a, b| a.label.cmp(&b.label));
    items.dedup_by(|a, b| a.label == b.label);
    Some(lsp_types::CompletionList {
        is_incomplete: false,
        items,
    })
}

fn empty_list() -> lsp_types::CompletionList {
    lsp_types::CompletionList {
        is_incomplete: false,
        items: Vec::new(),
    }
}

fn starts_with(candidate: &str, partial: &str) -> bool {
    candidate.starts_with(partial)
}
