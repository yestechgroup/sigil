#!/usr/bin/env python3
"""Differential test: compare `sigil model` against the Java oracle dumper.

Usage:
    python3 scripts/oracle_compare.py <fixture.rosetta>...
    python3 scripts/oracle_compare.py --cdm            (alias: --cdm-legacy)

The explicitly-passed files are compared as one workspace in a single
invocation of each tool (CI passes exactly one file per call).

Multi-file groups: a directory tests/oracle/<group>.multi/ holding two or
more .rosetta files is one cross-file scoping fixture. Every invocation
that names a fixture under tests/oracle/ additionally sweeps all
tests/oracle/*.multi/ groups — each group's files are passed to BOTH tools
in a single invocation (sorted by path, so both sides see the same
argument order; both dump `files` in argument order, which makes the
per-group file sequence deterministic regardless of directory enumeration
order). Because CI invokes this script once per single-file fixture, the
multi-file groups are compared once per fixture in a CI run: redundant,
but it keeps the workflow unchanged.

CDM differential (--cdm): both tools run over the *whole* legacy CDM
corpus (CDM 6.7.0, fetched by `scripts/fetch-cdm.sh legacy`; issue #10
Phase 2) and the two whole-corpus dumps are compared per file. The corpus
is a real multi-file workspace, so both sides must see all files in one
invocation: the oracle resolves cross-references only against the files
passed to that JVM, and sigil's `resolve()` is whole-workspace by design.
(Single-file oracle runs leave cross-file references unresolved and would
produce false mismatches — the comparison therefore slices whole-corpus
dumps, it never runs the tools per file.) One JVM for the whole corpus is
also the measured sweet spot: ~8-9 s and ~3 MB of JSON for ~80 files;
"batching" means many files per -Dexec.args, never one JVM per file, and
smaller batches would silently unresolve cross-batch references.

Files that sigil cannot parse (9.56-era `[synonym ...]` syntax the
newer-main vendored grammar dropped — reverse drift, see
docs/compatibility.md) are discovered with `sigil check` and excluded from
the value comparison. Additional per-file exclusions live in CDM_SKIP_LIST
below with an explicit reason string each; a bare glob is deliberately
avoided. Both dumps are reduced by normalize_cdm() (shared normalize() plus
CDM-specific reductions for the documented 9.58.1-vs-main divergences) and
compared file by file; the run exits non-zero on any unskipped mismatch.

Requires: JDK 21 + Maven (oracle side), a built sigil binary
(`cargo build -p sigil-cli`), and the dumper compiled
(`mvn compile` in tools/oracle-dumper).
"""

import copy
import json
import os
import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
DUMPER = REPO / "tools/oracle-dumper"
FIXTURES = REPO / "tests/oracle"
MULTI_SUFFIX = ".multi"
SIGIL = REPO / "target/debug/sigil"

# ---- CDM differential (issue #10 Phase 2) ---------------------------------

CDM_VARIANT = "legacy"
CDM_SPARSE = "rosetta-source/src/main/rosetta"

# Per-file exclusions from the CDM value comparison, with an explicit,
# reviewable reason for each (mirroring the oracle-version caveats in
# docs/compatibility.md). Keys are corpus file names.
#
# Deliberately EMPTY today: the divergences anticipated by the issue-#10
# probe turned out to be either absent (CDM 6.7.0 contains no `expr as
# Type` casts at all) or reducible to a common shape by normalize_cdm()
# (bare `one-of` conditions, see below). The mechanism stays so that any
# future genuinely-unfixable divergence gets an enumerated entry instead of
# a glob. Rationale and the full drift inventory: docs/cdm-conformance.md.
CDM_SKIP_LIST: dict[str, str] = {}


def extract_json(text: str) -> dict:
    """Parse the first JSON document embedded in `text`, skipping any
    leading log/jvm noise (SLF4J wiring, stack traces, sigil diagnostics)."""
    i = text.find("{")
    while i != -1:
        try:
            return json.loads(text[i:])
        except json.JSONDecodeError:
            i = text.find("{", i + 1)
    raise ValueError(f"no JSON document found in output:\n{text[:2000]}")


def run_sigil(files: list[str]) -> dict:
    out = subprocess.run(
        [str(SIGIL), "model", *files],
        capture_output=True,
        text=True,
        check=True,
    )
    return json.loads(out.stdout)


