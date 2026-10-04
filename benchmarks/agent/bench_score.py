"""Wright Agent Score (#467): per-language tracks, scenario macro-average, clustered bootstrap interval, and the score card."""

from __future__ import annotations

import json
import random
from collections import defaultdict
from math import comb
from pathlib import Path

CONTRACT = "wright-agent-score/v1"
CANONICAL = "wright+wright-skill/none/off"
MIN_SCENARIOS = 8
BOOTSTRAP_DRAWS = 10_000
BOOTSTRAP_SEED = 467
METHOD = f"two-stage percentile bootstrap (scenarios, then trials within a scenario), {BOOTSTRAP_DRAWS} draws, seed {BOOTSTRAP_SEED}"
VALID = ("completed", "timeout")  # a timeout is the agent's own outcome and counts; every other status is excluded and reported separately
TRACKS = {"workshop": "Wright Workshop Agent Score", "opy": "Wright OPY Agent Score"}


def cluster_interval(outcomes: dict[str, list[int]], draws: int = BOOTSTRAP_DRAWS, seed: int = BOOTSTRAP_SEED) -> tuple[float, float]:
    """95% interval of the macro-averaged usable rate, resampling scenarios first and trials within each chosen scenario."""
    names = sorted(outcomes)
    rng = random.Random(seed)
    means = []
    for _ in range(draws):
        total = 0.0
        for name in (rng.choice(names) for _ in names):
            trials = outcomes[name]
            total += sum(rng.choice(trials) for _ in trials) / len(trials)
        means.append(100 * total / len(names))
    means.sort()
    return means[int(0.025 * draws)], means[int(0.975 * draws) - 1]


def pass_power_k(usable: int, valid: int, k: int) -> float:
    """Probability that k distinct valid trials of a scenario are all usable."""
    return comb(usable, k) / comb(valid, k) if valid >= k else 0.0


def read_enforcement(run: dict) -> str | None:
    """The file-read policy in one word: 'allow-list' is recorded with its hidden/allowed paths, which differ per trial."""
    enforcement = run.get("fileReadEnforcement")
    return enforcement.get("mode") if isinstance(enforcement, dict) else enforcement


def identity_of(run: dict) -> dict:
    env = run["environment"]
    info = run.get("agentInfo") or {}
    return {
        "wrightSha256": env.get("wrightSha256"), "wright": env.get("wright"),
        "skills": {name: s["sha256"] for name, s in sorted((env.get("skills") or {}).items())},
        "suite": (env.get("suite") or {}).get("hash"),
        "agent": run["agent"]["id"], "model": info.get("model"), "effort": info.get("effort"),
        "protocol": run.get("protocol"),
        "fileReadEnforcement": read_enforcement(run), "fileWriteEnforcement": run.get("fileWriteEnforcement"),
        "networkEnforcement": run.get("networkEnforcement"),  # what the agent could reach is part of the environment being scored
    }


