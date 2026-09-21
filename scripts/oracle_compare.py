#!/usr/bin/env python3
"""Differential test: compare `sigil model` against the Java oracle dumper.

Usage:
    python3 scripts/oracle_compare.py <fixture.rosetta>...

Both tools run on the same files; outputs are normalized (key order,
sigil-only fields, known oracle-version differences) and compared.

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
            for condition in element.get("conditions", []):
                condition["expression"] = normalize_expression(condition["expression"])
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


def main() -> int:
    files = [str(Path(f).resolve()) for f in sys.argv[1:]]
    if not files:
        print(__doc__)
        return 2
    sigil_doc = normalize(run_sigil(files))
    oracle_doc = normalize(run_oracle(files))
    if sigil_doc == oracle_doc:
        print(f"OK: sigil matches the Java oracle on {len(files)} file(s)")
        return 0
    print("MISMATCH")
    print("--- sigil ---")
    print(json.dumps(sigil_doc, indent=2, sort_keys=True))
    print("--- oracle ---")
    print(json.dumps(oracle_doc, indent=2, sort_keys=True))
    return 1


if __name__ == "__main__":
    sys.exit(main())
