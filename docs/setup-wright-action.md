# `setup-wright` GitHub Action

Status: current contract (#717)
Scope: installing and caching the Wright CLI in GitHub Actions workflows

The Action lives in this repository at `.github/actions/setup-wright`. It only
sets up Wright. Command selection, arguments, and gating stay in ordinary
`run: wright ...` steps, so annotations, step summaries, and exit codes are
Wright's own (see [`cli/presentation.md`](cli/presentation.md)).

## Usage

Reference the Action at a Wright release ref, then run Wright as often as
needed:

```yaml
jobs:
  wright:
    runs-on: ubuntu-latest
    permissions:
      contents: read
    steps:
      - uses: actions/checkout@v7
      - uses: wrightkit/wright/.github/actions/setup-wright@v0.28.0
        with:
          version: 0.28.0
      - run: wright check
      - run: wright lint
      - run: wright analyze
```

| Input | Default | Meaning |
| --- | --- | --- |
| `version` | `latest` | Exact Wright version such as `0.28.0` (a leading `v` is accepted), or `latest`. |
| `cache` | `true` | Cache the verified executable. `false` downloads on every run. |

| Output | Meaning |
| --- | --- |
| `version` | Concrete Wright version installed. |
| `cache-hit` | `true` when a valid cached executable avoided the release download. |

The Action has no independent version. Use a Wright release tag (or a commit
SHA) as the `@ref`. The ref selects the Action's code; `version` selects the
Wright binary, and the two are independent.

Supported runners: Linux x86_64, macOS x86_64, macOS arm64, and Windows
x86_64, the targets with published release archives
([`release.md`](release.md#target-matrix-and-artifact-naming)). Any other
platform fails with an `unsupported platform` error.

## Version pinning

* An exact `version` is deterministic and, on a cache hit, makes no network
  request to the release distribution.
* `latest` first reads the small `latest/version` pointer from the release
  distribution (`no-store`, a few bytes), then uses the concrete version for
  every later step. A cache hit for that version never re-downloads the archive;
  a new pointer value selects a new cache entry and installs the new binary.
* Pin an exact version in workflows that must be reproducible.

## Cache identity

Only the `wright` executable and a SHA-256 sidecar are cached, with the Actions
cache service of the consuming repository. The key is exact:

```text
setup-wright-v1-<version>-<target-triple>
```

* There is no restore prefix, so a different version, OS, or architecture can
  never restore into the wrong executable.
* `v1` is the cache layout; changing the layout bumps it and abandons old
  entries.
* Actions caches are scoped to the consuming repository and ref: a pull request
  writes to its own scope, not to the default branch, and a fork pull request
  with a read-only token only restores. The Action needs no secrets and no
  permissions beyond `contents: read`.

On a miss the Action downloads the archive and its `.sha256` from the immutable
release route, verifies the checksum before extraction, and only then saves the
executable.

After every restore the Action recomputes the executable's SHA-256 against the
sidecar and requires `wright --version` to report the requested version. A
failing entry is deleted, a warning is printed, and the release is downloaded
and verified again. An invalid entry keeps its key until the cache service
evicts it, because Actions cache keys are immutable, so each run re-downloads
until then. The sidecar detects corruption and a wrong version. It does not
authenticate the cache against a writer who can alter both files; that trust
boundary is the consuming repository.

## What is not cached

Provider downloads, project sources, analysis or lint results, and build output
are never cached. Providers remain managed by Wright itself. Set `cache: false`
to skip the cache service entirely.

## Verification

* `scripts/test-setup-wright.sh` runs the helper against a mock release
  distribution: cache hit and miss (including zero requests on a hit), version
  and `latest` resolution, OS/architecture/version key separation, corrupt and
  wrong-version caches, checksum mismatch, a missing release, and unsupported
  platforms.
* The `setup-wright Action` job in `ci.yml` uses the Action from the checked-out
  ref on Linux, macOS, and Windows against the latest published release, moves
  the installed copy aside, installs again and requires `cache-hit == true` (skipped on fork pull requests, which cannot write the cache),
  then runs `wright check` as an ordinary step and requires the step summary to
  report PASS.
