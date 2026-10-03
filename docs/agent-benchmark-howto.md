# Run the agent benchmark

Run it from a shell, one command for every model, and get a results page you can publish. Background and the full contract are in [agent-benchmark.md](agent-benchmark.md).

## Set up once

1. Install `wright` on `PATH`, and the agent programs you want to test, each logged in.
2. Install the OverPy reference compiler the grader uses:
   ```sh
   python3 benchmarks/agent/agent_bench.py setup-oracle
   ```
3. Write `~/.config/wright-agent-bench/config.json` (the `WRIGHT_BENCH_CONFIG` variable overrides the path):
   ```json
   {
     "skill_dirs": {
       "wright-skill": "~/skills/skills/wright",
       "opy-skill": "~/skills/skills/overpy"
     },
     "models": [
       {"adapter": "devin", "model": "swe-2-max"},
       {"adapter": "codex", "model": "gpt-6-luna", "effort": "xhigh"},
       {"adapter": "pi", "model": "openai-codex/gpt-6-luna", "effort": "xhigh"},
       {"adapter": "grok", "model": "grok-4.7"}
     ]
   }
   ```
   `skill_dirs` points at the `wrightkit/skills` checkout. Keep the Wright binary, the skills, and the scenarios unchanged for as long as you want results to be comparable.

## Where things live

Everything is under one directory, `~/.local/share/wright-agent-bench` (the `WRIGHT_BENCH_HOME` variable moves it):

| Path | Holds |
| --- | --- |
| `runs/` | every evaluation run and the results page (`runs/results/leaderboard/`) |
| `wiki/` | local wiki snapshots (not published) |
| `skills-*/` | pinned copies of the skills under test |

Runs made by earlier versions are in `~/.cache/wright-agent-bench`; nothing writes there any more.

## Run everything

```sh
python3 benchmarks/agent/agent_bench.py suite --dry-run   # checks the setup, runs nothing
python3 benchmarks/agent/agent_bench.py suite
```

It evaluates the models one after another, each on 16 tasks tried 3 times, one trial at a time so provider limits are not hit. It is safe to stop and repeat: finished trials are skipped, and a model that hits a quota or an outage waits for the next run (the command ends with exit code 3 and says which). Run it again later, on another day if needed, and the same command carries on.

The results are in `~/.local/share/wright-agent-bench/runs/results/leaderboard/`:

| File | For |
| --- | --- |
| `LEADERBOARD.md` | pasting into a README or an issue |
| `leaderboard.html` | one self-contained page to host or share |
| `leaderboard.json` | other tools |

Use `--only devin codex:gpt-6-luna` to run some entries only.

## Run one model

```sh
python3 benchmarks/agent/agent_bench.py evaluate --adapter codex --model gpt-6-luna --effort xhigh --name codex-luna-xhigh
```

Put several runs side by side, or rebuild the page from chosen runs:

```sh
python3 benchmarks/agent/agent_bench.py compare ~/.local/share/wright-agent-bench/runs/{run-a,run-b}
python3 benchmarks/agent/agent_bench.py leaderboard ~/.local/share/wright-agent-bench/runs/{run-a,run-b}
```

Runs made against a different Wright binary, skills, or task suite are listed as not comparable instead of being ranked.

## The agent programs

| Program | `adapter` | `model` | `effort` | State |
| --- | --- | --- | --- | --- |
| Devin CLI | `devin` | `swe-2-max` (the effort is part of the name) | read from the name | works |
| Codex CLI | `codex` | `gpt-6-luna` | `low` to `xhigh` | works |
| pi | `pi` | `openai-codex/gpt-6-luna` (`pi --list-models`) | `low` to `xhigh` | works |
| Grok CLI | `grok` | `grok-4.7` (`grok models`) | optional | works |
| opencode | `opencode` | `openai/gpt-6-luna` (`opencode models`) | optional | works once its login is valid |
| Antigravity CLI | `agy` | `gemini-3.8-flash-high` | optional | not yet checked with the file sandbox |
| Claude Code | `claude-code` | `sonnet` | none | not yet checked with the file sandbox |
| Built-in loop, no program | `direct` | `anthropic/<model>` or `openai/<model>` | `openai/` only | needs an API key in the environment |

The agent program runs the model, so the same model scores differently under different programs. The page shows both.

## What the page tells you

- The score is the share of tasks an agent finished with a valid, safe result. A bar shows it; the bracketed range is the 95% interval.
- With 8 tasks per language the range is wide. A row marked "tied with top" cannot be told apart from the first.
- It is a reference for how an agent behaves with Wright and its guide, not a measure of general ability. Network access is off by instruction only, not blocked.

## When something goes wrong

| What you see | Meaning |
| --- | --- |
| Exit code 3, "waiting" | quota or outage; run the same command later |
| `cannot start: ...` | `--dry-run` names what is missing (binary, login, skill directory, oracle) |
| An agent exits at once with an auth error | its login expired; sign in again with that program |
| An agent cannot start under the file sandbox | add the paths it needs: `--allow-read PATH` or `allow_read` in the config |

The agent only sees its own run directory, the condition's skills, the tool binaries, and what its program needs to start; the home directories, drives, and benchmark data are hidden from it, so it cannot find the answers or other runs.
