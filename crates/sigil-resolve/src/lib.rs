//! Name resolution and scoping for Rune models.
//!
//! Reproduces the scoping semantics of the Java implementation
//! (`RosettaScopeProvider extends ImportedNamespaceAwareLocalScopeProvider`):
//!
//! 1. elements of the local file,
//! 2. explicit imports, in declaration order (first wins), honouring
//!    wildcards and aliases,
//! 3. a wildcard import of the file's own namespace (so same-namespace
//!    files can reference each other unqualified),
//! 4. the implicit wildcard import of the built-in `com.rosetta.model`
//!    namespace,
//! 5. fully-qualified names resolve against the global index.

use serde::Serialize;
use sigil_diag::Severity;
use sigil_model::{ModelFile, SemanticElement, TypeRef};

/// The namespace of Rune's built-in library, implicitly wildcard-imported
/// into every model.
pub const LIB_NAMESPACE: &str = "com.rosetta.model";

const BUILTIN_BASIC_TYPES: &str = include_str!("../builtin/basictypes.rosetta");
const BUILTIN_ANNOTATIONS: &str = include_str!("../builtin/annotations.rosetta");

/// A resolution-scope diagnostic. Resolution works on lowered models, which
/// do not carry source spans; diagnostics therefore point at the declaring
/// file and element path.
#[derive(Debug, Clone, Serialize)]
pub struct ResolutionDiagnostic {
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
    pub file: String,
    /// Where in the model the diagnostic applies, e.g. `type Foo.attributes[2]`.
    pub path: String,
}

impl ResolutionDiagnostic {
    fn error(code: &'static str, file: &str, path: &str, message: String) -> Self {
        ResolutionDiagnostic {
            severity: Severity::Error,
            code,
            message,
            file: file.to_string(),
            path: path.to_string(),
        }
    }
}

/// A global element identity: flat index across all files' elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ElementId(pub usize);

/// Resolution result: the fully-populated model plus diagnostics.
pub struct Resolution {
    /// Builtin files first, then user files in submission order.
    pub files: Vec<ModelFile>,
    /// Fully-qualified name of every element, aligned with flat element order.
    pub element_names: Vec<String>,
    pub diagnostics: Vec<ResolutionDiagnostic>,
}

impl Resolution {
    pub fn qname(&self, id: ElementId) -> &str {
        &self.element_names[id.0]
    }

    pub fn user_files(&self) -> &[ModelFile] {
        &self.files[2..]
    }
}

/// Load the built-in `.rosetta` files shipped with sigil (mirrors
/// `RosettaBuiltinsService`).
pub fn builtin_files() -> Vec<ModelFile> {
    let mut files = Vec::new();
    for (name, text) in [
        ("basictypes.rosetta", BUILTIN_BASIC_TYPES),
        ("annotations.rosetta", BUILTIN_ANNOTATIONS),
    ] {
        let source = sigil_diag::SourceFile::new(name, text);
        let (unit, diags) = sigil_syntax::parse(&source);
        if !diags.is_empty() || unit.is_none() {
            // Builtins must parse; failure is a programming error.
            panic!("builtin model {name} failed to parse: {diags:?}");
        }
        files.push(sigil_syntax::lower(name, &unit.unwrap()));
    }
    files
}

/// Resolve all references across `files` (which must NOT include builtins;
/// they are prepended automatically).
pub fn resolve(user_files: Vec<ModelFile>) -> Resolution {
    let mut files = builtin_files();
    files.extend(user_files);

    // ---- global symbol table -------------------------------------------
    let mut index = SymbolTable::default();
    let mut element_names: Vec<String> = Vec::new();
    let mut pre_diagnostics: Vec<ResolutionDiagnostic> = Vec::new();
    for file in &files {
        for element in &file.elements {
            let fqn = format!("{}.{}", file.namespace, element.name());
            let flat = element_names.len();
            if !index.insert(element, fqn.clone(), flat, file, &mut pre_diagnostics) {
                // Duplicate in its kind namespace: first definition wins.
            }
            element_names.push(fqn);
        }
    }

    let mut res = Resolution {
        files,
        element_names,
        diagnostics: pre_diagnostics,
    };

    // ---- per-file lookup contexts --------------------------------------
    let contexts: Vec<FileContext> = (0..res.files.len())
        .map(|i| build_context(&res, i))
        .collect();

    // ---- resolve every reference ---------------------------------------
    for (file_idx, ctx) in contexts.iter().enumerate() {
        let file = res.files[file_idx].clone();
        let mut resolved_file = file.clone();
        for element in &mut resolved_file.elements {
            let mut walker = ElementResolver {
                index: &index,
                ctx,
                file: &file,
                res: &mut res,
            };
            walker.resolve_element(element);
        }
        for config in &mut resolved_file.configurations {
            let mut walker = ElementResolver {
                index: &index,
                ctx,
                file: &file,
                res: &mut res,
            };
            walker.resolve_type_ref("configurations", &mut config.root, AnyType);
        }
        res.files[file_idx] = resolved_file;
    }

    // ---- inheritance cycle detection ------------------------------------
    detect_cycles(&mut res);

    res
}