def run_sigil_lenient(files: list[str]) -> dict:
    """Like run_sigil, but tolerates a non-zero exit code: over the CDM
    corpus sigil exits 1 on resolution diagnostics while still emitting the
    model JSON (only *parse* errors suppress the JSON)."""
    out = subprocess.run(
        [str(SIGIL), "model", *files],
        capture_output=True,
        text=True,
        check=False,
    )
    try:
        return extract_json(out.stdout)
    except ValueError:
        raise RuntimeError(f"sigil model produced no JSON:\n{out.stdout}\n{out.stderr}")


def run_oracle(files: list[str]) -> dict:
    result = subprocess.run(
        [
            "mvn",
            "-q",
            "compile",
            "exec:java",
            f"-Dexec.mainClass=sigil.ModelDumper",
            f"-Dexec.args={' '.join(files)}",
        ],
        cwd=DUMPER,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise RuntimeError(f"oracle dumper failed:\n{result.stdout}\n{result.stderr}")
    return extract_json(result.stdout)


def arg_value_text(value) -> str:
    """Render sigil's typed argument value the way the source writes it."""
    kind = value["kind"]
    if kind == "Int":
        return str(value["value"])
    if kind == "Number":
        return value["value"]
    if kind == "Str":
        return json.dumps(value["value"])
    if kind == "Bool":
        return "True" if value["value"] else "False"
    if kind == "Reference":
        return value["value"]
    raise ValueError(f"unknown argument kind {kind}")


def normalize_expression(node):
    """Normalize one expression-JSON node (recursive), centrally mapping the
    remaining Java-vs-sigil differences. The Java dumper already emits
    sigil's tag names (see ModelDumper.expressionJson for the EClass -> tag
    mapping), so only value renderings differ:
    * constructor type-call arguments: sigil types them
      ({"kind": "Int", "value": 30}), the oracle keeps source text;
      compare the source text (same rule as attribute type arguments).
    * cross-references (symbols, features, guards, ...): both sides keep the
      written source text, so nothing to map.
    """
    if isinstance(node, list):
        return [normalize_expression(item) for item in node]
    if not isinstance(node, dict):
        return node
    out = {}
    for key, value in node.items():
        if key == "arguments" and isinstance(value, list):
            out[key] = [
                {**argument, "value": (
                    arg_value_text(argument["value"])
                    if isinstance(argument.get("value"), dict)
                    else argument.get("value")
                )}
                for argument in value
            ]
        else:
            out[key] = normalize_expression(value)
    return out


def normalize_doc_refs(refs):
    """Reduce doc references to the shape both sides can agree on: the
    9.58.1 oracle keeps only `corpora` (resolved target names) and
    `reportedField`; sigil additionally serializes segments, rationales,
    provisions and (grammar-drift) `forPath`, and keeps corpora as
    written. The `body` link is dropped: 9.58.1's Xtext linker cannot
    resolve the bodyType-style reference real models write (`docReference
    CFTC ...` vs a body declared as `body CFTC CFTCBody`), so the oracle
    serializes `body: null` even for in-file declarations, and sigil does
    not model the resolved link yet either."""
    return [
        {
            "body": None,
            "corpora": ref.get("corpora", []),
            "reportedField": bool(
                ref.get("reportedField", ref.get("reported_field", False))
            ),
        }
        for ref in refs or []
    ]


def normalize(doc: dict) -> dict:
    """Reduce both sides to the comparable semantic shape."""
    files = [f for f in doc["files"] if not f["namespace"].startswith("com.rosetta.model")]
    for file in files:
        file["name"] = Path(file["name"]).name
        for element in file["elements"]:
            # The published oracle (9.58.1) predates annotations on type
            # aliases; drop the field on both sides.
            if element.get("kind") == "TypeAlias":
                element.pop("annotations", None)
            # The published oracle (9.58.1) predates `func extends` and
            # transform annotations; drop both fields on either side.
            if element.get("kind") == "Function":
                element.pop("superFunction", None)
                element.pop("transform", None)
                for condition in element.get("postConditions", []):
                    condition["expression"] = normalize_expression(condition["expression"])
            if element.get("kind") == "Rule" and element.get("input") is not None:
                element["input"] = normalize_expression(element["input"])
            if element.get("kind") == "Report":
                element["inputType"] = normalize_expression(element["inputType"])
            if "docReferences" in element:
                element["docReferences"] = normalize_doc_refs(element["docReferences"])
            if element.get("kind") == "ExternalRuleSource":
                # sigil keeps a full type ref for the super source and the
                # class data; the oracle only has the resolved target.
                element["superSource"] = resolve_ref(element.get("superSource"))
                for external_class in element.get("externalClasses", []):
                    external_class["data"] = resolve_ref(external_class.get("data"))
            # super types: reduce to the resolved target name on both sides.
            if element.get("superType") is not None and isinstance(
                element["superType"], dict
            ):
                element["superType"] = element["superType"].get("resolved")
            for attribute in element.get("attributes", []):
                # Argument values: sigil types them, the oracle keeps the
                # source text; compare the source text.
                for argument in attribute["type"].get("arguments", []):
                    if isinstance(argument.get("value"), dict):
                        argument["value"] = arg_value_text(argument["value"])
                if "docReferences" in attribute:
                    attribute["docReferences"] = normalize_doc_refs(attribute["docReferences"])
            for condition in element.get("conditions", []):
                condition["expression"] = normalize_expression(condition["expression"])
                if "docReferences" in condition:
                    condition["docReferences"] = normalize_doc_refs(condition["docReferences"])
            for value in element.get("values", []):
                if "docReferences" in value:
                    value["docReferences"] = normalize_doc_refs(value["docReferences"])
        for configuration in file.get("configurations", []):
            # sigil keeps a full type ref for the root; the oracle only has
            # the resolved target. (A string means normalize_cdm() already
            # reduced the root on both sides; pass it through.)
            root = configuration.get("root") or {}
            configuration["root"] = (
                root.get("resolved") if isinstance(root, dict) else root
            )
    return {"format": "normalized", "files": files}


def resolve_ref(value):
    """`{"name", "arguments", "resolved"}` -> the resolved FQN; a plain
    string (the oracle side) passes through."""
    if isinstance(value, dict):
        return value.get("resolved")
    return value


# ---- CDM-specific normalization -------------------------------------------
#
# The 9.58.1 oracle and sigil's newer-main grammar diverge on a handful of
# constructs that occur in CDM 6.7.0 but not in the tests/oracle fixtures.
# Each reduction below is a documented, shape-exact mapping — never a value
# guess. The inventory (with occurrence counts over the corpus) lives in
# docs/cdm-conformance.md; the grammar-drift background in
# docs/compatibility.md.

# Cross-reference fields whose value is written source text; when the 9.58.1
# dumper slices node text it can drag a preceding `//` line comment into the
# value ("// comment\n<indent>Symbol"). sigil never emits comments, so the
# value is the last line.
_REF_TEXT_FIELDS = ("symbol", "feature", "target", "enumeration")


def _is_bare_one_of(node: dict) -> bool:
    """sigil's parser shapes the parameter-less `one-of` condition form as
    Binary('one', '-', 'of'); both grammars actually define it as
    OneOfOperation over a derived implicit item (that is also exactly what
    the 9.58.1 oracle emits)."""
    def sym(side):
        return (
            isinstance(side, dict)
            and side.get("kind") == "SymbolReference"
            and side.get("args") == []
        )

    return (
        node.get("kind") == "Binary"
        and node.get("op") == "-"
        and sym(node.get("left"))
        and node["left"].get("symbol") == "one"
        and sym(node.get("right"))
        and node["right"].get("symbol") == "of"
    )


def _cdm_walk(node):
    """One recursive pass over a (raw) dump file (in place): rewrite bare
    `one-of`, strip comment pollution from reference texts, drop
    constructor pair keys, and reduce doc references everywhere
    (normalize() only covers elements/attributes/conditions/enum values —
    CDM functions carry them on `inputs` too)."""
    if isinstance(node, list):
        for i, item in enumerate(node):
            node[i] = _cdm_walk(item)
        return node
    if not isinstance(node, dict):
        return node
    if _is_bare_one_of(node):
        return {"kind": "OneOf", "argument": {"kind": "ImplicitVariable"}}
    for key, value in node.items():
        node[key] = _cdm_walk(value)
    for field in _REF_TEXT_FIELDS:
        value = node.get(field)
        if isinstance(value, str) and "\n" in value:
            node[field] = value.splitlines()[-1].strip()
    if node.get("kind") == "Constructor":
        # The 9.58.1 linker leaves constructor pair keys (crossrefs to the
        # constructed type's attributes) unresolved across files; sigil
        # keeps the written name. Compare the pair values only.
        for pair in node.get("values", []):
            if isinstance(pair, dict):
                pair.pop("key", None)
    if isinstance(node.get("docReferences"), list):
        node["docReferences"] = normalize_doc_refs(node["docReferences"])
    return node


def normalize_cdm(doc: dict) -> dict:
    """normalize() plus the CDM-specific reductions. Applied to per-file
    slices of both whole-corpus dumps; the fixture path never runs this."""
    doc = copy.deepcopy(doc)
    for file in doc["files"]:
        file["name"] = Path(file["name"]).name
        # Configuration key drift: sigil serializes `q_type`, the oracle
        # `qType`. The root is reduced to the bare target name on both
        # sides (sigil does not serialize the resolved FQN; the oracle's
        # FQN last segment is exactly that name).
        for configuration in file.get("configurations", []):
            if "q_type" in configuration:
                configuration["qType"] = configuration.pop("q_type")
            root = configuration.get("root")
            if isinstance(root, dict):
                full = root.get("resolved") or root.get("name")
                configuration["root"] = full.rsplit(".", 1)[-1] if full else None
        # The 9.58.1 linker leaves operation path segments unresolved once
        # they leave the function's own file (and occasionally even the
        # first segment); sigil resolves them. assignRoot *does* resolve on
        # both sides and stays compared; only the segment chain is dropped.
        for element in file["elements"]:
            if element.get("kind") == "Function":
                for operation in element.get("operations", []):
                    operation.pop("path", None)
            # Annotation qualifier paths: sigil serializes the structured
            # path ({data, attributes}), the 9.58.1 oracle only a Path
            # marker (its qualifier path model differs and is outside the
            # comparison scope — same rule as the fixtures' annotation
            # docstring in the dumper).
            for attribute in element.get("attributes", []):
                for annotation in attribute.get("annotations", []):
                    for qualifier in annotation.get("qualifiers", []):
                        value = qualifier.get("value")
                        if isinstance(value, dict) and value.get("kind") == "Path":
                            qualifier["value"] = {"kind": "Path"}
        _cdm_walk(file)
    return normalize(doc)


def leaf_paths(node, prefix=""):
    if isinstance(node, dict):
        for key, value in node.items():
            yield from leaf_paths(value, f"{prefix}.{key}")
    elif isinstance(node, list):
        for i, value in enumerate(node):
            yield from leaf_paths(value, f"{prefix}[{i}]")
    else:
        yield prefix, node


def diff_leaves(a, b, limit=8):
    pa = dict(leaf_paths(a))
    pb = dict(leaf_paths(b))
    diffs = []
    for key in sorted(set(pa) | set(pb)):
        va, vb = pa.get(key, "<absent>"), pb.get(key, "<absent>")
        if va != vb:
            diffs.append((key, va, vb))
            if len(diffs) >= limit:
                break
    return diffs


# ---- CDM runner ------------------------------------------------------------


def cdm_corpus_dir() -> Path:
    cache_root = Path(os.environ.get("SIGIL_CDM_CACHE", REPO / "target/cdm-corpus"))
    return cache_root / CDM_VARIANT / CDM_SPARSE


def ensure_cdm_corpus() -> Path:
    """Fetch the legacy corpus if absent (idempotent), else error with
    instructions."""
    corpus = cdm_corpus_dir()
    if not list(corpus.glob("*.rosetta")):
        print(f"oracle_compare: legacy CDM corpus not found at {corpus}; fetching",
              file=sys.stderr)
        subprocess.run(
            [str(REPO / "scripts/fetch-cdm.sh"), CDM_VARIANT],
            check=True,
        )
    if not list(corpus.glob("*.rosetta")):
        print(
            "oracle_compare: no .rosetta files in "
            f"{corpus}\n  fetch manually: scripts/fetch-cdm.sh {CDM_VARIANT}",
            file=sys.stderr,
        )
        sys.exit(2)
    return corpus


def cdm_compare() -> bool:
    corpus = ensure_cdm_corpus()
    all_files = sorted(str(p) for p in corpus.glob("*.rosetta"))

    # Discover what sigil cannot parse (reverse drift: 9.56-era synonym
    # syntax dropped from the newer-main vendored grammar — never "fix"
    # sigil for these, see docs/compatibility.md).
    check = subprocess.run(
        [str(SIGIL), "check", *all_files], capture_output=True, text=True
    )
    parse_failures = {}
    for line in check.stdout.splitlines():
        match = re.match(r"^(.+?\.rosetta):\d+:\d+: error:", line)
        if match:
            parse_failures.setdefault(Path(match.group(1)).name, line)
    skipped = {name: CDM_SKIP_LIST[name] for name in CDM_SKIP_LIST
               if f"{corpus}/{name}" in all_files}
    compared = [
        f for f in all_files
        if Path(f).name not in parse_failures and Path(f).name not in skipped
    ]

    # Both sides see the whole comparable workspace in ONE invocation each
    # (cross-file resolution; see module docstring), then per-file slices
    # are compared.
    sigil_doc = run_sigil_lenient(compared)
    oracle_doc = run_oracle(compared)
    sigil_by_name = {f["name"]: f for f in normalize_cdm(sigil_doc)["files"]}
    oracle_by_name = {f["name"]: f for f in normalize_cdm(oracle_doc)["files"]}

    missing = sorted(set(sigil_by_name) ^ set(oracle_by_name))
    matched, mismatched = [], []
    for name in sorted(set(sigil_by_name) & set(oracle_by_name)):
        if sigil_by_name[name] == oracle_by_name[name]:
            matched.append(name)
        else:
            mismatched.append(name)

    print(f"CDM differential ({CDM_VARIANT} corpus, oracle 9.58.1) over {corpus}")
    print(f"  corpus files: {len(all_files)}")
    print(f"  sigil parse failures (reverse drift, 9.56 synonym syntax): "
          f"{len(parse_failures)}")
    for name in sorted(parse_failures):
        print(f"    - {name}")
    print(f"  excluded via skip list: {len(skipped)}")
    for name, reason in sorted(skipped.items()):
        print(f"    - {name}: {reason}")
    print(f"  compared: {len(compared)}  matched: {len(matched)}  "
          f"mismatched: {len(mismatched)}")
    for name in missing:
        print(f"  MISSING SLICE: {name} (only on one side)")
    for name in mismatched:
        print(f"  MISMATCH {name}:")
        for key, va, vb in diff_leaves(sigil_by_name[name], oracle_by_name[name]):
            print(f"    {key}\n      sigil : {str(va)[:160]}\n      oracle: {str(vb)[:160]}")
    if mismatched or missing:
        print(f"FAIL: {len(mismatched)} mismatched, {len(missing)} missing slice(s)")
        return False
    print(f"OK: sigil matches the Java oracle on all {len(matched)} compared "
          f"CDM files (after documented normalization)")
    return True


# ---- fixture runner --------------------------------------------------------


def multi_groups() -> list[list[str]]:
    """Every tests/oracle/<group>.multi/ directory as a sorted file list."""
    groups = []
    for group_dir in sorted(FIXTURES.glob(f"*{MULTI_SUFFIX}")):
        if group_dir.is_dir():
            files = sorted(str(p) for p in group_dir.glob("*.rosetta"))
            if files:
                groups.append(files)
    return groups


def compare(files: list[str]) -> bool:
    sigil_doc = normalize(run_sigil(files))
    oracle_doc = normalize(run_oracle(files))
    if sigil_doc == oracle_doc:
        names = ", ".join(Path(f).name for f in files)
        print(f"OK: sigil matches the Java oracle on {len(files)} file(s): {names}")
        return True
    print("MISMATCH")
    print("--- sigil ---")
    print(json.dumps(sigil_doc, indent=2, sort_keys=True))
    print("--- oracle ---")
    print(json.dumps(oracle_doc, indent=2, sort_keys=True))
    return False


def main() -> int:
    args = sys.argv[1:]
    if "--cdm" in args or "--cdm-legacy" in args:
        if len(args) != 1:
            print("usage: oracle_compare.py --cdm (no other arguments)", file=sys.stderr)
            return 2
        return 0 if cdm_compare() else 1
    files = [str(Path(f).resolve()) for f in args]
    if not files:
        print(__doc__)
        return 2
    ok = compare(files)
    # CI (which always passes tests/oracle fixtures) also sweeps the
    # multi-file groups on every invocation; ad-hoc runs outside
    # tests/oracle/ stay single-file.
    if any(Path(f).is_relative_to(FIXTURES) for f in files):
        for group in multi_groups():
            ok = compare(group) and ok
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
