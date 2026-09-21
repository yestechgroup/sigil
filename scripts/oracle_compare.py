#!/usr/bin/env python3
"""Differential test: compare `sigil model` against the Java oracle dumper.

Usage:
    python3 scripts/oracle_compare.py <fixture.rosetta>...

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

Requires: JDK 21 + Maven (oracle side), a built sigil binary
(`cargo build -p sigil-cli`), and the dumper compiled
(`mvn compile` in tools/oracle-dumper).
"""

import json
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
DUMPER = REPO / "tools/oracle-dumper"
FIXTURES = REPO / "tests/oracle"
MULTI_SUFFIX = ".multi"


def run_sigil(files: list[str]) -> dict:
    out = subprocess.run(
        [str(REPO / "target/debug/sigil"), "model", *files],
        capture_output=True,
        text=True,
        check=True,
    )
    return json.loads(out.stdout)


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
    stdout = result.stdout[result.stdout.index("{") :]
    return json.loads(stdout)


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
            # the resolved target.
            root = configuration.get("root") or {}
            configuration["root"] = root.get("resolved")
    return {"format": "normalized", "files": files}


def resolve_ref(value):
    """`{"name", "arguments", "resolved"}` -> the resolved FQN; a plain
    string (the oracle side) passes through."""
    if isinstance(value, dict):
        return value.get("resolved")
    return value


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
    files = [str(Path(f).resolve()) for f in sys.argv[1:]]
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
