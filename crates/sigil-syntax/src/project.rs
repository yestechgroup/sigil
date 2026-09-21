//! Multi-file project ingestion: deterministic `*.rosetta` discovery and
//! loading, so embedders (CLIs, editors, build tools) don't have to
//! reimplement file-system traversal.
//!
//! The traversal rules mirror the language server's workspace scan
//! (`sigil-lsp`), which is the behavioural source of truth:
//!
//! - only files whose name ends in `.rosetta` are collected;
//! - entries whose file name starts with `.` (hidden/dot directories) or
//!   whose file name is exactly `target` are skipped entirely (the
//!   whole subtree is ignored);
//! - recursion is depth-capped: the root counts as depth 0 and traversal
//!   stops once the depth exceeds 16, so pathological trees cannot loop
//!   forever (this also bounds symlink cycles);
//! - results are sorted by path and deduplicated, so the output never
//!   depends on the order the file system happens to return.

use std::path::{Path, PathBuf};

use sigil_diag::{Diagnostic, SourceFile};
use sigil_model::ModelFile;

/// Maximum directory depth for [`discover_rosetta_files`], mirrored from
/// the language server's workspace scan.
const MAX_DEPTH: usize = 16;

/// Recursively collect every `*.rosetta` file under each root, using the
/// rules documented in [the module docs](self). A root that is a regular
/// file is collected as-is when its name ends in `.rosetta`; a root that
/// cannot be read is skipped. The returned list is sorted and free of
/// duplicates, independent of directory iteration order.
pub fn discover_rosetta_files(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in roots {
        if root.is_dir() {
            collect(root, 0, &mut out);
        } else if root
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(".rosetta"))
        {
            out.push(root.clone());
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Load and parse a mixed set of files and directories into
/// [`ModelFile`]s, ready to be handed to `sigil_resolve::resolve`.
///
/// Each path is used as-is if it is a file, or expanded with
/// [`discover_rosetta_files`] if it is a directory. The combined file list
/// is sorted, so the returned models and diagnostics are in a
/// deterministic order regardless of the input order or of directory
/// iteration order.
///
/// Each parsed file's `SourceFile`/`ModelFile` name is its `file://` URI
/// (same convention as the language server's workspace analysis; no
/// percent-encoding). Files that cannot be read are skipped, mirroring the
/// language server's scan. The returned diagnostics are the syntax
/// diagnostics of all parsed files; resolution diagnostics come from
/// resolving the returned models.
pub fn load_user_files(paths: &[PathBuf]) -> (Vec<ModelFile>, Vec<Diagnostic>) {
    let mut files = Vec::new();
    for path in paths {
        if path.is_dir() {
            files.extend(discover_rosetta_files(std::slice::from_ref(path)));
        } else {
            files.push(path.clone());
        }
    }
    files.sort();
    files.dedup();

    let mut models = Vec::new();
    let mut diagnostics = Vec::new();
    for path in files {
        let Some(uri) = path_to_uri(&path) else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let source = SourceFile::new(uri.clone(), text);
        let (unit, diags) = crate::parse(&source);
        diagnostics.extend(diags);
        if let Some(unit) = unit {
            models.push(crate::lower(&source.name, &unit));
        }
    }
    (models, diagnostics)
}

/// Best-effort `file://` URI for a path. No percent-encoding: paths with
/// spaces or non-ASCII will not round-trip (documented limitation,
/// inherited from the language server).
pub fn path_to_uri(path: &Path) -> Option<String> {
    let text = path.to_str()?;
    if text.starts_with("file://") {
        return Some(text.to_string());
    }
    Some(format!("file://{text}"))
}

/// Walk one directory level. Mirrors the language server's private
/// `collect_rosetta_files`; entries are sorted per directory on top of the
/// final sort so the traversal order is deterministic even before
/// deduplication.
fn collect(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > MAX_DEPTH {
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
            collect(&path, depth + 1, out);
        } else if name.ends_with(".rosetta") {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    static NEXT_ID: AtomicU32 = AtomicU32::new(0);

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sigil-syntax-project-{}-{}-{name}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::SeqCst),
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn write(dir: &Path, rel: &str) -> PathBuf {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dirs");
        }
        std::fs::write(&path, "namespace test\n").expect("write file");
        path
    }

    fn cleanup(dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn discover_returns_sorted_paths() {
        let root = temp_dir("sorted");
        let expected = vec![
            write(&root, "a.rosetta"),
            write(&root, "b/c.rosetta"),
            write(&root, "b/d/e.rosetta"),
            write(&root, "z.rosetta"),
        ];
        let found = discover_rosetta_files(std::slice::from_ref(&root));
        assert_eq!(found, expected);
        assert_eq!(found, {
            let mut sorted = found.clone();
            sorted.sort();
            sorted
        });
        cleanup(&root);
    }

    #[test]
    fn discover_skips_dot_and_target_dirs() {
        let root = temp_dir("skips");
        write(&root, "kept.rosetta");
        write(&root, "nested/also_kept.rosetta");
        write(&root, ".hidden/dropped.rosetta");
        write(&root, "target/dropped.rosetta");
        write(&root, "nested/.hidden/dropped_too.rosetta");
        let found = discover_rosetta_files(std::slice::from_ref(&root));
        assert_eq!(
            found,
            vec![
                root.join("kept.rosetta"),
                root.join("nested/also_kept.rosetta")
            ]
        );
        cleanup(&root);
    }

    #[test]
    fn discover_is_deterministic_across_runs() {
        let root = temp_dir("deterministic");
        write(&root, "m/n/o/deep.rosetta");
        write(&root, "m/p/q/deep.rosetta");
        write(&root, "m/p/deep.rosetta");
        write(&root, "m/deep.rosetta");
        write(&root, "sibling.rosetta");
        let first = discover_rosetta_files(std::slice::from_ref(&root));
        for _ in 0..4 {
            let again = discover_rosetta_files(std::slice::from_ref(&root));
            assert_eq!(first, again);
        }
        cleanup(&root);
    }

    #[test]
    fn load_user_files_handles_mixed_files_and_dirs() {
        let root = temp_dir("mixed");
        let dir_a = root.join("a");
        let dir_b = root.join("b");
        std::fs::create_dir_all(&dir_a).unwrap();
        std::fs::create_dir_all(&dir_b).unwrap();
        let in_dir_a = write(&dir_a, "one.rosetta");
        let in_dir_b = write(&dir_b, "two.rosetta");
        let loose = write(&root, "loose.rosetta");

        let (models, diagnostics) = load_user_files(&[dir_b, loose.clone(), dir_a]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(
            models.iter().map(|m| m.name.clone()).collect::<Vec<_>>(),
            vec![
                path_to_uri(&in_dir_a).unwrap(),
                path_to_uri(&in_dir_b).unwrap(),
                path_to_uri(&loose).unwrap(),
            ]
        );
        cleanup(&root);
    }
}
