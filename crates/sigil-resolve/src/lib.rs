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
use sigil_diag::{Severity, Span};
use sigil_model::{ModelFile, SemanticElement, TypeRef};

/// The namespace of Rune's built-in library, implicitly wildcard-imported
/// into every model.
pub const LIB_NAMESPACE: &str = "com.rosetta.model";

const BUILTIN_BASIC_TYPES: &str = include_str!("../builtin/basictypes.rosetta");
const BUILTIN_ANNOTATIONS: &str = include_str!("../builtin/annotations.rosetta");

/// A resolution-scope diagnostic. Points at the declaring file, the model
/// path, and — since the lowered model carries spans — at the byte range of
/// the offending reference or declaration.
#[derive(Debug, Clone, Serialize)]
pub struct ResolutionDiagnostic {
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
    pub file: String,
    /// Where in the model the diagnostic applies, e.g. `type Foo.attributes[2]`.
    pub path: String,
    /// Byte range into `file` of the offending reference or declaration;
    /// `None` when no span is available. (Not serialized: positions are
    /// rendered by `canonical_json`.)
    #[serde(skip_serializing)]
    pub span: Option<Span>,
}

impl ResolutionDiagnostic {
    fn error(
        code: &'static str,
        file: &str,
        path: &str,
        message: String,
        span: Option<Span>,
    ) -> Self {
        ResolutionDiagnostic {
            severity: Severity::Error,
            code,
            message,
            file: file.to_string(),
            path: path.to_string(),
            span,
        }
    }
}

/// A global element identity: flat index across all files' elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ElementId(pub usize);

/// The declared kind of a top-level model element: one per
/// [`SemanticElement`] variant (i.e. one per EClass of the Rune metamodel
/// that appears as a model root).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ElementKind {
    Data,
    Enumeration,
    Annotation,
    TypeAlias,
    BasicType,
    RecordType,
    LibraryFunction,
    Function,
    Rule,
    Report,
    ExternalRuleSource,
    Schema,
    Body,
    Corpus,
    Segment,
    MetaType,
}

impl ElementKind {
    /// Every kind, in [`SemanticElement`] variant order.
    pub const ALL: &'static [ElementKind] = &[
        ElementKind::Data,
        ElementKind::Enumeration,
        ElementKind::Annotation,
        ElementKind::TypeAlias,
        ElementKind::BasicType,
        ElementKind::RecordType,
        ElementKind::LibraryFunction,
        ElementKind::Function,
        ElementKind::Rule,
        ElementKind::Report,
        ElementKind::ExternalRuleSource,
        ElementKind::Schema,
        ElementKind::Body,
        ElementKind::Corpus,
        ElementKind::Segment,
        ElementKind::MetaType,
    ];

    /// The kind of `element`.
    pub fn of(element: &SemanticElement) -> ElementKind {
        match element {
            SemanticElement::Data(_) => ElementKind::Data,
            SemanticElement::Enumeration(_) => ElementKind::Enumeration,
            SemanticElement::Annotation(_) => ElementKind::Annotation,
            SemanticElement::TypeAlias(_) => ElementKind::TypeAlias,
            SemanticElement::BasicType(_) => ElementKind::BasicType,
            SemanticElement::RecordType(_) => ElementKind::RecordType,
            SemanticElement::LibraryFunction(_) => ElementKind::LibraryFunction,
            SemanticElement::Function(_) => ElementKind::Function,
            SemanticElement::Rule(_) => ElementKind::Rule,
            SemanticElement::Report(_) => ElementKind::Report,
            SemanticElement::ExternalRuleSource(_) => ElementKind::ExternalRuleSource,
            SemanticElement::Schema(_) => ElementKind::Schema,
            SemanticElement::Body(_) => ElementKind::Body,
            SemanticElement::Corpus(_) => ElementKind::Corpus,
            SemanticElement::Segment(_) => ElementKind::Segment,
            SemanticElement::MetaType(_) => ElementKind::MetaType,
        }
    }
}

/// Resolution result: the fully-populated model plus diagnostics.
pub struct Resolution {
    /// Builtin files first, then user files in submission order.
    pub files: Vec<ModelFile>,
    /// Fully-qualified name of every element, aligned with flat element order.
    pub element_names: Vec<String>,
    /// Source span of every element's declaration, aligned with
    /// `element_names`.
    pub element_spans: Vec<Span>,
    pub diagnostics: Vec<ResolutionDiagnostic>,
}

impl Resolution {
    pub fn qname(&self, id: ElementId) -> &str {
        &self.element_names[id.0]
    }

    /// The element with flat id `id`.
    pub fn element(&self, id: ElementId) -> &SemanticElement {
        let mut count = 0usize;
        for file in &self.files {
            if id.0 < count + file.elements.len() {
                return &file.elements[id.0 - count];
            }
            count += file.elements.len();
        }
        unreachable!("element id out of range")
    }

    /// The declared kind of the element with flat id `id`.
    pub fn kind_of(&self, id: ElementId) -> ElementKind {
        ElementKind::of(self.element(id))
    }

    /// All elements of `kind`, as `(id, element)` pairs ordered by
    /// [`ElementId`] (i.e. declaration order). Includes builtins; to
    /// exclude them, keep only ids whose file falls outside
    /// [`Resolution::user_files`] — the builtin prefix is always exactly
    /// two files (flat file indices 0-1), but element ids start after the
    /// entire builtin element count, not at 2.
    pub fn elements_of_kind(&self, kind: ElementKind) -> Vec<(ElementId, &SemanticElement)> {
        (0..self.element_names.len())
            .map(ElementId)
            .filter(|&id| self.kind_of(id) == kind)
            .map(|id| (id, self.element(id)))
            .collect()
    }

