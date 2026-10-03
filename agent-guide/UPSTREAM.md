# Vendored agent guide

`wright/` is a verbatim copy of the `wright` skill from
`https://github.com/wrightkit/skills` at commit `c1ae8ee`, embedded in the
binary for `wright agent install`. The skills repository is the authoring
home; sync this copy deliberately — replace the directory contents and update
both the pin above and `UPSTREAM` in `crates/wright-cli/src/agent.rs` —
rather than editing it here.
