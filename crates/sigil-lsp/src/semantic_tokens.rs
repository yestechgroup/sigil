//! `textDocument/semanticTokens` (issue #2, task 4): exact highlighting
//! for types vs enums vs annotations vs keywords, where the TextMate
//! grammar in `editors/vscode/` only approximates.
//!
//! Classification is grounded in what the toolchain knows:
//!
//! - *declaration names and references* come from the resolved model
//!   ([`sigil_resolve`]), which already carries the byte spans and the
//!   resolved flat ids of every type/annotation reference;
//! - *namespace and import names* come from the syntax AST (the lowered
//!   model has no spans for them);
//! - *keywords, comments, strings and numbers* come from a lightweight
//!   byte-level scan of the document text, mirroring the grammar's
//!   terminals (`//` and `/* */` comments, quoted strings, integer
//!   literals). Identifiers found by the scan are classified by the
//!   highlight spans when one contains them, falling back to the Rune
//!   keyword list — so a legal keyword-spelled name (`condition`) that
//!   resolves to a declaration is highlighted as a declaration, never as
//!   a keyword.
//!
//! Token types (LSP `SemanticTokenType`):
//!
//! | Rune construct                          | token type    |
//! | --------------------------------------- | ------------- |
//! | namespace / imported namespace          | `namespace`   |
//! | type, choice, typeAlias, basicType, recordType, schema, metaType, segment, rule source, unresolved type ref | `type` |
//! | enum declaration; a reference resolving to an enum | `enum` |
//! | enum value                              | `enumMember`  |
//! | annotation declaration and `[...]` references | `decorator` |
//! | func, rule, library function            | `function`    |
//! | attribute (declared or referenced in `[anno attr]`) | `property` |
//! | regulatory body/corpus (declaration and reference) | `namespace` |
//! | grammar keywords                        | `keyword`     |
//! | comments                                | `comment`     |
//! | quoted strings                          | `string`      |
//! | integer literals (cardinalities)        | `number`      |
//!
//! Annotations map to **`decorator`** (not `macro`): they decorate
//! declarations and attributes with metadata, which is exactly the
//! semantics LSP's `decorator` token type describes. Declaration names
//! carry the `declaration` modifier; references carry none.
//!
//! Only the *full* request is implemented (the capability advertises
//! `full: true`, `range: false`). The legend is server-defined and the
//! tokens are emitted regardless of the client's
//! `semanticTokens.tokenTypes` capabilities — the common server
//! behaviour, since the legend indices are what the client must interpret.

use lsp_types::{
    SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokens,
    SemanticTokensFullOptions, SemanticTokensLegend, SemanticTokensOptions,
    SemanticTokensServerCapabilities, WorkDoneProgressOptions,
};
use sigil_diag::Span;
use sigil_model::{AnnotationRef, Attribute, ModelFile, SemanticElement, TypeRef};
use sigil_resolve::{ElementId, ElementKind, Resolution};

use crate::features;
use crate::position::{LineIndex, PositionEncoding};
use crate::world::World;

// ---- legend -----------------------------------------------------------------

/// Token classes in legend-index order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Namespace,
    Type,
    Enum,
    EnumMember,
    Decorator,
    Function,
    Property,
    Keyword,
    Comment,
    String,
    Number,
}

impl Class {
    fn index(self) -> u32 {
        match self {
            Class::Namespace => 0,
            Class::Type => 1,
            Class::Enum => 2,
            Class::EnumMember => 3,
            Class::Decorator => 4,
            Class::Function => 5,
            Class::Property => 6,
            Class::Keyword => 7,
            Class::Comment => 8,
            Class::String => 9,
            Class::Number => 10,
        }
    }

    fn token_type(self) -> SemanticTokenType {
        match self {
            Class::Namespace => SemanticTokenType::NAMESPACE,
            Class::Type => SemanticTokenType::TYPE,
            Class::Enum => SemanticTokenType::ENUM,
            Class::EnumMember => SemanticTokenType::ENUM_MEMBER,
            Class::Decorator => SemanticTokenType::DECORATOR,
            Class::Function => SemanticTokenType::FUNCTION,
            Class::Property => SemanticTokenType::PROPERTY,
            Class::Keyword => SemanticTokenType::KEYWORD,
            Class::Comment => SemanticTokenType::COMMENT,
            Class::String => SemanticTokenType::STRING,
            Class::Number => SemanticTokenType::NUMBER,
        }
    }
}

/// The one modifier this server uses; bit 0 of the modifiers bitset.
const DECLARATION_BIT: u32 = 1;