// A tiny shim so the borrow checker is satisfied while walking elements with
// shared context and mutable diagnostics.
struct ElementResolver<'a> {
    index: &'a SymbolTable,
    ctx: &'a FileContext,
    file: &'a ModelFile,
    res: &'a mut Resolution,
}

#[allow(non_snake_case)]
struct AnyType;

impl<'a> ElementResolver<'a> {
    fn resolve_element(&mut self, element: &mut SemanticElement) {
        match element {
            SemanticElement::Data(data) => {
                let path = format!("type {}", data.name);
                if let Some(super_type) = &mut data.super_type {
                    self.resolve_parent(
                        &path,
                        super_type,
                        |e| matches!(e, SemanticElement::Data(_)),
                        "data type",
                    );
                }
                self.resolve_attributes(&path, &mut data.attributes);
                self.resolve_annotations(&path, &mut data.annotations);
            }
            SemanticElement::Enumeration(enumm) => {
                let path = format!("enum {}", enumm.name);
                if let Some(super_type) = &mut enumm.super_type {
                    self.resolve_parent(
                        &path,
                        super_type,
                        |e| matches!(e, SemanticElement::Enumeration(_)),
                        "enumeration",
                    );
                }
                self.resolve_annotations(&path, &mut enumm.annotations);
            }
            SemanticElement::Annotation(anno) => {
                let path = format!("annotation {}", anno.name);
                self.resolve_attributes(&path, &mut anno.attributes);
            }
            SemanticElement::TypeAlias(alias) => {
                let path = format!("typeAlias {}", alias.name);
                for parameter in &mut alias.parameters {
                    let p = format!("{path}.parameters[{}]", parameter.name);
                    self.resolve_type_ref(&p, &mut parameter.type_ref, AnyType);
                }
                let mut t = std::mem::take(&mut alias.type_ref);
                self.resolve_type_ref(&path, &mut t, AnyType);
                alias.type_ref = t;
                self.resolve_annotations(&path, &mut alias.annotations);
            }
            SemanticElement::BasicType(basic) => {
                let path = format!("basicType {}", basic.name);
                for parameter in &mut basic.parameters {
                    let p = format!("{path}.parameters[{}]", parameter.name);
                    self.resolve_type_ref(&p, &mut parameter.type_ref, AnyType);
                }
            }
            SemanticElement::RecordType(record) => {
                let path = format!("recordType {}", record.name);
                for feature in &mut record.features {
                    let p = format!("{path}.features[{}]", feature.name);
                    self.resolve_type_ref(&p, &mut feature.type_ref, AnyType);
                }
            }
            SemanticElement::LibraryFunction(function) => {
                let path = format!("library function {}", function.name);
                for parameter in &mut function.parameters {
                    let p = format!("{path}.parameters[{}]", parameter.name);
                    self.resolve_type_ref(&p, &mut parameter.type_ref, AnyType);
                }
                let mut t = std::mem::take(&mut function.return_type);
                self.resolve_type_ref(&format!("{path}.returnType"), &mut t, AnyType);
                function.return_type = t;
            }
        }
    }