    /// The first element whose fully-qualified name is `name`, falling back
    /// to the first whose simple (unqualified) name matches; `None` when no
    /// element matches. Anonymous elements (reports) have no name and are
    /// never returned by the simple-name fallback.
    pub fn find_by_name(&self, name: &str) -> Option<(ElementId, &SemanticElement)> {
        if let Some(pos) = self.element_names.iter().position(|n| n == name) {
            return Some((ElementId(pos), self.element(ElementId(pos))));
        }
        (0..self.element_names.len())
            .map(ElementId)
            .find(|&id| {
                let qname = &self.element_names[id.0];
                !qname.is_empty()
                    && qname.rsplit('.').next().unwrap_or(qname) == name
                    && ElementKind::of(self.element(id)) != ElementKind::Report
            })
            .map(|id| (id, self.element(id)))
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
    let mut element_spans: Vec<Span> = Vec::new();
    let mut pre_diagnostics: Vec<ResolutionDiagnostic> = Vec::new();
    for file in &files {
        for element in &file.elements {
            let fqn = format!("{}.{}", file.namespace, element.name());
            let flat = element_names.len();
            if !index.insert(element, fqn.clone(), flat, file, &mut pre_diagnostics) {
                // Duplicate in its kind namespace: first definition wins.
            }
            element_names.push(fqn);
            element_spans.push(element.span());
        }
    }

    let mut res = Resolution {
        files,
        element_names,
        element_spans,
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
                // Condition expressions see the attributes of the enclosing
                // type, including inherited ones (the implicit-variable
                // features), then fall back to the file scope.
                let mut frame: Vec<String> =
                    data.attributes.iter().map(|a| a.name.clone()).collect();
                if let Some(super_id) = data.super_type.as_ref().and_then(|t| t.resolved) {
                    for name in feature_names(self.res, ElementId(super_id)) {
                        if !frame.contains(&name) {
                            frame.push(name);
                        }
                    }
                }
                for condition in data.conditions.iter_mut() {
                    let c_path = format!(
                        "{path}.conditions[{}]",
                        condition.name.as_deref().unwrap_or("")
                    );
                    self.resolve_annotations(&c_path, &mut condition.annotations);
                    self.resolve_expr_heads(
                        &c_path,
                        &mut condition.expression,
                        &mut vec![frame.clone()],
                    );
                }
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
            SemanticElement::Function(function) => {
                let path = format!("func {}", function.name);
                if let Some(super_function) = &mut function.super_function {
                    self.resolve_parent(
                        &path,
                        super_function,
                        |e| matches!(e, SemanticElement::Function(_)),
                        "function",
                    );
                }
                self.resolve_annotations(&path, &mut function.annotations);
                self.resolve_attributes(&path, &mut function.inputs);
                if let Some(output) = &mut function.output {
                    let o_path = format!("{path}.output");
                    let mut t = std::mem::take(&mut output.type_ref);
                    self.resolve_type_ref(&o_path, &mut t, AnyType);
                    output.type_ref = t;
                    self.resolve_annotations(&o_path, &mut output.annotations);
                }

                let inputs: Vec<String> = function.inputs.iter().map(|a| a.name.clone()).collect();
                let shortcuts: Vec<String> =
                    function.shortcuts.iter().map(|s| s.name.clone()).collect();
                let output_name = function.output.as_ref().map(|a| a.name.clone());

                // The dispatch attribute resolves to an input attribute;
                // the enumeration to an enum; the value to one of its values.
                if let Some(dispatch) = &mut function.dispatch {
                    if !inputs.contains(&dispatch.attribute) {
                        self.res.diagnostics.push(ResolutionDiagnostic::error(
                            "E0107",
                            &self.file.name,
                            &format!("{path}.dispatch"),
                            format!(
                                "dispatch attribute '{}' is not an input of '{}'",
                                dispatch.attribute, function.name
                            ),
                            Some(dispatch.attribute_span),
                        ));
                    }
                    let mut enumeration = TypeRef::unresolved_spanned(
                        dispatch.enumeration.clone(),
                        dispatch.enumeration_span,
                    );
                    self.resolve_type_ref(&format!("{path}.dispatch"), &mut enumeration, AnyType);
                    let enum_id = enumeration.resolved;
                    if let Some(id) = enum_id {
                        if !matches!(
                            element_at(self.res, ElementId(id)),
                            SemanticElement::Enumeration(_)
                        ) {
                            self.res.diagnostics.push(ResolutionDiagnostic::error(
                                "E0106",
                                &self.file.name,
                                &format!("{path}.dispatch"),
                                format!(
                                    "'{}' is not an enumeration",
                                    self.res.qname(ElementId(id))
                                ),
                                Some(dispatch.enumeration_span),
                            ));
                        } else if !enum_value_names(self.res, ElementId(id))
                            .contains(&dispatch.value)
                        {
                            self.res.diagnostics.push(ResolutionDiagnostic::error(
                                "E0107",
                                &self.file.name,
                                &format!("{path}.dispatch"),
                                format!(
                                    "'{}' is not a value of '{}'",
                                    dispatch.value,
                                    self.res.qname(ElementId(id))
                                ),
                                Some(dispatch.value_span),
                            ));
                        }
                    }
                }

                for (idx, shortcut) in function.shortcuts.iter_mut().enumerate() {
                    let s_path = format!("{path}.shortcuts[{}]", shortcut.name);
                    // An alias cannot reference itself, but sees all other
                    // aliases, the inputs and the output.
                    let mut frame = inputs.clone();
                    frame.extend(shortcuts.iter().filter(|n| *n != &shortcut.name).cloned());
                    if let Some(out) = &output_name {
                        frame.push(out.clone());
                    }
                    self.resolve_expr_heads(&s_path, &mut shortcut.expression, &mut vec![frame]);
                    let _ = idx;
                }
                for condition in function.conditions.iter_mut() {
                    let c_path = format!(
                        "{path}.conditions[{}]",
                        condition.name.as_deref().unwrap_or("")
                    );
                    self.resolve_annotations(&c_path, &mut condition.annotations);
                    // Non-post conditions must not see the output.
                    let mut frame = inputs.clone();
                    frame.extend(shortcuts.clone());
                    self.resolve_expr_heads(&c_path, &mut condition.expression, &mut vec![frame]);
                }
                for (idx, operation) in function.operations.iter_mut().enumerate() {
                    let o_path = format!("{path}.operations[{idx}]");
                    // The assign root resolves to the output first, then to
                    // the shortcuts — never to an input.
                    let mut roots: Vec<String> = output_name.iter().cloned().collect();
                    roots.extend(shortcuts.iter().cloned());
                    if !roots.contains(&operation.assign_root) {
                        self.res.diagnostics.push(ResolutionDiagnostic::error(
                            "E0101",
                            &self.file.name,
                            &format!("{o_path}.assignRoot"),
                            format!("unknown symbol '{}'", operation.assign_root),
                            Some(operation.assign_root_span),
                        ));
                    } else if !operation.path.is_empty() {
                        // Segments resolve against the receiver type chain
                        // starting at the assign root's type. Alias roots
                        // have no inferable type, so their paths are left
                        // unresolved (documented limitation).
                        let mut type_id = if Some(&operation.assign_root) == output_name.as_ref() {
                            function.output.as_ref().and_then(|o| o.type_ref.resolved)
                        } else {
                            None
                        };
                        for (seg_idx, segment) in operation.path.iter_mut().enumerate() {
                            if let Some(id) = type_id {
                                match attribute_type(self.res, ElementId(id), &segment.feature) {
                                    Some(next) => {
                                        segment.resolved = true;
                                        type_id = next;
                                    }
                                    None => {
                                        self.res.diagnostics.push(ResolutionDiagnostic::error(
                                            "E0107",
                                            &self.file.name,
                                            &format!("{o_path}.path[{seg_idx}]"),
                                            format!(
                                                "unknown attribute '{}' on '{}'",
                                                segment.feature,
                                                self.res.qname(ElementId(id))
                                            ),
                                            Some(segment.span),
                                        ));
                                        type_id = None;
                                    }
                                }
                            } else {
                                break;
                            }
                        }
                    }
                    let mut frame = inputs.clone();
                    if let Some(out) = &output_name {
                        frame.push(out.clone());
                    }
                    frame.extend(shortcuts.iter().cloned());
                    self.resolve_expr_heads(&o_path, &mut operation.expression, &mut vec![frame]);
                }
                for condition in function.post_conditions.iter_mut() {
                    let c_path = format!(
                        "{path}.postConditions[{}]",
                        condition.name.as_deref().unwrap_or("")
                    );
                    self.resolve_annotations(&c_path, &mut condition.annotations);
                    // Post-conditions see everything.
                    let mut frame = inputs.clone();
                    if let Some(out) = &output_name {
                        frame.push(out.clone());
                    }
                    frame.extend(shortcuts.iter().cloned());
                    self.resolve_expr_heads(&c_path, &mut condition.expression, &mut vec![frame]);
                }
            }
            SemanticElement::Rule(rule) => {
                let path = format!("rule {}", rule.name);
                if let Some(input) = &mut rule.input {
                    let mut t = std::mem::take(input);
                    self.resolve_type_ref(&format!("{path}.input"), &mut t, AnyType);
                    *input = t;
                }
                // The rule expression sees the input type's attributes (the
                // implicit-variable features) and the file scope.
                let frame = rule
                    .input
                    .as_ref()
                    .and_then(|t| t.resolved)
                    .map(|id| feature_names(self.res, ElementId(id)))
                    .unwrap_or_default();
                self.resolve_expr_heads(&path, &mut rule.expression, &mut vec![frame]);
            }
            SemanticElement::Report(report) => {
                let path = "report".to_string();
                let reg = &mut report.regulatory;
                self.resolve_named_ref(
                    &format!("{path}.body"),
                    &mut reg.body,
                    |e| matches!(e, SemanticElement::Body(_)),
                    "body",
                );
                for corpus in reg.corpora.iter_mut() {
                    self.resolve_named_ref(
                        &format!("{path}.corpora"),
                        corpus,
                        |e| matches!(e, SemanticElement::Corpus(_)),
                        "corpus",
                    );
                }
                for (idx, segment) in reg.segments.iter_mut().enumerate() {
                    self.resolve_named_ref(
                        &format!("{path}.segments[{idx}]"),
                        &mut segment.segment,
                        |e| matches!(e, SemanticElement::Segment(_)),
                        "segment",
                    );
                }
                let mut t = std::mem::take(&mut report.input_type);
                self.resolve_type_ref(&format!("{path}.inputType"), &mut t, AnyType);
                report.input_type = t;
                for (idx, rule) in report.eligibility_rules.iter_mut().enumerate() {
                    self.resolve_named_ref(
                        &format!("{path}.eligibilityRules[{idx}]"),
                        rule,
                        |e| matches!(e, SemanticElement::Rule(_)),
                        "rule",
                    );
                }
                self.resolve_named_ref(
                    &format!("{path}.reportType"),
                    &mut report.report_type,
                    |e| matches!(e, SemanticElement::Data(_)),
                    "data type",
                );
                if let Some(source) = &mut report.rule_source {
                    self.resolve_named_ref(
                        &format!("{path}.ruleSource"),
                        source,
                        |e| matches!(e, SemanticElement::ExternalRuleSource(_)),
                        "rule source",
                    );
                }
            }
            SemanticElement::ExternalRuleSource(source) => {
                let path = format!("rule source {}", source.name);
                if let Some(super_source) = &mut source.super_source {
                    self.resolve_parent(
                        &path,
                        super_source,
                        |e| matches!(e, SemanticElement::ExternalRuleSource(_)),
                        "rule source",
                    );
                }
                for (class_idx, class) in source.classes.iter_mut().enumerate() {
                    let c_path = format!("{path}.externalClasses[{class_idx}]");
                    let mut t = std::mem::take(&mut class.data);
                    self.resolve_type_ref(&c_path, &mut t, AnyType);
                    if let Some(id) = t.resolved {
                        if !matches!(
                            element_at(self.res, ElementId(id)),
                            SemanticElement::Data(_)
                        ) {
                            self.res.diagnostics.push(ResolutionDiagnostic::error(
                                "E0106",
                                &self.file.name,
                                &c_path,
                                format!("'{}' is not a data type", self.res.qname(ElementId(id))),
                                Some(t.span),
                            ));
                            t.resolved = None;
                        }
                    }
                    class.data = t;
                    let data_id = class.data.resolved;
                    for (attr_idx, attribute) in class.attributes.iter_mut().enumerate() {
                        let a_path = format!("{c_path}.attributes[{attr_idx}]");
                        if let Some(id) = data_id {
                            if attribute_exists(self.res, ElementId(id), &attribute.attribute) {
                                attribute.resolved = true;
                            } else {
                                self.res.diagnostics.push(ResolutionDiagnostic::error(
                                    "E0107",
                                    &self.file.name,
                                    &a_path,
                                    format!(
                                        "unknown attribute '{}' on '{}'",
                                        attribute.attribute,
                                        self.res.qname(ElementId(id))
                                    ),
                                    Some(attribute.attribute_span),
                                ));
                            }
                        }
                        for rule in attribute.rule_references.iter_mut() {
                            self.resolve_rule_ref(&a_path, rule);
                        }
                    }
                }
            }
            SemanticElement::Schema(schema) => {
                let path = format!("schema {}", schema.name);
                self.resolve_annotations(&path, &mut schema.annotations);
            }
            SemanticElement::Body(_) => {}
            SemanticElement::Corpus(_) => {}
            SemanticElement::Segment(_) => {}
            SemanticElement::MetaType(meta) => {
                let path = format!("metaType {}", meta.name);
                let mut t = std::mem::take(&mut meta.type_ref);
                self.resolve_type_ref(&path, &mut t, AnyType);
                meta.type_ref = t;
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
        // Super references may point at non-`RosettaType` elements (a
        // function's `extends`, for instance), so the target is looked up
        // and kind-checked here instead of going through the type-ref
        // path's `is_type` filter.
        match lookup(self.res, self.ctx, self.index, false, &t.name) {
            Some(id) => {
                if kind(element_at(self.res, id)) {
                    t.resolved = Some(id.0);
                } else {
                    self.res.diagnostics.push(ResolutionDiagnostic::error(
                        "E0106",
                        &self.file.name,
                        path,
                        format!("'{}' is not a {kind_name}", self.res.qname(id)),
                        Some(t.span),
                    ));
                }
            }
            None => {
                self.res.diagnostics.push(ResolutionDiagnostic::error(
                    "E0101",
                    &self.file.name,
                    path,
                    format!("unknown {} '{}'", kind_name, t.name),
                    Some(t.span),
                ));
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
                        Some(type_ref.span),
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
                    Some(type_ref.span),
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
                                    Some(annotation.span),
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
                            Some(annotation.span),
                        ));
                    }
                }
                None => {
                    self.res.diagnostics.push(ResolutionDiagnostic::error(
                        "E0102",
                        &self.file.name,
                        path,
                        format!("unknown annotation '{}'", annotation.annotation),
                        Some(annotation.span),
                    ));
                }
            }
        }
    }

    /// Resolve a by-name reference to a named element (report parts).
    fn resolve_named_ref(
        &mut self,
        path: &str,
        reference: &mut sigil_model::NamedRef,
        kind: impl Fn(&SemanticElement) -> bool,
        kind_name: &str,
    ) {
        let name = reference.name.clone();
        match lookup(self.res, self.ctx, self.index, false, &name) {
            Some(id) => {
                if kind(element_at(self.res, id)) {
                    reference.resolved = Some(id.0);
                } else {
                    self.res.diagnostics.push(ResolutionDiagnostic::error(
                        "E0106",
                        &self.file.name,
                        path,
                        format!("'{}' is not a {kind_name}", self.res.qname(id)),
                        Some(reference.span),
                    ));
                }
            }
            None => {
                self.res.diagnostics.push(ResolutionDiagnostic::error(
                    "E0101",
                    &self.file.name,
                    path,
                    format!("unknown {kind_name} '{}'", name),
                    Some(reference.span),
                ));
            }
        }
    }

    /// Resolve a `[ruleReference R]` target (a reporting rule).
    fn resolve_rule_ref(&mut self, path: &str, rule: &mut sigil_model::RuleReference) {
        if rule.empty {
            return;
        }
        let Some(name) = rule.rule.clone() else {
            return;
        };
        match lookup(self.res, self.ctx, self.index, false, &name) {
            Some(id) => {
                if matches!(element_at(self.res, id), SemanticElement::Rule(_)) {
                    rule.resolved = Some(id.0);
                } else {
                    self.res.diagnostics.push(ResolutionDiagnostic::error(
                        "E0106",
                        &self.file.name,
                        path,
                        format!("'{}' is not a rule", self.res.qname(id)),
                        None,
                    ));
                }
            }
            None => {
                self.res.diagnostics.push(ResolutionDiagnostic::error(
                    "E0101",
                    &self.file.name,
                    path,
                    format!("unknown rule '{}'", name),
                    None,
                ));
            }
        }
    }

    /// Resolve the head symbols of every `SymbolReference` in an
    /// expression against the local scope frames (innermost last) and,
    /// failing those, the file scope. This mirrors Xtext's
    /// `ROSETTA_SYMBOL_REFERENCE__SYMBOL` scoping: function-local symbols
    /// (inputs, output, aliases, closure parameters, enclosing-type
    /// attributes) shadow the file-wide element scope. Only the head is
    /// resolved — `->` feature segments inside expressions are parsed but
    /// not type-checked (see docs/compatibility.md).
    fn resolve_expr_heads(
        &mut self,
        path: &str,
        expr: &mut sigil_model::expr::Expr,
        frames: &mut Vec<Vec<String>>,
    ) {
        use sigil_model::expr::Expr;
        match expr {
            Expr::SymbolReference { symbol, .. } => {
                self.resolve_head(path, symbol, frames);
            }
            Expr::ConstructorExpression { type_call, .. } => {
                let mut t = std::mem::take(type_call);
                self.resolve_type_ref(&format!("{path}.constructor"), &mut t, AnyType);
                *type_call = t;
            }
            _ => {}
        }
        if let Some(parameters) = expr.inline_parameters() {
            frames.push(parameters);
            expr.for_each_child_mut(&mut |child| {
                self.resolve_expr_heads(path, child, frames);
            });
            frames.pop();
        } else {
            expr.for_each_child_mut(&mut |child| {
                self.resolve_expr_heads(path, child, frames);
            });
        }
    }

    fn resolve_head(&mut self, path: &str, symbol: &str, frames: &[Vec<String>]) {
        if frames.iter().rev().any(|f| f.iter().any(|n| n == symbol)) {
            return; // locally bound
        }
        match lookup(self.res, self.ctx, self.index, false, symbol) {
            Some(id) => {
                if element_at(self.res, id).is_annotation() {
                    self.res.diagnostics.push(ResolutionDiagnostic::error(
                        "E0106",
                        &self.file.name,
                        path,
                        format!("'{}' is not a symbol", self.res.qname(id)),
                        None,
                    ));
                }
            }
            None => {
                // A dotted head is either a qualified name or a local
                // `Type.feature` chain (`Product.price`), which Xtext
                // resolves through the qualified names of nested elements.
                if symbol.contains('.') && self.resolve_qualified_head(symbol) {
                    return;
                }
                self.res.diagnostics.push(ResolutionDiagnostic::error(
                    "E0101",
                    &self.file.name,
                    path,
                    format!("unknown symbol '{symbol}'"),
                    None,
                ));
            }
        }
    }

    /// `Type.feature` / `Enum.value` chains that are not file-scope
    /// qualified names: every segment after the first must be a feature of
    /// the previous segment's type.
    fn resolve_qualified_head(&self, symbol: &str) -> bool {
        let mut segments = symbol.split('.');
        let first = segments.next().unwrap_or_default();
        let mut current = match lookup_simple(self.res, self.ctx, self.index, false, first) {
            Some(id) => id,
            None => return false,
        };
        for segment in segments {
            match attribute_type(self.res, current, segment) {
                Some(next) => match next {
                    Some(id) => current = ElementId(id),
                    // The feature exists; its type is not resolvable, so
                    // any further segments cannot be checked either.
                    None => return true,
                },
                None => return false,
            }
        }
        true
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
        // Anonymous root elements (reports) are not nameable symbols.
        if element.name().is_empty() {
            return true;
        }
        if table.contains_key(&fqn) {
            diagnostics.push(ResolutionDiagnostic::error(
                "E0104",
                &file.name,
                element.name(),
                format!("duplicate definition of '{fqn}'"),
                Some(element.span()),
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
    res.element(id)
}

/// The features visible on an instance of the type with flat id `id`:
/// a data type's own and inherited attributes, or an enumeration's own and
/// inherited values (both are `RosettaFeature`s).
fn feature_names(res: &Resolution, id: ElementId) -> Vec<String> {
    match element_at(res, id) {
        SemanticElement::Data(d) => {
            let mut names: Vec<String> = d.attributes.iter().map(|a| a.name.clone()).collect();
            if let Some(super_id) = d.super_type.as_ref().and_then(|t| t.resolved) {
                for name in feature_names(res, ElementId(super_id)) {
                    if !names.contains(&name) {
                        names.push(name);
                    }
                }
            }
            names
        }
        SemanticElement::Enumeration(e) => {
            let mut names: Vec<String> = e.values.iter().map(|v| v.name.clone()).collect();
            if let Some(super_id) = e.super_type.as_ref().and_then(|t| t.resolved) {
                for name in feature_names(res, ElementId(super_id)) {
                    if !names.contains(&name) {
                        names.push(name);
                    }
                }
            }
            names
        }
        _ => Vec::new(),
    }
}

/// The values of the enumeration with flat id `id`, including inherited.
fn enum_value_names(res: &Resolution, id: ElementId) -> Vec<String> {
    match element_at(res, id) {
        SemanticElement::Enumeration(e) => {
            let mut names: Vec<String> = e.values.iter().map(|v| v.name.clone()).collect();
            if let Some(super_id) = e.super_type.as_ref().and_then(|t| t.resolved) {
                for name in enum_value_names(res, ElementId(super_id)) {
                    if !names.contains(&name) {
                        names.push(name);
                    }
                }
            }
            names
        }
        _ => Vec::new(),
    }
}

/// Whether `name` is a feature (attribute or enum value) of the type with
/// flat id `id`.
fn attribute_exists(res: &Resolution, id: ElementId, name: &str) -> bool {
    feature_names(res, id).iter().any(|n| n == name)
}

/// The type of the attribute `name` on the data type `id`, including
/// inherited attributes: `Some(Some(target))` when found and typed,
/// `Some(None)` when found with an unresolvable type (an enum value, for
/// instance), `None` when absent.
fn attribute_type(res: &Resolution, id: ElementId, name: &str) -> Option<Option<usize>> {
    match element_at(res, id) {
        SemanticElement::Data(d) => {
            if let Some(attribute) = d.attributes.iter().find(|a| a.name == name) {
                return Some(attribute.type_ref.resolved);
            }
            if let Some(super_id) = d.super_type.as_ref().and_then(|t| t.resolved) {
                return attribute_type(res, ElementId(super_id), name);
            }
            None
        }
        SemanticElement::Enumeration(e) => {
            if e.values.iter().any(|v| v.name == name) {
                Some(None)
            } else if let Some(super_id) = e.super_type.as_ref().and_then(|t| t.resolved) {
                attribute_type(res, ElementId(super_id), name)
            } else {
                None
            }
        }
        _ => None,
    }
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
                                res.element_spans.get(next).copied(),
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

fn rule_references_json(
    rules: &[sigil_model::RuleReference],
    res: &Resolution,
) -> Vec<serde_json::Value> {
    rules
        .iter()
        .map(|r| {
            json!({
                "rule": r.resolved.map(|id| res.element_names[id].clone()).or(r.rule.clone()),
                "empty": r.empty,
            })
        })
        .collect()
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
                    "ruleReferences": rule_references_json(&a.rule_references, res),
                    "docReferences": a.doc_references,
                })
            })
            .collect(),
    )
}

