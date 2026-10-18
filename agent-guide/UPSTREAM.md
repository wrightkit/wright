# Vendored agent guide

`wright/` is a verbatim copy of the `wright` skill from
`https://github.com/wrightkit/skills` at commit `478ea04`, embedded in the
binary for `wright agent install`. The skills repository is the authoring
home; never edit this copy. To sync it, clone `wrightkit/skills` and run

```sh
python3 scripts/sync-agent-guide.py --skills-dir <skills clone>
```

which replaces the directory contents and rewrites both pins (the one above
and `UPSTREAM` in `crates/wright-cli/src/agent.rs`). CI runs the same script
with `--check` and fails when the copy or either pin drifts from the skills
repository. Sync in its own change: agent results measured with a previous
guide are not comparable.