    fn resolve_attributes(&mut self, path: &str, attributes: &mut [sigil_model::Attribute]) {
        for (idx, attribute) in attributes.iter_mut().enumerate() {
            let attr_path = format!("{path}.attributes[{idx}]");
            let mut t = std::mem::take(&mut attribute.type_ref);
            self.resolve_type_ref(&attr_path, &mut t, AnyType);
            attribute.type_ref = t;
            self.resolve_annotations(&attr_path, &mut attribute.annotations);
        }
    }

    fn resolve_parent(
        &mut self,
        path: &str,
        super_type: &mut TypeRef,
        kind: impl Fn(&SemanticElement) -> bool,
        kind_name: &str,
    ) {
        let mut t = std::mem::take(super_type);
        self.resolve_type_ref(path, &mut t, AnyType);
        if let Some(id) = t.resolved.map(ElementId) {
            if !kind(element_at(self.res, id)) {
                self.res.diagnostics.push(ResolutionDiagnostic::error(
                    "E0106",
                    &self.file.name,
                    path,
                    format!("super type '{}' is not a {kind_name}", self.res.qname(id)),
                ));
                t.resolved = None;
            }
        }
        *super_type = t;
    }

    fn resolve_type_ref(&mut self, path: &str, type_ref: &mut TypeRef, _kind: AnyType) {
        match lookup(self.res, self.ctx, self.index, false, &type_ref.name) {
            Some(id) => {
                if !element_at(self.res, id).is_type() {
                    self.res.diagnostics.push(ResolutionDiagnostic::error(
                        "E0106",
                        &self.file.name,
                        path,
                        format!("'{}' is not a type", self.res.qname(id)),
                    ));
                } else {
                    type_ref.resolved = Some(id.0);
                }
            }
            None => {
                self.res.diagnostics.push(ResolutionDiagnostic::error(
                    "E0101",
                    &self.file.name,
                    path,
                    format!("unknown type '{}'", type_ref.name),
                ));
            }
        }
    }

    fn resolve_annotations(&mut self, path: &str, annotations: &mut [sigil_model::AnnotationRef]) {
        for annotation in annotations.iter_mut() {
            match lookup(self.res, self.ctx, self.index, true, &annotation.annotation) {
                Some(id) => {
                    let target = element_at(self.res, id);
                    if let SemanticElement::Annotation(anno) = target {
                        if let Some(attr_name) = &annotation.attribute {
                            if !anno.attributes.iter().any(|a| a.name == *attr_name) {
                                self.res.diagnostics.push(ResolutionDiagnostic::error(
                                    "E0103",
                                    &self.file.name,
                                    path,
                                    format!(
                                        "annotation '{}' has no attribute '{}'",
                                        self.res.qname(id),
                                        attr_name
                                    ),
                                ));
                            }
                        }
                        annotation.annotation_resolved = Some(id.0);
                    } else {
                        self.res.diagnostics.push(ResolutionDiagnostic::error(
                            "E0102",
                            &self.file.name,
                            path,
                            format!("'{}' is not an annotation", self.res.qname(id)),
                        ));
                    }
                }
                None => {
                    self.res.diagnostics.push(ResolutionDiagnostic::error(
                        "E0102",
                        &self.file.name,
                        path,
                        format!("unknown annotation '{}'", annotation.annotation),
                    ));
                }
            }
        }
    }
}

// ---- scope chain --------------------------------------------------------

/// Kind-aware symbol table: Rune allows a `typeAlias calculation` and an
/// `annotation calculation` to coexist (Xtext scopes are EClass-filtered),
/// but duplicates within a kind are errors.
#[derive(Default)]
struct SymbolTable {
    types: std::collections::HashMap<String, ElementId>,
    annotations: std::collections::HashMap<String, ElementId>,
}

impl SymbolTable {
    fn insert(
        &mut self,
        element: &SemanticElement,
        fqn: String,
        flat: usize,
        file: &ModelFile,
        diagnostics: &mut Vec<ResolutionDiagnostic>,
    ) -> bool {
        let (table, kind_name) = match element {
            SemanticElement::Annotation(_) => (&mut self.annotations, "annotation"),
            _ => (&mut self.types, "element"),
        };
        if table.contains_key(&fqn) {
            diagnostics.push(ResolutionDiagnostic::error(
                "E0104",
                &file.name,
                element.name(),
                format!("duplicate definition of '{fqn}'"),
            ));
            false
        } else {
            let _ = kind_name;
            table.insert(fqn.clone(), ElementId(flat));
            true
        }
    }