/// The server's semantic tokens legend and capability entry
/// (`full: true`, no range/delta support).
pub fn server_capability() -> SemanticTokensServerCapabilities {
    SemanticTokensServerCapabilities::SemanticTokensOptions(SemanticTokensOptions {
        work_done_progress_options: WorkDoneProgressOptions::default(),
        legend: SemanticTokensLegend {
            token_types: [
                Class::Namespace,
                Class::Type,
                Class::Enum,
                Class::EnumMember,
                Class::Decorator,
                Class::Function,
                Class::Property,
                Class::Keyword,
                Class::Comment,
                Class::String,
                Class::Number,
            ]
            .iter()
            .map(|c| c.token_type())
            .collect(),
            token_modifiers: vec![SemanticTokenModifier::DECLARATION],
        },
        range: None,
        full: Some(SemanticTokensFullOptions::Bool(true)),
    })
}

// ---- highlight collection -----------------------------------------------------

/// One classified region of the document (a name region or a scan token).
struct Highlight {
    span: Span,
    class: Class,
    declaration: bool,
}

/// `textDocument/semanticTokens/full` for one document: the exact token
/// array, or `None` for an unknown document.
pub fn semantic_tokens(world: &World, uri: &str) -> Option<SemanticTokens> {
    let doc = world.document(uri)?;
    let text = doc.text.clone();

    let mut highlights = Vec::new();
    collect_syntax(uri, &text, &mut highlights);
    collect_resolution(world, uri, &text, &mut highlights);
    highlights.sort_by_key(|h| h.span.start);

    let tokens = scan(&text, &highlights);
    Some(encode(&doc.line_index, &text, tokens, world.encoding()))
}

/// Namespace and import names: only the syntax AST has spans for them.
fn collect_syntax(uri: &str, text: &str, out: &mut Vec<Highlight>) {
    let Some(unit) = features::parse_unit(uri, text) else {
        return;
    };
    let ns = &unit.namespace;
    if !ns.name.0.is_empty() {
        out.push(Highlight {
            span: features::find_name_region(text, ns.span, &ns.name.0),
            class: Class::Namespace,
            declaration: true,
        });
    }
    for import in &unit.imports {
        if import.namespace.0.is_empty() {
            continue;
        }
        out.push(Highlight {
            span: features::find_name_region(text, import.span, &import.namespace.0),
            class: Class::Namespace,
            declaration: false,
        });
    }
}

fn class_of_kind(kind: ElementKind) -> Class {
    match kind {
        ElementKind::Enumeration => Class::Enum,
        ElementKind::Annotation => Class::Decorator,
        ElementKind::LibraryFunction | ElementKind::Function | ElementKind::Rule => Class::Function,
        ElementKind::Body | ElementKind::Corpus => Class::Namespace,
        _ => Class::Type,
    }
}

fn push_decl(span: Span, name: &str, class: Class, text: &str, out: &mut Vec<Highlight>) {
    if name.is_empty() {
        return; // anonymous roots (reports)
    }
    out.push(Highlight {
        span: features::find_name_region(text, span, name),
        class,
        declaration: true,
    });
}

fn push_type_ref(resolution: &Resolution, r: &TypeRef, text: &str, out: &mut Vec<Highlight>) {
    let class = r
        .resolved
        .map(|id| class_of_kind(resolution.kind_of(ElementId(id))))
        .unwrap_or(Class::Type);
    out.push(Highlight {
        span: features::type_ref_name_span(r, text),
        class,
        declaration: false,
    });
}

/// A by-name reference with its own resolved id (`NamedRef` in the model:
/// report body/corpus/segment/eligibility/report-type/rule-source refs).
fn push_named_ref(
    resolution: &Resolution,
    resolved: Option<usize>,
    span: Span,
    name: &str,
    text: &str,
    out: &mut Vec<Highlight>,
) {
    let class = resolved
        .map(|id| class_of_kind(resolution.kind_of(ElementId(id))))
        .unwrap_or(Class::Type);
    out.push(Highlight {
        span: features::find_name_region(text, span, name),
        class,
        declaration: false,
    });
}

fn push_annotation_ref(a: &AnnotationRef, text: &str, out: &mut Vec<Highlight>) {
    let name_span = features::find_name_region(text, a.span, &a.annotation);
    out.push(Highlight {
        span: name_span,
        class: Class::Decorator,
        declaration: false,
    });
    // The referenced annotation's attribute (`[metadata id]`): no span in
    // the model, so re-derive it from the text after the name region.
    if let Some(attr) = &a.attribute {
        let rest = Span::new(name_span.end.min(a.span.end), a.span.end);
        if rest.start < rest.end {
            let attr_span = features::find_name_region(text, rest, attr);
            // A proper sub-region means the attribute was found; the
            // fallback would return `rest` unchanged.
            if attr_span.start > rest.start || attr_span.end < rest.end {
                out.push(Highlight {
                    span: attr_span,
                    class: Class::Property,
                    declaration: false,
                });
            }
        }
    }
}

