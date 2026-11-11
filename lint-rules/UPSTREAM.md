# Embedded first-party lint rules

`rules/` is a verbatim copy of `rules/` from
`https://github.com/wrightkit/lint-rules` at commit
`08cb2c84fb40d02ab75961ce3088881eec84a035`, embedded in the binary as the
default first-party lint package. The lint-rules repository is the authoring
home; never edit this copy. A project that pins `wrightkit/lint-rules` under
`packages` replaces this default with the pinned revision.

To move the embedded revision, clone `wrightkit/lint-rules` and run

```sh
python3 scripts/sync-lint-rules.py --lint-rules-dir <lint-rules clone> [--ref <commit>]
```

which replaces `rules/` and rewrites the commit above. CI runs the same script
with `--check` and fails when the copy drifts from the recorded commit or a
file is not embedded by `crates/wright-analyzer/src/first_party.rs`. The
update procedure is in [`docs/cli/lint.md`](../docs/cli/lint.md#embedded-first-party-rules).