def card(results: list[dict], language: str, expected: list[str]) -> dict:
    """The score card of one language track, or a refusal when the runs are not one comparable environment."""
    track = [r for r in results if r.get("language") == language and (r.get("condition") or {}).get("label") == CANONICAL and r.get("split") == "test"]
    excluded = defaultdict(list)
    valid = []
    for run in track:
        (valid if run.get("status") in VALID else excluded[run.get("status") or "missing-status"]).append(run)
    if not valid:
        return {"contract": CONTRACT, "track": TRACKS[language], "refused": "no valid canonical test runs for this language"}
    identities = {json.dumps(identity_of(r), sort_keys=True) for r in valid}
    if len(identities) > 1:
        differing = sorted({k for a in map(json.loads, identities) for b in map(json.loads, identities) for k in a if a[k] != b[k]})
        return {"contract": CONTRACT, "track": TRACKS[language], "refused": f"runs come from more than one environment; they differ in: {', '.join(differing)}"}
    identity = json.loads(next(iter(identities)))
    per: dict[str, list[int]] = defaultdict(list)
    by_scenario: dict[str, list[dict]] = defaultdict(list)
    for run in valid:
        per[run["scenario"]].append(1 if run.get("usable") else 0)
        by_scenario[run["scenario"]].append(run)
    counts = {len(v) for v in per.values()}
    provisional = []
    missing = sorted(set(expected) - set(per))
    if missing:
        provisional.append(f"missing held-out scenarios: {', '.join(missing)}")
    if len(counts) > 1:
        provisional.append(f"unequal valid trials per scenario: {sorted(counts)}")
    if len(expected) < MIN_SCENARIOS:
        provisional.append(f"fewer than {MIN_SCENARIOS} held-out scenarios in the suite ({len(expected)})")
    k = min(counts)
    rates = {s: sum(v) / len(v) for s, v in per.items()}
    lo, hi = cluster_interval(per)
    families: dict[str, list[float]] = defaultdict(list)
    layers: dict[str, int] = defaultdict(int)
    for s, runs in by_scenario.items():
        families[runs[0]["family"]].append(rates[s])
        for run in runs:
            for layer in run.get("failedLayers") or []:
                layers[layer] += 1
    tokens = [(r.get("usage") or {}).get("totalTokens") for r in valid]
    usable_tokens = [t for t, r in zip(tokens, valid) if r.get("usable") and t]
    return {
        "contract": CONTRACT, "track": TRACKS[language], "scoreVersion": "v1", "language": language,
        "score": round(100 * sum(rates.values()) / len(rates), 1), "ci95": [round(lo, 1), round(hi, 1)], "ciMethod": METHOD,
        "trialsPerScenario": k, "scenarios": len(per), "validRuns": len(valid),
        "passPowK": round(100 * sum(pass_power_k(sum(v), len(v), k) for v in per.values()) / len(per), 1),
        "provisional": provisional,
        "identity": identity,
        "suite": (valid[0]["environment"].get("suite") or {}),
        "harness": sorted({r["environment"].get("harness") for r in valid if r["environment"].get("harness")}),
        "networkEnforcement": sorted({r.get("networkEnforcement") or "not recorded" for r in valid}),
        "fileWriteEnforcement": sorted({r.get("fileWriteEnforcement") or "not recorded" for r in valid}),
        "fileReadEnforcement": sorted({read_enforcement(r) or "not recorded" for r in valid}),
        "exclusions": {status: len(runs) for status, runs in sorted(excluded.items())},
        "perScenario": [{"scenario": s, "usable": sum(v), "valid": len(v), "rate": round(rates[s], 3)} for s, v in sorted(per.items())],
        "byFamily": {f: round(100 * sum(v) / len(v), 1) for f, v in sorted(families.items())},
        "failureLayers": dict(sorted(layers.items())),
        "secondary": {
            "tokensPerRun": round(sum(t for t in tokens if t) / max(1, sum(1 for t in tokens if t))) if any(tokens) else None,
            "tokensPerUsableResult": round(sum(t for t in tokens if t) / len(usable_tokens)) if usable_tokens else None,
            "secondsPerRun": round(sum(r["agent"]["seconds"] for r in valid) / len(valid), 1),
        },
    }