fn push_attribute(a: &Attribute, resolution: &Resolution, text: &str, out: &mut Vec<Highlight>) {
    push_decl(a.span, &a.name, Class::Property, text, out);
    push_type_ref(resolution, &a.type_ref, text, out);
    for anno in &a.annotations {
        push_annotation_ref(anno, text, out);
    }
}

fn collect_resolution(world: &World, uri: &str, text: &str, out: &mut Vec<Highlight>) {
    let resolution = &world.analysis().resolution;
    let Some(file) = resolution.files.iter().find(|f| f.name == uri) else {
        return;
    };
    collect_file(resolution, file, text, out);
}

fn collect_file(resolution: &Resolution, file: &ModelFile, text: &str, out: &mut Vec<Highlight>) {
    for config in &file.configurations {
        push_type_ref(resolution, &config.root, text, out);
    }
    for element in &file.elements {
        match element {
            SemanticElement::Data(d) => {
                push_decl(d.span, &d.name, Class::Type, text, out);
                if let Some(s) = &d.super_type {
                    push_type_ref(resolution, s, text, out);
                }
                for anno in &d.annotations {
                    push_annotation_ref(anno, text, out);
                }
                for a in &d.attributes {
                    push_attribute(a, resolution, text, out);
                }
            }
            SemanticElement::Enumeration(e) => {
                push_decl(e.span, &e.name, Class::Enum, text, out);
                if let Some(s) = &e.super_type {
                    push_type_ref(resolution, s, text, out);
                }
                for anno in &e.annotations {
                    push_annotation_ref(anno, text, out);
                }
                for v in &e.values {
                    push_decl(v.span, &v.name, Class::EnumMember, text, out);
                    for anno in &v.annotations {
                        push_annotation_ref(anno, text, out);
                    }
                }
            }
            SemanticElement::Annotation(a) => {
                push_decl(a.span, &a.name, Class::Decorator, text, out);
                for attr in &a.attributes {
                    push_attribute(attr, resolution, text, out);
                }
            }
            SemanticElement::TypeAlias(t) => {
                push_decl(t.span, &t.name, Class::Type, text, out);
                for anno in &t.annotations {
                    push_annotation_ref(anno, text, out);
                }
                for p in &t.parameters {
                    push_type_ref(resolution, &p.type_ref, text, out);
                }
                push_type_ref(resolution, &t.type_ref, text, out);
            }
            SemanticElement::BasicType(b) => {
                push_decl(b.span, &b.name, Class::Type, text, out);
                for p in &b.parameters {
                    push_type_ref(resolution, &p.type_ref, text, out);
                }
            }
            SemanticElement::RecordType(r) => {
                push_decl(r.span, &r.name, Class::Type, text, out);
                for f in &r.features {
                    push_type_ref(resolution, &f.type_ref, text, out);
                }
            }
            SemanticElement::LibraryFunction(f) => {
                push_decl(f.span, &f.name, Class::Function, text, out);
                for p in &f.parameters {
                    push_type_ref(resolution, &p.type_ref, text, out);
                }
                push_type_ref(resolution, &f.return_type, text, out);
            }
            SemanticElement::Function(f) => {
                push_decl(f.span, &f.name, Class::Function, text, out);
                if let Some(s) = &f.super_function {
                    push_type_ref(resolution, s, text, out);
                }
                for anno in &f.annotations {
                    push_annotation_ref(anno, text, out);
                }
                for a in f.inputs.iter().chain(f.output.iter()) {
                    push_attribute(a, resolution, text, out);
                }
                for c in f.conditions.iter().chain(f.post_conditions.iter()) {
                    for anno in &c.annotations {
                        push_annotation_ref(anno, text, out);
                    }
                }
            }
            SemanticElement::Rule(r) => {
                push_decl(r.span, &r.name, Class::Function, text, out);
                if let Some(i) = &r.input {
                    push_type_ref(resolution, i, text, out);
                }
            }
            SemanticElement::Report(rep) => {
                let reg = &rep.regulatory;
                push_named_ref(
                    resolution,
                    reg.body.resolved,
                    reg.body.span,
                    &reg.body.name,
                    text,
                    out,
                );
                for c in reg.corpora.iter().chain(rep.eligibility_rules.iter()) {
                    push_named_ref(resolution, c.resolved, c.span, &c.name, text, out);
                }
                for s in &reg.segments {
                    push_named_ref(
                        resolution,
                        s.segment.resolved,
                        s.segment.span,
                        &s.segment.name,
                        text,
                        out,
                    );
                }
                push_type_ref(resolution, &rep.input_type, text, out);
                push_named_ref(
                    resolution,
                    rep.report_type.resolved,
                    rep.report_type.span,
                    &rep.report_type.name,
                    text,
                    out,
                );
                if let Some(s) = &rep.rule_source {
                    push_named_ref(resolution, s.resolved, s.span, &s.name, text, out);
                }
            }
            SemanticElement::ExternalRuleSource(s) => {
                push_decl(s.span, &s.name, Class::Type, text, out);
                if let Some(sup) = &s.super_source {
                    push_type_ref(resolution, sup, text, out);
                }
                for c in &s.classes {
                    push_type_ref(resolution, &c.data, text, out);
                }
            }
            SemanticElement::Schema(s) => {
                push_decl(s.span, &s.name, Class::Type, text, out);
                for anno in &s.annotations {
                    push_annotation_ref(anno, text, out);
                }
            }
            SemanticElement::Body(b) => push_decl(b.span, &b.name, Class::Namespace, text, out),
            SemanticElement::Corpus(c) => push_decl(c.span, &c.name, Class::Namespace, text, out),
            SemanticElement::Segment(s) => push_decl(s.span, &s.name, Class::Type, text, out),
            SemanticElement::MetaType(m) => {
                push_decl(m.span, &m.name, Class::Type, text, out);
                push_type_ref(resolution, &m.type_ref, text, out);
            }
        }
    }
}