    fn get(&self, fqn: &str, want_annotation: bool) -> Option<ElementId> {
        let table = if want_annotation {
            &self.annotations
        } else {
            &self.types
        };
        table.get(fqn).copied()
    }
}

struct FileContext {
    /// (simple name, flat element id)
    local: Vec<(String, usize)>,
    /// Import rules in declaration order (first match wins).
    imports: Vec<ImportRule>,
    namespace: String,
}

struct ImportRule {
    /// The alias, or the last segment of a non-wildcard import.
    key: Option<String>,
    /// Fully-qualified name of the imported element (non-wildcard).
    target: Option<String>,
    /// The namespace for wildcard imports.
    wildcard_namespace: Option<String>,
}

fn build_context(res: &Resolution, file_idx: usize) -> FileContext {
    let file = &res.files[file_idx];
    let offset = element_offset(res, file_idx);
    let local = (0..file.elements.len())
        .map(|i| {
            let flat = offset + i;
            (simple_name(&res.element_names[flat]).to_string(), flat)
        })
        .collect();

    let mut imports = Vec::new();
    for import in &file.imports {
        let ns = import.imported_namespace.trim_end_matches(".*").to_string();
        if import.wildcard {
            imports.push(ImportRule {
                key: import.namespace_alias.clone(),
                target: None,
                wildcard_namespace: Some(ns),
            });
        } else {
            let last = ns.rsplit('.').next().unwrap_or(&ns).to_string();
            imports.push(ImportRule {
                key: Some(import.namespace_alias.clone().unwrap_or(last)),
                target: Some(ns),
                wildcard_namespace: None,
            });
        }
    }
    FileContext {
        local,
        imports,
        namespace: file.namespace.clone(),
    }
}

fn lookup(
    res: &Resolution,
    ctx: &FileContext,
    index: &SymbolTable,
    want_annotation: bool,
    name: &str,
) -> Option<ElementId> {
    if let Some((head, rest)) = name.split_once('.') {
        // Alias-prefixed: `dep.MyFunc` where `import a.b.* as dep`.
        for rule in &ctx.imports {
            if rule.key.as_deref() == Some(head) {
                if let Some(target) = &rule.target {
                    if let Some(id) = index.get(&format!("{target}.{rest}"), want_annotation) {
                        return Some(id);
                    }
                }
                if let Some(wildcard_ns) = &rule.wildcard_namespace {
                    if let Some(id) = index.get(&format!("{wildcard_ns}.{rest}"), want_annotation) {
                        return Some(id);
                    }
                }
            }
        }
        // Fully-qualified name.
        index.get(name, want_annotation)
    } else {
        lookup_simple(res, ctx, index, want_annotation, name)
    }
}

fn lookup_simple(
    res: &Resolution,
    ctx: &FileContext,
    index: &SymbolTable,
    want_annotation: bool,
    name: &str,
) -> Option<ElementId> {
    // 1. local file
    if let Some((_, flat)) = ctx.local.iter().find(|(n, _)| n == name) {
        return Some(ElementId(*flat));
    }
    // 2. explicit imports, in order (first wins)
    for rule in &ctx.imports {
        if let Some(wildcard_ns) = &rule.wildcard_namespace {
            if let Some(id) = index.get(&format!("{wildcard_ns}.{name}"), want_annotation) {
                return Some(id);
            }
        } else if rule.key.as_deref() == Some(name) {
            if let Some(target) = &rule.target {
                if let Some(id) = index.get(target, want_annotation) {
                    return Some(id);
                }
            }
        }
    }
    // 3. own namespace (elements in other files with the same namespace)
    if let Some(id) = index.get(&format!("{}.{}", ctx.namespace, name), want_annotation) {
        return Some(id);
    }
    // 4. implicit import of the built-in library namespace
    if ctx.namespace != LIB_NAMESPACE {
        if let Some(id) = index.get(&format!("{LIB_NAMESPACE}.{name}"), want_annotation) {
            return Some(id);
        }
    }
    let _ = res;
    None
}

// ---- helpers ------------------------------------------------------------

