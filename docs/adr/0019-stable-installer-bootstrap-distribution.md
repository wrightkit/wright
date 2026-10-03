# ADR-0019: Stable installer bootstrap distribution

- Status: Accepted
- Date: 2026-09-27
- Related: [ADR-0014](0014-namespaced-immutable-r2-artifacts.md),
  [docs/release.md](../release.md)

## Context

The website build copied Wright's canonical installer files into its static
output. That made the website delivery layer responsible for the first
installer request and required it to synchronize and validate another
repository's release scripts. Wright already owns the scripts and the R2
release publication that those scripts use.

## Decision

- Wright's `install.sh` and `install.ps1` remain the canonical installer
  sources. The stable release workflow publishes the exact files from its
  release commit to `wright/install.sh` and `wright/install.ps1` in the
  `wrightkit-release` R2 bucket.
- The public entrypoints are
  `https://install.wrightkit.dev/wright/install.sh` and
  `https://install.wrightkit.dev/wright/install.ps1`. The hostname is an R2
  custom domain attached to the existing release bucket. The website only
  displays these commands; it does not copy or publish the files.
- These two unversioned bootstrap objects are mutable and use
  `Cache-Control: no-store, max-age=0` and
  `Content-Type: text/plain; charset=utf-8`. The stable publisher uploads and
  verifies both scripts with the immutable archive/checksum set, before the
  GitHub Release becomes public (#293): it fetches them back, compares exact
  bytes, and verifies their cache and content-type headers, and
  `wright/latest/version` advances only after the canonical release is public.
- Nightly publication does not change the stable bootstrap objects. The
  scripts continue to resolve and download release archives from
  `releases.wrightkit.dev`.

The immutable versioned archive/checksum paths and the version-pointer behavior
in ADR-0014 remain unchanged. The install custom domain serves the same public
bucket object keys; the installer script paths are the documented bootstrap
surface.

## Alternatives considered

- **Keep copying scripts through the website build:** rejected because it
  assigns installer publication and request delivery to the consumer site.
- **Fetch scripts from a moving GitHub branch:** rejected because the stable
  release workflow should publish the canonical files and verify the public
  endpoint as part of Wright's distribution contract.

## Consequences

- The first install request and the binary download path have an explicit
  Wright-owned release boundary.
- Stable scripts are updated only by stable releases, not by website builds or
  nightly publication.
- Release publication requires the `install.wrightkit.dev` R2 custom domain to
  be active and readable over HTTP/1.1.

## Compatibility impact

The documented install commands move from `wrightkit.dev` script paths to
`install.wrightkit.dev`. The old website-served script paths are no longer part
of the supported bootstrap contract.

## Scope boundaries

This decision does not change the contents or runtime behavior of either
installer, release archive naming, checksums, package-manager distribution, or
the immutable R2 artifact contract in ADR-0014.