// ---- text scan ----------------------------------------------------------------

/// Grammar keywords the scanner can classify directly: everything Xtext
/// reserves (the rename keyword list) plus the `isEvent`/`isProduct`
/// configuration keywords. Words the grammar's `ValidID` explicitly
/// allows as names (`condition`, `source`, ...) are absent — a resolving
/// name is classified by its highlight span instead.
fn is_keyword(word: &str) -> bool {
    features::RUNE_KEYWORDS.contains(&word) || matches!(word, "isEvent" | "isProduct")
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// One token from the byte-level text scan.
struct RawToken {
    span: Span,
    class: Class,
    declaration: bool,
}

/// Scan comments, strings, numbers and identifiers out of `text`,
/// classifying identifiers through the (sorted) highlight spans and the
/// keyword list. Mirrors the grammar's terminals: `//` line comments,
/// `/* */` block comments, single- or double-quoted strings with
/// backslash escapes, and `^`-escaped identifiers.
fn scan(text: &str, highlights: &[Highlight]) -> Vec<RawToken> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut hi = 0usize; // highlight cursor: highlights sorted by start
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                let end = text[i..]
                    .find('\n')
                    .map_or(bytes.len(), |rel| i + rel)
                    .min(bytes.len());
                out.push(RawToken {
                    span: Span::new(i, end),
                    class: Class::Comment,
                    declaration: false,
                });
                i = end;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let end = text[i + 2..]
                    .find("*/")
                    .map_or(bytes.len(), |rel| i + 2 + rel + 2)
                    .min(bytes.len());
                out.push(RawToken {
                    span: Span::new(i, end),
                    class: Class::Comment,
                    declaration: false,
                });
                i = end;
            }
            quote @ (b'"' | b'\'') => {
                let mut j = i + 1;
                while j < bytes.len() {
                    if bytes[j] == b'\\' {
                        j += 2; // the escape backslash and the escaped byte
                    } else if bytes[j] == quote {
                        j += 1;
                        break;
                    } else {
                        j += 1;
                    }
                }
                let end = j.min(bytes.len());
                out.push(RawToken {
                    span: Span::new(i, end),
                    class: Class::String,
                    declaration: false,
                });
                i = end;
            }
            b'0'..=b'9' => {
                let mut j = i + 1;
                while j < bytes.len()
                    && (bytes[j].is_ascii_digit()
                        || (bytes[j] == b'.'
                            && bytes.get(j + 1).is_some_and(|b| b.is_ascii_digit())))
                {
                    j += 1;
                }
                out.push(RawToken {
                    span: Span::new(i, j),
                    class: Class::Number,
                    declaration: false,
                });
                i = j;
            }
            b'^' if bytes.get(i + 1).is_some_and(|b| is_ident_start(*b)) => {
                let mut j = i + 2;
                while j < bytes.len() && is_ident_continue(bytes[j]) {
                    j += 1;
                }
                if let Some(token) = classify_ident(highlights, &mut hi, text, i, j) {
                    out.push(token);
                }
                i = j;
            }
            b if is_ident_start(b) => {
                let mut j = i + 1;
                while j < bytes.len() && is_ident_continue(bytes[j]) {
                    j += 1;
                }
                if let Some(token) = classify_ident(highlights, &mut hi, text, i, j) {
                    out.push(token);
                }
                i = j;
            }
            _ => i += 1,
        }
    }
    out
}