fn element_offset(res: &Resolution, file_idx: usize) -> usize {
    res.files[..file_idx].iter().map(|f| f.elements.len()).sum()
}

fn simple_name(qname: &str) -> &str {
    qname.rsplit('.').next().unwrap_or(qname)
}

fn element_at(res: &Resolution, id: ElementId) -> &SemanticElement {
    let mut count = 0usize;
    for file in &res.files {
        if id.0 < count + file.elements.len() {
            return &file.elements[id.0 - count];
        }
        count += file.elements.len();
    }
    unreachable!("element id out of range")
}

fn element_flat_index(res: &Resolution, namespace: &str, name: &str) -> Option<usize> {
    let mut count = 0usize;
    for file in &res.files {
        for element in &file.elements {
            if file.namespace == namespace && element.name() == name {
                return Some(count);
            }
            count += 1;
        }
    }
    None
}

/// Detect cycles in data/enum inheritance graphs (iterative DFS).
fn detect_cycles(res: &mut Resolution) {
    let n = res.element_names.len();
    let mut graph: Vec<Vec<usize>> = vec![Vec::new(); n];
    for file in &res.files {
        for element in &file.elements {
            let (this, parent) = match element {
                SemanticElement::Data(d) => (
                    element_flat_index(res, &file.namespace, &d.name),
                    d.super_type.as_ref().and_then(|t| t.resolved),
                ),
                SemanticElement::Enumeration(e) => (
                    element_flat_index(res, &file.namespace, &e.name),
                    e.super_type.as_ref().and_then(|t| t.resolved),
                ),
                _ => continue,
            };
            if let (Some(this), Some(parent)) = (this, parent) {
                graph[this].push(parent);
            }
        }
    }
    let mut colour = vec![0u8; n]; // 0=white 1=grey 2=black
    for start in 0..n {
        if colour[start] != 0 {
            continue;
        }
        let mut stack = vec![start];
        let mut pushed_grey = false;
        while let Some(&node) = stack.last() {
            if colour[node] == 0 {
                colour[node] = 1;
                for &next in &graph[node] {
                    match colour[next] {
                        0 => stack.push(next),
                        1 if !pushed_grey => {
                            res.diagnostics.push(ResolutionDiagnostic::error(
                                "E0105",
                                file_of_element(res, next),
                                res.qname(ElementId(next)),
                                format!(
                                    "inheritance cycle involving '{}'",
                                    res.qname(ElementId(next))
                                ),
                            ));
                            pushed_grey = true;
                        }
                        _ => {}
                    }
                }
            } else {
                colour[node] = 2;
                stack.pop();
            }
        }
    }
}

fn file_of_element(res: &Resolution, flat: usize) -> &str {
    let mut count = 0usize;
    for file in &res.files {
        if flat < count + file.elements.len() {
            return &file.name;
        }
        count += file.elements.len();
    }
    ""
}

// ---- canonical JSON -----------------------------------------------------

use serde_json::json;

fn type_json(type_ref: &TypeRef, res: &Resolution) -> serde_json::Value {
    json!({
        "name": type_ref.name,
        "arguments": type_ref.arguments,
        "resolved": type_ref.resolved.map(|id| res.element_names[id].clone()),
    })
}

fn annotations_json(annos: &[sigil_model::AnnotationRef], res: &Resolution) -> serde_json::Value {
    serde_json::Value::Array(
        annos
            .iter()
            .map(|a| {
                json!({
                    "annotation": {
                        "name": a.annotation,
                        "resolved": a.annotation_resolved.map(|id| res.element_names[id].clone()),
                    },
                    "attribute": a.attribute,
                    "qualifiers": a.qualifiers,
                })
            })
            .collect(),
    )
}

fn attributes_json(attributes: &[sigil_model::Attribute], res: &Resolution) -> serde_json::Value {
    serde_json::Value::Array(
        attributes
            .iter()
            .map(|a| {
                json!({
                    "name": a.name,
                    "override": a.is_override,
                    "type": type_json(&a.type_ref, res),
                    "cardinality": a.cardinality.to_constraint_string(),
                    "definition": a.definition,
                    "annotations": annotations_json(&a.annotations, res),
                    "labels": a.labels,
                    "ruleReferences": a.rule_references,
                    "docReferences": a.doc_references,
                })
            })
            .collect(),
    )
}

