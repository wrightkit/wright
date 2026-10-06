"""Seeded defect classes for generated benchmark scenarios (SPEC-534).

A defect class knows three things about an injected defect: where it can go
(`sites`), how to put it there (`apply`), and how to verify a fix behaviorally
(the checks it returns). Generated instances are ordinary scenario directories;
`agent_bench.py validate` calibrates them like hand-written ones, and the
checks never compare against a fixed expected solution.
"""

from __future__ import annotations

import json
import re
from dataclasses import dataclass


@dataclass(frozen=True)
class Site:
    """One place a defect can be injected. Ordered: `sites()` returns a stable
    list so a seeded pick is deterministic."""

    file: str                           # project-relative path the injection edits
    symbol: str                         # the name the calls keep referencing
    calls: tuple[tuple[str, int], ...]  # (file, 1-based line) of each call left dangling


@dataclass
class Applied:
    """One injected instance: the defective tree, the overlays the suite
    calibrates with, and the checks generated for it."""

    seed_files: dict[str, str]            # every project file, defective
    reference: dict[str, str]             # overlay restoring the pristine files it replaced
    negatives: dict[str, dict]            # overlay name -> {"files": {...}, "fails": [check ids]}
    checks: list[dict]
    writable: list[str]
    prompt: str


class Defect:
    id = ""
    family = ""
    language = ""

    def sites(self, files: dict[str, str]) -> list[Site]:
        raise NotImplementedError

    def apply(self, files: dict[str, str], site: Site, compiled: str) -> Applied:
        raise NotImplementedError


def _rules(compiled: str) -> list[str]:
    return [rf'rule \("{re.escape(name)}"\)' for name in re.findall(r'rule \("([^"]+)"\)', compiled)]


def _subroutine_body(compiled: str, symbol: str) -> list[str]:
    """Escaped action lines of the subroutine `<symbol>`: its `Subroutine; <symbol>;`
    event followed by the actions block, in either compiler's rule naming."""
    match = re.search(rf"Subroutine;\s*{re.escape(symbol)};.*?actions \{{(.*?)}}", compiled, re.S)
    if not match:
        return []
    return [re.escape(line.strip()) for line in match.group(1).splitlines() if line.strip().endswith(";")]


class RenamedCallable(Defect):
    """A `def` was renamed without updating its callers: every call site becomes an
    `unknown-action` error. Family `diagnostics` — the agent has to find and fix a
    batch of diagnostics across files. Both correct fixes pass: restore the name
    under `def`, or retarget the calls to the new name; only deleting the calls or
    the definition fails the preserved-behavior probes."""

    id = "renamed-callable"
    family = "diagnostics"
    language = "opy"

    def sites(self, files: dict[str, str]) -> list[Site]:
        sites = []
        for def_file in sorted(files):
            if not def_file.endswith(".opy"):
                continue
            for match in re.finditer(r"(?m)^def (\w+)\(", files[def_file]):
                symbol = match.group(1)
                calls = tuple(
                    (name, number)
                    for name in sorted(files)
                    for number, line in enumerate(files[name].splitlines(), 1)
                    if name.endswith(".opy")
                    and not line.lstrip().startswith(("#", "def "))
                    and re.search(rf"(?<![\w\"]){re.escape(symbol)}\(", line)
                )
                if calls:
                    sites.append(Site(def_file, symbol, calls))
        return sites

    def apply(self, files: dict[str, str], site: Site, compiled: str) -> Applied:
        broken = f"{site.symbol}_v2"
        defective = dict(files)
        defective[site.file] = defective[site.file].replace(f"def {site.symbol}(", f"def {broken}(", 1)
        if defective[site.file] == files[site.file]:
            raise SystemExit(f"{self.id}: site {site.file}:{site.symbol} did not inject")

        without_def = dict(defective)
        without_def[site.file] = self._drop_block(defective[site.file], f"def {broken}(")
        for call_file, _ in site.calls:
            without_def[call_file] = "\n".join(
                line for line in without_def[call_file].split("\n")
                if not re.search(rf"(?<![\w\"]){re.escape(site.symbol)}\(", line) or line.lstrip().startswith("#")
            )
        without_def = {name: (text if text.endswith("\n") else text + "\n") for name, text in without_def.items()}
        call_files = {name for name, _ in site.calls}
        negatives = {"deleted-def": {"files": {n: without_def[n] for n in {site.file, *call_files}},
                                     "fails": ["body-preserved", "callable-still-called", "missing-name", "rules-preserved"]}}

        rules, body = _rules(compiled), _subroutine_body(compiled, site.symbol)
        if not rules or not body:
            raise SystemExit(f"{self.id}: compiled seed lacks probes for site {site.symbol}")
        checks = [
            {"id": "valid-project", "kind": "check", "layer": "opy-rs"},
            {"id": "upstream-valid", "kind": "oracle", "layer": "agent"},
            {"id": "compiles", "kind": "wright-compile", "layer": "opy-rs"},
            {"id": "callable-still-called", "kind": "compiled-contains", "layer": "agent",
             "all": [rf"Call Subroutine\({re.escape(site.symbol)}\w*\)"]},
            {"id": "rules-preserved", "kind": "compiled-contains", "layer": "agent", "all": rules},
            {"id": "body-preserved", "kind": "compiled-contains", "layer": "agent", "all": body},
            {"id": "missing-name", "kind": "answer", "layer": "agent", "key": "missing", "expected": site.symbol},
        ]
        prompt = (
            f"The project fails `wright check` with an `unknown-action` diagnostic, and more may surface as you fix them.\n\n"
            f"Find every diagnostic and fix the project with the smallest change that keeps the intended behavior. "
            f"Record the name the diagnostics report as undefined as `{{\"missing\": \"<name>\"}}` in `answer.json`. "
            f"The project must validate cleanly when you are done.\n"
        )
        return Applied(
            seed_files=defective,
            reference={site.file: files[site.file], "answer.json": json.dumps({"missing": site.symbol}) + "\n"},
            negatives=negatives,
            checks=checks,
            writable=sorted({*files, "answer.json"}),
            prompt=prompt,
        )

    @staticmethod
    def _drop_block(text: str, header: str) -> str:
        """Remove a top-level `def` block: the header line and its indented body."""
        lines = text.split("\n")
        start = next(i for i, line in enumerate(lines) if line.startswith(header))
        end = start + 1
        while end < len(lines) and (lines[end].startswith((" ", "\t")) or not lines[end].strip()):
            end += 1
        return "\n".join(lines[:start] + lines[end:])


REGISTRY = {defect.id: defect for defect in (RenamedCallable(),)}