fn conditions_json(
    conditions: &[sigil_model::Condition],
    res: &Resolution,
) -> Vec<serde_json::Value> {
    conditions
        .iter()
        .map(|c| {
            json!({
                "name": c.name,
                "definition": c.definition,
                "annotations": annotations_json(&c.annotations, res),
                "docReferences": c.doc_references,
                "expression": c.expression.to_json(),
            })
        })
        .collect()
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
            // `Choice.getConditions()` in the Ecore model returns a
            // hardcoded `one-of item` condition when none are declared;
            // mirror that derived behaviour here.
            "conditions": serde_json::Value::Array(
                conditions_json(&d.conditions, res).into_iter().chain(
                    if d.is_choice && d.conditions.is_empty() {
                        vec![json!({
                            "name": "Choice",
                            "definition": null,
                            "annotations": [],
                            "docReferences": [],
                            "expression": { "kind": "OneOf", "argument": { "kind": "ImplicitVariable" } },
                        })]
                    } else {
                        vec![]
                    },
                ).collect(),
            ),
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
        SemanticElement::Function(f) => json!({
            "kind": "Function",
            "name": f.name,
            "definition": f.definition,
            "superFunction": f.super_function.as_ref().map(|t| type_json(t, res)),
            "transform": serde_json::Value::Array(
                f.transform.iter().map(|t| json!({
                    "kind": t.kind.as_str(),
                    "ref": t.reference.as_ref().map(|r| json!({ "name": r })),
                })).collect(),
            ),
            "dispatch": f.dispatch.as_ref().map(|d| json!({
                "attribute": d.attribute,
                "enumeration": d.enumeration,
                "value": d.value,
            })),
            "annotations": annotations_json(&f.annotations, res),
            "docReferences": f.doc_references,
            "inputs": attributes_json(&f.inputs, res),
            "output": f.output.as_ref().map(|o| attributes_json(std::slice::from_ref(o), res)[0].clone()),
            "shortcuts": serde_json::Value::Array(
                f.shortcuts.iter().map(|s| json!({
                    "name": s.name,
                    "definition": s.definition,
                    "expression": s.expression.to_json(),
                })).collect(),
            ),
            "conditions": serde_json::Value::Array(conditions_json(&f.conditions, res)),
            "operations": serde_json::Value::Array(
                f.operations.iter().map(|o| json!({
                    "definition": o.definition,
                    "add": o.add,
                    "assignRoot": o.assign_root,
                    "path": o.path.iter().map(|p| p.feature.clone()).collect::<Vec<_>>(),
                    "expression": o.expression.to_json(),
                })).collect(),
            ),
            "postConditions": serde_json::Value::Array(conditions_json(&f.post_conditions, res)),
        }),
        SemanticElement::Rule(r) => json!({
            "kind": "Rule",
            "name": r.name,
            "definition": r.definition,
            "eligibility": r.eligibility,
            "input": r.input.as_ref().map(|t| type_json(t, res)),
            "expression": r.expression.to_json(),
        }),
        SemanticElement::Report(r) => json!({
            "kind": "Report",
            "regulatoryBody": {
                "body": named_ref_json(&r.regulatory.body, res),
                "corpora": r.regulatory.corpora.iter().map(|c| named_ref_json(c, res)).collect::<Vec<_>>(),
                "segments": r.regulatory.segments.iter().map(|s| json!({
                    "segment": named_ref_json(&s.segment, res),
                    "reference": s.reference,
                })).collect::<Vec<_>>(),
            },
            "eligibilityRules": r.eligibility_rules.iter().map(|e| named_ref_json(e, res)).collect::<Vec<_>>(),
            "inputType": type_json(&r.input_type, res),
            "reportType": named_ref_json(&r.report_type, res),
            "ruleSource": r.rule_source.as_ref().map(|s| named_ref_json(s, res)),
        }),
        SemanticElement::ExternalRuleSource(s) => json!({
            "kind": "ExternalRuleSource",
            "name": s.name,
            "superSource": s.super_source.as_ref().map(|t| type_json(t, res)),
            "externalClasses": serde_json::Value::Array(
                s.classes.iter().map(|c| json!({
                    "data": type_json(&c.data, res),
                    "attributes": c.attributes.iter().map(|a| json!({
                        "operator": if a.add { "+" } else { "-" },
                        "attribute": a.attribute,
                        "ruleReferences": rule_references_json(&a.rule_references, res),
                    })).collect::<Vec<_>>(),
                })).collect(),
            ),
        }),
        SemanticElement::Schema(s) => json!({
            "kind": "Schema",
            "name": s.name,
            "definition": s.definition,
            "format": s.format,
            "annotations": annotations_json(&s.annotations, res),
        }),
        SemanticElement::Body(b) => json!({
            "kind": "Body",
            "name": b.name,
            "bodyType": b.body_type,
            "definition": b.definition,
        }),
        SemanticElement::Corpus(c) => json!({
            "kind": "Corpus",
            "name": c.name,
            "corpusType": c.corpus_type,
            "displayName": c.display_name,
            "body": c.body,
            "definition": c.definition,
        }),
        SemanticElement::Segment(s) => json!({
            "kind": "Segment",
            "name": s.name,
        }),
        SemanticElement::MetaType(m) => json!({
            "kind": "MetaType",
            "name": m.name,
            "type": type_json(&m.type_ref, res),
        }),
    }
}