def render(c: dict) -> str:
    if "refused" in c:
        return f"{c['track']}: no score. {c['refused']}\n"
    ident = c["identity"]
    lines = [
        f"{c['track']} {c['scoreVersion']}", "",
        f"Agent:       {ident['agent']}", f"Model:       {ident['model'] or 'not recorded'}", f"Inference:   effort {ident['effort'] or 'not recorded'}", "",
        f"Score:       {c['score']}", f"95% CI:      {c['ci95'][0]}-{c['ci95'][1]}  ({c['ciMethod']})",
        f"Pass^{c['trialsPerScenario']}:      {c['passPowK']} (secondary)",
        f"Trials:      {c['trialsPerScenario']} per scenario ({c['validRuns']} valid runs)", f"Scenarios:   {c['scenarios']} held-out", "",
        f"Suite:       {c['suite'].get('version')} {c['suite'].get('hash')}", f"Wright:      {ident['wright']} sha256 {ident['wrightSha256']}",
        f"Skills:      {', '.join(f'{n} {h}' for n, h in ident['skills'].items()) or 'none'}", f"Harness:     {', '.join(c['harness']) or 'not recorded'}",
        f"Network:     {', '.join(x or 'not recorded' for x in c['networkEnforcement'])}", f"File writes: {', '.join(x or 'not recorded' for x in c['fileWriteEnforcement'])}",
        f"File reads:  {', '.join(x or 'not recorded' for x in c['fileReadEnforcement'])}",
        f"Excluded:    {c['exclusions'] or 'none'}",
    ]
    if c["provisional"]:
        lines += ["", "PROVISIONAL: " + "; ".join(c["provisional"])]
    if "declared-only" in c["networkEnforcement"]:
        lines += ["", "Network was off by declaration only for some runs; the canonical condition asks for it to be disabled."]
    lines += ["", "By scenario: " + ", ".join(f"{p['scenario']} {p['usable']}/{p['valid']}" for p in c["perScenario"]), "By family: " + ", ".join(f"{f} {v}" for f, v in c["byFamily"].items()),
              "Failure layers: " + (", ".join(f"{k} {v}" for k, v in c["failureLayers"].items()) or "none")]
    return "\n".join(lines) + "\n"


COMPARABLE = ("wrightSha256", "skills", "suite", "fileReadEnforcement", "fileWriteEnforcement", "networkEnforcement")  # what must match for two cards to be read side by side; agent, model, and effort are what is being compared


def compare(dirs: list[Path]) -> str:
    """One table from the score cards of several evaluation runs, with a warning when they were not made against the same Wright, skills, and suite."""
    rows, identities = [], {}
    for directory in dirs:
        path = directory / "score.json"
        if not path.is_file():
            rows.append(("", directory.name, "no score", "no score.json in this directory"))
            continue
        for card_ in json.loads(path.read_text())["cards"]:
            if "refused" in card_:
                rows.append((card_["track"], directory.name, "no score", card_["refused"]))
                continue
            ident = card_["identity"]
            identities.setdefault(card_["track"], []).append((directory.name, {k: ident.get(k) for k in COMPARABLE}))  # cards written before a field existed compare as 'not recorded'
            label = " ".join(filter(None, (ident.get("agent"), ident.get("effort") and f"effort {ident['effort']}")))
            note = "provisional: " + "; ".join(card_["provisional"]) if card_["provisional"] else ""
            rows.append((card_["track"], directory.name, f"{card_['score']} [{card_['ci95'][0]}-{card_['ci95'][1]}]", f"{label}; {card_['trialsPerScenario']} trial(s) x {card_['scenarios']} scenarios; excluded {card_['exclusions'] or 'none'}. {note}".strip()))
    lines = ["| track | run | score [95% CI] | agent and notes |", "| --- | --- | --- | --- |", *(f"| {t} | {d} | {s} | {n} |" for t, d, s, n in sorted(rows))]
    for track, entries in identities.items():
        differing = sorted({k for _, a in entries for _, b in entries for k in COMPARABLE if a[k] != b[k]})
        if differing:
            lines.append(f"\nWARNING {track}: runs differ in {', '.join(differing)}, so these scores are not directly comparable.")
    return "\n".join(lines) + "\n"


def main(dirs: list[Path], languages: list[str], expected_by_language: dict[str, list[str]], out: Path | None) -> int:
    from bench_report import load
    results = load(dirs)
    status = 0
    cards = []
    for language in languages:
        c = card(results, language, expected_by_language[language])
        cards.append(c)
        print(render(c))
        status = max(status, 2 if "refused" in c else 0)
    target = out or dirs[0]
    (target / "score.json").write_text(json.dumps({"contract": CONTRACT, "cards": cards}, indent=2) + "\n")
    (target / "score.txt").write_text("\n".join(render(c) for c in cards))
    return status