/// Classify one identifier occurrence `text[start..end]` (which may carry
/// a leading `^` escape). A containing highlight span wins — so keyword-
/// spelled declaration names are highlighted as declarations — otherwise
/// a Rune keyword is classified as such, and anything else (plain
/// identifiers) produces no token.
fn classify_ident(
    highlights: &[Highlight],
    hi: &mut usize,
    text: &str,
    start: usize,
    end: usize,
) -> Option<RawToken> {
    let span = Span::new(start, end);
    let name_start = if text.as_bytes()[start] == b'^' {
        start + 1
    } else {
        start
    };
    if let Some(h) = containing(highlights, hi, name_start, end) {
        return Some(RawToken {
            span,
            class: h.class,
            declaration: h.declaration,
        });
    }
    if is_keyword(&text[name_start..end]) {
        return Some(RawToken {
            span,
            class: Class::Keyword,
            declaration: false,
        });
    }
    None
}

/// The highlight span containing the identifier (`name_start..end`), via a
/// forward-only cursor over the start-sorted highlights. A highlight can
/// contain several identifiers (a qualified reference `a.b.C`), so the
/// cursor only advances past spans that end before the name begins.
fn containing<'a>(
    highlights: &'a [Highlight],
    hi: &mut usize,
    name_start: usize,
    end: usize,
) -> Option<&'a Highlight> {
    while *hi < highlights.len() && highlights[*hi].span.end <= name_start {
        *hi += 1;
    }
    let mut k = *hi;
    while k < highlights.len() && highlights[k].span.start <= name_start {
        if highlights[k].span.end >= end {
            return Some(&highlights[k]);
        }
        k += 1;
    }
    None
}

// ---- LSP encoding -------------------------------------------------------------

/// Split a raw token's span into per-line segments. Semantic tokens live on
/// a single line (the encoding has only a horizontal length), so multi-line
/// block comments and multi-line strings must be split.
fn line_segments(text: &str, span: Span) -> Vec<Span> {
    let end = span.end.min(text.len());
    if span.start >= end {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut start = span.start;
    for (rel, b) in text[span.start..end].bytes().enumerate() {
        if b == b'\n' {
            out.push(Span::new(start, span.start + rel));
            start = span.start + rel + 1;
        }
    }
    if start < end {
        out.push(Span::new(start, end));
    }
    out
}

/// Encode the scanned tokens into the LSP 3.16 relative-delta form:
/// `(deltaLine, deltaStart, length, tokenType, tokenModifiers)` quintuples
/// with `deltaStart`/`length` counted in the negotiated position encoding.
fn encode(
    line_index: &LineIndex,
    text: &str,
    tokens: Vec<RawToken>,
    enc: PositionEncoding,
) -> SemanticTokens {
    let mut flat: Vec<(u32, u32, u32, Class, bool)> = Vec::new(); // (line, char, len)
    for token in tokens {
        for seg in line_segments(text, token.span) {
            let start = line_index.position(text, seg.start, enc);
            let end = line_index.position(text, seg.end, enc);
            if end.line != start.line || end.character <= start.character {
                continue;
            }
            flat.push((
                start.line,
                start.character,
                end.character - start.character,
                token.class,
                token.declaration,
            ));
        }
    }
    flat.sort_by_key(|t| (t.0, t.1, t.2));

    let mut data = Vec::with_capacity(flat.len() * 5);
    let (mut prev_line, mut prev_char, mut prev_end) = (0u32, 0u32, 0u32);
    for (line, character, length, class, declaration) in flat {
        // Overlap insurance for degenerate spans: never emit a token that
        // starts inside the previous one on the same line (the protocol
        // cannot express negative deltas).
        if line == prev_line && character < prev_end {
            continue;
        }
        let delta_line = line - prev_line;
        let delta_start = if delta_line == 0 {
            character - prev_char
        } else {
            character
        };
        data.push(SemanticToken {
            delta_line,
            delta_start,
            length,
            token_type: class.index(),
            token_modifiers_bitset: if declaration { DECLARATION_BIT } else { 0 },
        });
        prev_line = line;
        prev_char = character;
        prev_end = character + length;
    }
    SemanticTokens {
        result_id: None,
        data,
    }
}