fn parameters_json(
    parameters: &[sigil_model::TypeParameter],
    res: &Resolution,
) -> serde_json::Value {
    serde_json::Value::Array(
        parameters
            .iter()
            .map(|p| {
                json!({
                    "name": p.name,
                    "type": type_json(&p.type_ref, res),
                    "definition": p.definition,
                })
            })
            .collect(),
    )
}

fn element_json(element: &SemanticElement, res: &Resolution) -> serde_json::Value {
    match element {
        SemanticElement::Data(d) => json!({
            "kind": if d.is_choice { "Choice" } else { "Data" },
            "name": d.name,
            "definition": d.definition,
            "superType": d.super_type.as_ref().map(|t| type_json(t, res)),
            "annotations": annotations_json(&d.annotations, res),
            "docReferences": d.doc_references,
            "attributes": attributes_json(&d.attributes, res),
        }),
        SemanticElement::Enumeration(e) => json!({
            "kind": "Enumeration",
            "name": e.name,
            "definition": e.definition,
            "superType": e.super_type.as_ref().map(|t| type_json(t, res)),
            "annotations": annotations_json(&e.annotations, res),
            "docReferences": e.doc_references,
            "values": serde_json::Value::Array(
                e.values.iter().map(|v| json!({
                    "name": v.name,
                    "display": v.display,
                    "definition": v.definition,
                    "annotations": annotations_json(&v.annotations, res),
                    "docReferences": v.doc_references,
                })).collect(),
            ),
        }),
        SemanticElement::Annotation(a) => json!({
            "kind": "Annotation",
            "name": a.name,
            "definition": a.definition,
            "prefix": a.prefix,
            "attributes": attributes_json(&a.attributes, res),
        }),
        SemanticElement::TypeAlias(t) => json!({
            "kind": "TypeAlias",
            "name": t.name,
            "definition": t.definition,
            "parameters": parameters_json(&t.parameters, res),
            "type": type_json(&t.type_ref, res),
            "annotations": annotations_json(&t.annotations, res),
        }),
        SemanticElement::BasicType(b) => json!({
            "kind": "BasicType",
            "name": b.name,
            "definition": b.definition,
            "parameters": parameters_json(&b.parameters, res),
        }),
        SemanticElement::RecordType(r) => json!({
            "kind": "RecordType",
            "name": r.name,
            "definition": r.definition,
            "features": serde_json::Value::Array(
                r.features.iter().map(|f| json!({
                    "name": f.name,
                    "type": type_json(&f.type_ref, res),
                })).collect(),
            ),
        }),
        SemanticElement::LibraryFunction(f) => json!({
            "kind": "LibraryFunction",
            "name": f.name,
            "definition": f.definition,
            "parameters": serde_json::Value::Array(
                f.parameters.iter().map(|p| json!({
                    "name": p.name,
                    "type": type_json(&p.type_ref, res),
                    "isArray": p.is_array,
                })).collect(),
            ),
            "returnType": type_json(&f.return_type, res),
        }),
    }
}

/// Canonical JSON rendering of a resolved model: the compatibility artifact
/// compared against the Java oracle.
pub fn canonical_json(resolution: &Resolution) -> serde_json::Value {
    let files: Vec<serde_json::Value> = resolution
        .files
        .iter()
        .map(|file| {
            json!({
                "name": file.name,
                "namespace": file.namespace,
                "overridden": file.overridden,
                "scope": file.scope.as_ref().map(|s| json!({
                    "name": s.name,
                    "definition": s.definition,
                })),
                "version": file.version,
                "imports": file.imports,
                "configurations": file.configurations,
                "elements": serde_json::Value::Array(
                    file.elements.iter().map(|e| element_json(e, resolution)).collect(),
                ),
            })
        })
        .collect();

    let diagnostics: Vec<serde_json::Value> = resolution
        .diagnostics
        .iter()
        .map(|d| {
            json!({
                "severity": d.severity.as_str(),
                "code": d.code,
                "message": d.message,
                "file": d.file,
                "path": d.path,
            })
        })
        .collect();

    json!({
        "format": "sigil-model/1",
        "files": files,
        "diagnostics": diagnostics,
    })
}
