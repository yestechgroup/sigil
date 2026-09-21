//! Acceptance test for issue #7: `Resolution::elements_of_kind` must agree
//! with an independent manual scan over `Resolution.files` for every
//! element kind, across the whole conformance corpus.

use std::path::{Path, PathBuf};

use sigil_diag::SourceFile;
use sigil_model::{ModelFile, SemanticElement};
use sigil_resolve::{resolve, ElementId, ElementKind, Resolution};

fn conformance_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/conformance")
        .canonicalize()
        .expect("tests/conformance must exist")
}

fn case_dirs() -> Vec<PathBuf> {
    let root = conformance_root();
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&root)
        .expect("conformance root")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .flat_map(|group| {
            std::fs::read_dir(&group)
                .expect("case group")
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.is_dir())
        })
        .collect();
    dirs.sort();
    dirs
}

fn resolve_case(dir: &Path) -> Resolution {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("case dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|e| e == "rosetta").unwrap_or(false))
        .collect();
    paths.sort();

    let models: Vec<ModelFile> = paths
        .iter()
        .filter_map(|path| {
            let text = std::fs::read_to_string(path).expect("read case file");
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            let (unit, _) = sigil_syntax::parse(&SourceFile::new(name.clone(), text));
            unit.map(|unit| sigil_syntax::lower(&name, &unit))
        })
        .collect();
    resolve(models)
}

// ---- independent reference implementation --------------------------------
//
// A private mirror of the kind lattice with its own exhaustive match,
// deliberately not routed through `ElementKind::of`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScanKind {
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

const ALL_SCAN_KINDS: &[ScanKind] = &[
    ScanKind::Data,
    ScanKind::Enumeration,
    ScanKind::Annotation,
    ScanKind::TypeAlias,
    ScanKind::BasicType,
    ScanKind::RecordType,
    ScanKind::LibraryFunction,
    ScanKind::Function,
    ScanKind::Rule,
    ScanKind::Report,
    ScanKind::ExternalRuleSource,
    ScanKind::Schema,
    ScanKind::Body,
    ScanKind::Corpus,
    ScanKind::Segment,
    ScanKind::MetaType,
];

fn scan_kind(element: &SemanticElement) -> ScanKind {
    match element {
        SemanticElement::Data(_) => ScanKind::Data,
        SemanticElement::Enumeration(_) => ScanKind::Enumeration,
        SemanticElement::Annotation(_) => ScanKind::Annotation,
        SemanticElement::TypeAlias(_) => ScanKind::TypeAlias,
        SemanticElement::BasicType(_) => ScanKind::BasicType,
        SemanticElement::RecordType(_) => ScanKind::RecordType,
        SemanticElement::LibraryFunction(_) => ScanKind::LibraryFunction,
        SemanticElement::Function(_) => ScanKind::Function,
        SemanticElement::Rule(_) => ScanKind::Rule,
        SemanticElement::Report(_) => ScanKind::Report,
        SemanticElement::ExternalRuleSource(_) => ScanKind::ExternalRuleSource,
        SemanticElement::Schema(_) => ScanKind::Schema,
        SemanticElement::Body(_) => ScanKind::Body,
        SemanticElement::Corpus(_) => ScanKind::Corpus,
        SemanticElement::Segment(_) => ScanKind::Segment,
        SemanticElement::MetaType(_) => ScanKind::MetaType,
    }
}

fn to_element_kind(kind: ScanKind) -> ElementKind {
    match kind {
        ScanKind::Data => ElementKind::Data,
        ScanKind::Enumeration => ElementKind::Enumeration,
        ScanKind::Annotation => ElementKind::Annotation,
        ScanKind::TypeAlias => ElementKind::TypeAlias,
        ScanKind::BasicType => ElementKind::BasicType,
        ScanKind::RecordType => ElementKind::RecordType,
        ScanKind::LibraryFunction => ElementKind::LibraryFunction,
        ScanKind::Function => ElementKind::Function,
        ScanKind::Rule => ElementKind::Rule,
        ScanKind::Report => ElementKind::Report,
        ScanKind::ExternalRuleSource => ElementKind::ExternalRuleSource,
        ScanKind::Schema => ElementKind::Schema,
        ScanKind::Body => ElementKind::Body,
        ScanKind::Corpus => ElementKind::Corpus,
        ScanKind::Segment => ElementKind::Segment,
        ScanKind::MetaType => ElementKind::MetaType,
    }
}

/// Manual scan: count elements of `kind` by walking `resolution.files`
/// directly (the pre-#7 way embedders had to do it).
fn scan_count(res: &Resolution, kind: ScanKind) -> usize {
    let mut count = 0;
    for file in &res.files {
        for element in &file.elements {
            if scan_kind(element) == kind {
                count += 1;
            }
        }
    }
    count
}

#[test]
fn elements_of_kind_matches_manual_scan_over_corpus() {
    let cases = case_dirs();
    assert!(!cases.is_empty(), "no conformance cases found");
    let mut checked = 0usize;

    for case in &cases {
        let res = resolve_case(case);
        let total: usize = res.files.iter().map(|f| f.elements.len()).sum();
        assert_eq!(
            total,
            res.element_names.len(),
            "{}: element_names out of sync with files",
            case.display()
        );

        let mut sum = 0usize;
        for kind in ALL_SCAN_KINDS {
            let api = res.elements_of_kind(to_element_kind(*kind));
            assert_eq!(
                api.len(),
                scan_count(&res, *kind),
                "{}: kind {:?} count mismatch",
                case.display(),
                kind
            );
            // Ordered by ElementId, i.e. declaration order.
            for window in api.windows(2) {
                assert!(
                    window[0].0 .0 < window[1].0 .0,
                    "{}: kind {:?} not ordered by ElementId",
                    case.display(),
                    kind
                );
            }
            sum += api.len();
        }
        // The two exhaustive kind lists must together cover every element.
        assert_eq!(
            sum,
            total,
            "{}: kind lists are not exhaustive",
            case.display()
        );
        checked += 1;
    }
    assert!(checked > 0);
}

#[test]
fn kind_of_matches_manual_scan_per_element() {
    for case in &case_dirs() {
        let res = resolve_case(case);
        let mut flat = 0usize;
        for file in &res.files {
            for element in &file.elements {
                assert_eq!(
                    to_element_kind(scan_kind(element)),
                    res.kind_of(ElementId(flat)),
                    "{}: kind_of(ElementId({flat})) mismatch for {}",
                    case.display(),
                    res.qname(ElementId(flat))
                );
                flat += 1;
            }
        }
        assert_eq!(flat, res.element_names.len());
    }
}

#[test]
fn find_by_name_round_trips_corpus_qnames() {
    for case in &case_dirs() {
        let res = resolve_case(case);
        for id in 0..res.element_names.len() {
            let qname = res.qname(ElementId(id)).to_string();
            let element = res.element(ElementId(id));
            if element.name().is_empty() {
                continue; // anonymous reports are not nameable
            }
            let (found_id, found) = res.find_by_name(&qname).unwrap_or_else(|| {
                panic!("{}: find_by_name({qname}) returned None", case.display())
            });
            assert_eq!(
                res.qname(found_id),
                qname,
                "{}: find_by_name({qname}) returned a different qname",
                case.display()
            );
            // The same FQN can legally exist in two kind namespaces (a
            // `typeAlias` and an `annotation` with one name, say); only the
            // first is findable by name then.
            let unique = res.element_names.iter().filter(|n| **n == qname).count() == 1;
            if unique {
                assert_eq!(
                    found_id,
                    ElementId(id),
                    "{}: find_by_name({qname}) returned the wrong element",
                    case.display()
                );
                assert_eq!(
                    ElementKind::of(found),
                    res.kind_of(ElementId(id)),
                    "{}: find_by_name({qname}) kind mismatch",
                    case.display()
                );
            }
        }
    }
}

#[test]
fn find_by_name_falls_back_to_simple_name() {
    let res = resolve_case(&conformance_root().join("model/basic"));
    let any = res
        .user_files()
        .first()
        .and_then(|f| f.elements.first())
        .expect("model/basic declares at least one element");
    let name = any.name().to_string();
    assert!(!name.is_empty());
    let (id, found) = res
        .find_by_name(&name)
        .unwrap_or_else(|| panic!("find_by_name({name}) returned None"));
    assert_eq!(found.name(), name);
    assert!(res.qname(id).ends_with(&format!(".{name}")));
    assert_eq!(res.kind_of(id), ElementKind::of(any));
}