/// A by-name reference renders as the resolved FQN, falling back to the
/// written name when unresolved (matching the oracle dumper, which emits
/// FQNs of resolved targets).
fn named_ref_json(reference: &sigil_model::NamedRef, res: &Resolution) -> serde_json::Value {
    match reference.resolved {
        Some(id) => json!(res.element_names[id]),
        None => json!(reference.name),
    }
}

/// Canonical JSON rendering of a resolved model: the compatibility artifact
/// compared against the Java oracle.
///
/// `sources` maps file names to their text (user files; builtins may be
/// omitted). It is needed to resolve diagnostic byte spans into line/column
/// positions: diagnostics then carry a `span` with 1-based `line`/`column`
/// pairs plus a human-readable `location` (`file:line:col`). Columns count
/// plain characters (Unicode scalar values) — not LSP UTF-16 code units and
/// not bytes. Diagnostics without a span (or whose file has no source text
/// available) render `"span": null`.
pub fn canonical_json(resolution: &Resolution, sources: &[(String, String)]) -> serde_json::Value {
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
            let located = d.span.and_then(|span| {
                let text = &sources.iter().find(|(n, _)| *n == d.file)?.1;
                let (start_line, start_column) = sigil_diag::line_col_in(text, span.start);
                let (end_line, end_column) = sigil_diag::line_col_in(text, span.end);
                Some((start_line, start_column, end_line, end_column))
            });
            json!({
                "severity": d.severity.as_str(),
                "code": d.code,
                "message": d.message,
                "file": d.file,
                "path": d.path,
                "span": located.map(|(sl, sc, el, ec)| json!({
                    "start": { "line": sl, "column": sc },
                    "end": { "line": el, "column": ec },
                })),
                "location": located.map(|(sl, sc, _, _)| format!("{}:{}:{}", d.file, sl, sc)),
            })
        })
        .collect();

    json!({
        "format": "sigil-model/1",
        "files": files,
        "diagnostics": diagnostics,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sigil_diag::SourceFile;

    fn resolution_for(text: &str) -> Resolution {
        let source = SourceFile::new("test.rosetta", text);
        let (unit, syntax) = sigil_syntax::parse(&source);
        assert!(syntax.is_empty(), "{syntax:?}");
        resolve(vec![sigil_syntax::lower("test.rosetta", &unit.unwrap())])
    }

    fn codes(res: &Resolution) -> Vec<(&'static str, String)> {
        res.diagnostics
            .iter()
            .map(|d| (d.code, d.message.clone()))
            .collect()
    }

    const BASE: &str = "\
namespace test.scope

type Trade:
	price number (1..1)
	quantity number (1..1)
";

    /// The output attribute is visible in operation expressions.
    #[test]
    fn output_visible_in_operations() {
        let res = resolution_for(&format!(
            "{BASE}\nfunc F:\n\tinputs:\n\t\ttrade Trade (1..1)\n\toutput:\n\t\tresult number (1..1)\n\tset result:\n\t\tresult + trade -> price\n\tpost-condition P:\n\t\tresult exists\n"
        ));
        assert!(res.diagnostics.is_empty(), "{:?}", codes(&res));
    }

    /// Non-post conditions must not see the output; post-conditions must.
    #[test]
    fn output_hidden_in_non_post_conditions() {
        let res = resolution_for(&format!(
            "{BASE}\nfunc F:\n\tinputs:\n\t\ttrade Trade (1..1)\n\toutput:\n\t\tresult number (1..1)\n\tcondition C:\n\t\tresult exists\n"
        ));
        let (code, message) = codes(&res)
            .into_iter()
            .next()
            .expect("expected a diagnostic");
        assert_eq!(code, "E0101");
        assert!(message.contains("unknown symbol 'result'"), "{message}");
    }

    /// The dispatch head's attribute must name one of the inputs.
    #[test]
    fn dispatch_attribute_scope() {
        let ok = resolution_for(&format!(
            "{BASE}\nenum Kind:\n\tA\nfunc F(trade: Kind->A):\n\tinputs:\n\t\ttrade Trade (1..1)\n\toutput:\n\t\tresult number (1..1)\n"
        ));
        assert!(ok.diagnostics.is_empty(), "{:?}", codes(&ok));

        let bad = resolution_for(&format!(
            "{BASE}\nenum Kind:\n\tA\nfunc F(bogus: Kind->A):\n\tinputs:\n\t\ttrade Trade (1..1)\n\toutput:\n\t\tresult number (1..1)\n"
        ));
        let (code, message) = codes(&bad)
            .into_iter()
            .next()
            .expect("expected a diagnostic");
        assert_eq!(code, "E0107");
        assert!(message.contains("dispatch attribute 'bogus'"), "{message}");
    }

    /// Aliases see each other (but not themselves) and the inputs; the
    /// assign root accepts the output and aliases, never inputs.
    #[test]
    fn alias_chain_and_assign_root_scope() {
        let ok = resolution_for(&format!(
            "{BASE}\nfunc F:\n\tinputs:\n\t\ttrade Trade (1..1)\n\toutput:\n\t\tresult number (1..1)\n\talias a: trade -> price\n\talias b: a + 1.0\n\tset result:\n\t\tb\n\tset a:\n\t\tresult\n"
        ));
        assert!(ok.diagnostics.is_empty(), "{:?}", codes(&ok));

        let self_ref = resolution_for(&format!(
            "{BASE}\nfunc F:\n\tinputs:\n\t\ttrade Trade (1..1)\n\toutput:\n\t\tresult number (1..1)\n\talias a: a + 1.0\n\tset result:\n\t\ta\n"
        ));
        let (code, message) = codes(&self_ref)
            .into_iter()
            .next()
            .expect("expected a diagnostic");
        assert_eq!(code, "E0101");
        assert!(message.contains("unknown symbol 'a'"), "{message}");

        let input_root = resolution_for(&format!(
            "{BASE}\nfunc F:\n\tinputs:\n\t\ttrade Trade (1..1)\n\toutput:\n\t\tresult number (1..1)\n\tset trade:\n\t\t1.0\n"
        ));
        let (code, message) = codes(&input_root)
            .into_iter()
            .next()
            .expect("expected a diagnostic");
        assert_eq!(code, "E0101");
        assert!(message.contains("unknown symbol 'trade'"), "{message}");
    }

    /// An unknown type reference must produce an E0101 whose span covers the
    /// reference text and whose rendered position points at it.
    #[test]
    fn unknown_type_diagnostic_points_at_reference() {
        let text = "namespace test\n\ntype Foo:\n    attr Unknown (1..1)\n";
        let offset = text.find("Unknown").unwrap();
        let source = SourceFile::new("test.rosetta", text);
        let (unit, syntax) = sigil_syntax::parse(&source);
        assert!(syntax.is_empty(), "{syntax:?}");
        let resolution = resolve(vec![sigil_syntax::lower("test.rosetta", &unit.unwrap())]);

        let diag = resolution
            .diagnostics
            .iter()
            .find(|d| d.code == "E0101")
            .expect("expected an unknown-type diagnostic");
        assert_eq!(diag.file, "test.rosetta");
        let span = diag.span.expect("diagnostic must carry a span");
        // Identifier spans may absorb trailing whitespace (the parser's
        // terminals munch it), but the reference text must be covered and
        // the start must be exact.
        assert_eq!(span.start, offset);
        assert!(span.end >= offset + "Unknown".len());

        let json = canonical_json(
            &resolution,
            &[("test.rosetta".to_string(), text.to_string())],
        );
        let diag = json["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["code"] == "E0101")
            .unwrap();
        // `Unknown` starts on line 4, column 10 (1-based, character columns).
        assert_eq!(diag["span"]["start"]["line"], 4);
        assert_eq!(diag["span"]["start"]["column"], 10);
        assert_eq!(diag["span"]["end"]["line"], 4);
        assert_eq!(diag["location"], "test.rosetta:4:10");
    }
}

// ---------------------------------------------------------------------------
// LSP support — additive public API used by `sigil-lsp`.
//
// Everything below is new (Phase 3) and strictly additive: it only reads
// the resolution result through existing internals and does not modify any
// resolution behaviour above.
// ---------------------------------------------------------------------------

/// The source text of the built-in `.rosetta` files, in load order, for
/// tools that must map builtin byte spans back to source lines (LSP
/// hover/definition into the built-in library).
pub fn builtin_sources() -> Vec<(&'static str, &'static str)> {
    vec![
        ("basictypes.rosetta", BUILTIN_BASIC_TYPES),
        ("annotations.rosetta", BUILTIN_ANNOTATIONS),
    ]
}

/// A name visible from some file's scope, produced for completion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleName {
    /// The name as it can be written in the file: a simple name, or
    /// `alias.Name` for aliased wildcard imports.
    pub name: String,
    /// Fully-qualified name of the target element.
    pub fqn: String,
    /// Flat element id, as accepted by `Resolution::qname`.
    pub element: ElementId,
}

impl Resolution {
    fn file_index(&self, name: &str) -> Option<usize> {
        self.files.iter().position(|f| f.name == name)
    }

    fn find_by_fqn(&self, fqn: &str) -> Option<ElementId> {
        self.element_names
            .iter()
            .position(|n| n == fqn)
            .map(ElementId)
    }

    /// All type-like names visible from `file` (local elements, explicit
    /// imports, the file's own namespace, and the built-in library),
    /// deduplicated by visible name and sorted. Mirrors the scope chain
    /// used for resolution.
    pub fn visible_types(&self, file: &str) -> Vec<VisibleName> {
        self.visible(file, false)
    }

    /// All annotation names visible from `file`.
    pub fn visible_annotations(&self, file: &str) -> Vec<VisibleName> {
        self.visible(file, true)
    }

    fn visible(&self, file: &str, annotation: bool) -> Vec<VisibleName> {
        let Some(idx) = self.file_index(file) else {
            return Vec::new();
        };
        let ctx = build_context(self, idx);
        let mut out: Vec<VisibleName> = Vec::new();
        let mut seen = std::collections::BTreeSet::new();

        // Local file, own namespace, built-in library.
        for (file_idx, f) in self.files.iter().enumerate() {
            let offset = element_offset(self, file_idx);
            for (i, element) in f.elements.iter().enumerate() {
                let wanted = if annotation {
                    element.is_annotation()
                } else {
                    element.is_type()
                };
                if !wanted {
                    continue;
                }
                let visible =
                    file_idx == idx || f.namespace == ctx.namespace || f.namespace == LIB_NAMESPACE;
                if !visible {
                    continue;
                }
                let flat = offset + i;
                let simple = element.name().to_string();
                if seen.insert(simple.clone()) {
                    out.push(VisibleName {
                        name: simple.clone(),
                        fqn: self.element_names[flat].clone(),
                        element: ElementId(flat),
                    });
                }
            }
        }

        // Explicit imports, in declaration order.
        for rule in &ctx.imports {
            if let Some(target) = &rule.target {
                if let Some(id) = self.find_by_fqn(target) {
                    let wanted = if annotation {
                        element_at(self, id).is_annotation()
                    } else {
                        element_at(self, id).is_type()
                    };
                    if wanted {
                        if let Some(key) = &rule.key {
                            if seen.insert(key.clone()) {
                                out.push(VisibleName {
                                    name: key.clone(),
                                    fqn: target.clone(),
                                    element: id,
                                });
                            }
                        }
                    }
                }
            }
            if let Some(wildcard_ns) = &rule.wildcard_namespace {
                for (file_idx, f) in self.files.iter().enumerate() {
                    if f.namespace != *wildcard_ns {
                        continue;
                    }
                    let offset = element_offset(self, file_idx);
                    for (i, element) in f.elements.iter().enumerate() {
                        let wanted = if annotation {
                            element.is_annotation()
                        } else {
                            element.is_type()
                        };
                        if !wanted {
                            continue;
                        }
                        let flat = offset + i;
                        let visible_name = match &rule.key {
                            Some(alias) => format!("{alias}.{}", element.name()),
                            None => element.name().to_string(),
                        };
                        if seen.insert(visible_name.clone()) {
                            out.push(VisibleName {
                                name: visible_name,
                                fqn: self.element_names[flat].clone(),
                                element: ElementId(flat),
                            });
                        }
                    }
                }
            }
        }

        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// The attributes (name, definition) declared by the annotation with
    /// fully-qualified name `fqn`, for `[annotation attr` completion.
    pub fn annotation_attributes(&self, fqn: &str) -> Vec<(String, Option<String>)> {
        let Some(id) = self.find_by_fqn(fqn) else {
            return Vec::new();
        };
        match element_at(self, id) {
            SemanticElement::Annotation(a) => a
                .attributes
                .iter()
                .map(|attr| (attr.name.clone(), attr.definition.clone()))
                .collect(),
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod lsp_support_tests {
    use super::*;
    use sigil_diag::SourceFile;

    fn resolution() -> Resolution {
        let base = SourceFile::new(
            "base.rosetta",
            "namespace test\n\ntype Party:\n    name string (1..1)\n",
        );
        let main = SourceFile::new("main.rosetta", "namespace test\n\nimport test.Party\n\ntype Trade:\n    party Party (1..1)\n    [metadata id]\n");
        let mut models = Vec::new();
        for f in [&base, &main] {
            let (unit, diags) = sigil_syntax::parse(f);
            assert!(diags.is_empty(), "{diags:?}");
            models.push(sigil_syntax::lower(&f.name.clone(), &unit.unwrap()));
        }
        resolve(models)
    }

    #[test]
    fn visible_types_follow_the_scope_chain() {
        let res = resolution();
        let vis = res.visible_types("main.rosetta");
        let names: Vec<&str> = vis.iter().map(|v| v.name.as_str()).collect();
        // Local, same-namespace and builtin names are visible.
        assert!(names.contains(&"Trade"), "{names:?}");
        assert!(names.contains(&"Party"), "{names:?}");
        assert!(names.contains(&"int"), "{names:?}");
        // Annotations are not types.
        assert!(!names.contains(&"metadata"), "{names:?}");
    }

    #[test]
    fn visible_annotations_and_their_attributes() {
        let res = resolution();
        let vis = res.visible_annotations("main.rosetta");
        let names: Vec<&str> = vis.iter().map(|v| v.name.as_str()).collect();
        assert!(names.contains(&"metadata"), "{names:?}");
        assert!(!names.contains(&"Trade"), "{names:?}");

        let attrs = res.annotation_attributes("com.rosetta.model.metadata");
        let attr_names: Vec<&str> = attrs.iter().map(|(n, _)| n.as_str()).collect();
        assert!(attr_names.contains(&"id"), "{attr_names:?}");
        assert!(
            attrs.iter().all(|(_, d)| d.is_some()),
            "definitions present"
        );
    }
}
