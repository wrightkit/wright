# ADR-0038: Bounded R2 retention with GitHub redirect for pruned stable versions

- Status: Accepted
- Date: 2026-11-11
- Related: [Issue #1077](https://github.com/wrightkit/wright/issues/1077),
  [ADR-0014](0014-namespaced-immutable-r2-artifacts.md),
  [docs/release.md](../release.md)

## Context

ADR-0014 keeps every R2 object forever. The `wrightkit-release` bucket nears
the 10 GB free tier. GitHub Releases already hold every stable archive, so R2
needs only a recent window. Nightly objects exist only in R2.

## Decision

- R2 keeps the newest 5 nightly commit prefixes. The publisher deletes older
  nightly prefixes after it advances `wright/nightly/version`.
- R2 keeps the newest 3 stable version prefixes, ordered by semver. The
  publisher deletes older prefixes after it advances `wright/latest/version`.
- A Cloudflare Worker serves `releases.wrightkit.dev` and
  `install.wrightkit.dev` from the bucket. When a
  `wright/releases/<version>/` archive or checksum is missing from R2, it
  returns a 302 to the same file in the `v<version>` GitHub Release. All other
  missing keys return 404.
- Stable pruning runs only when the repository variable
  `R2_REDIRECT_WORKER_DEPLOYED` is `true`. Deploy the Worker first.

This supersedes the rejection of a Worker in ADR-0014. The rejected Worker
resolved the latest version. This Worker only redirects pruned immutable paths,
and it holds no state.

## Consequences

- Pinned R2 URLs for pruned versions keep working through the redirect.
  Installers and `wright update` follow redirects.
- Nightly commits older than the window are gone.
- Retained objects stay immutable and byte-identical to the release artifacts.
- Changing a retention number is a one-line workflow change.
