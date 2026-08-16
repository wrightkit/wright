# wright-lpp

Wright's Language Provider Protocol (LPP) v1 client: provider discovery,
stdio process lifecycle, JSON-RPC correlation, capability negotiation, and
structured failures.

The wire contract (message shapes, methods, error kinds, conformance
fixtures) is owned by the
[`language-provider-protocol`](https://github.com/wrightkit/language-provider-protocol)
repository; this crate consumes that contract and does not redefine it.

## Layers

```text
ToolService / language services
        |
        |  LanguageProvider (transport-neutral, language-neutral trait)
        v
 StdioLanguageProvider    -- capability guards, typed LPP data mapping
        |
        |  JsonRpcClient   -- framing, correlation, timeouts, session phase
        v
 ChildProcess             -- spawn/kill/wait a long-running stdio provider
        |
        v
 a provider binary (any source language; the conformance reference
 provider serves the deliberately foreign `x-demo-lang` language)
```

`StdioLanguageProvider` implements the `LanguageProvider` trait, which is
the stable seam ToolService and language services consume. The trait exposes
provider capabilities and source-oriented operations only; JSON-RPC framing,
correlation ids, process handles, and timeouts stay below it.

## Language neutrality

Nothing in this crate branches on a particular source language. Providers
are discovered by opaque language id strings through `ProviderRegistry`; a
language id such as `x-demo-lang` is just a key. When no provider is
configured for a language id, or when a required capability was not
negotiated, the client refuses explicitly with a structured
`ProviderError` — there is no silent fallback to in-process compiler
semantics.

## Capabilities

All eight LPP v1 capabilities are supported: `check`, `compile`,
`reconstruct`, `symbols`, `definition`, `references`, `rename`, and
`editValidation`. Every method is guarded: invoking a method whose
capability was not negotiated fails with `capability-unavailable` before
anything reaches the provider.

## Process lifecycle

* spawn the provider binary with stdin/stdout piped (stderr passes through
  for provider logging);
* `lpp/initialize` handshake with protocol version `1.0`, including
  protocol-version-mismatch handling (the session stays restartable);
* request correlation with integer ids, timeouts, and late-response
  tolerance;
* `lpp/shutdown`, then end-of-file on the provider's stdin and a bounded
  wait for exit status 0 (with kill fallback);
* deterministic handling of provider exit: pending requests fail with
  `provider-exited` carrying the observed status.

## Failures

Every interaction fails deterministically into a structured
`ProviderError` variant with a stable machine `code()`:

| code | when |
| --- | --- |
| `provider-not-configured` | no provider is registered for the language id |
| `provider-spawn` | the provider binary could not be spawned |
| `provider-io` | a transport read/write failure |
| `provider-exited` | the provider process exited without responding |
| `provider-timeout` | no response within the configured timeout |
| `provider-malformed` | provider output is not a valid LPP v1 message |
| `jsonrpc-error` | a standard JSON-RPC error (`-32700`..`-32603`) |
| `protocol-version-mismatch` | unsupported protocol version (wire or echo) |
| `capability-unavailable` | required capability not negotiated |
| `refusal` | a well-formed, machine-readable decline (normal outcome) |
| `invalid-language`, `invalid-document`, `invalid-position`, `invalid-artifact`, `invalid-request` | typed LPP errors |
| `provider-not-initialized` / `provider-already-initialized` / `provider-shutdown` | client-side session-phase violations |

## Testing

Unit tests (`tests/unit.rs` and in-crate tests) exercise framing,
correlation, timeouts, malformed responses, protocol mismatches, session
phase guards, and capability refusals against a scripted fake provider over
OS pipes — no provider binary needed.

The end-to-end suite (`tests/mock_provider.rs`) runs against the reference
conformance mock provider (`x-demo-lang`) from
`language-provider-protocol`. It is skipped with a clear reason when the
binary is not configured, and REQUIRED in CI (see `.github/workflows/ci.yml`).

Pinned `language-provider-protocol` commit:
`416b293e26e6fb2d29061608a493a7aecd2ce14f`.

```text
git clone https://github.com/wrightkit/language-provider-protocol
git -C language-provider-protocol checkout 416b293e26e6fb2d29061608a493a7aecd2ce14f
cargo build -p lpp-mock-provider           # inside language-provider-protocol
```

Then run the suite from the wright workspace root:

```text
cargo test -p wright-lpp
LPP_MOCK_PROVIDER=language-provider-protocol/target/debug/lpp-mock-provider \
  cargo test -p wright-lpp --test mock_provider
LPP_MOCK_PROVIDER=language-provider-protocol/target/debug/lpp-mock-provider \
  cargo test -p wright-driver --test lpp
```

## Integration

`wright-driver` sessions carry a `ProviderRegistry` in their
`SessionConfig`; `CompilerSession::language_provider(language_id)` and
`ToolService::language_provider(language_id)` spawn a provider client for an
opaque language id. `ProviderRegistry::from_env()` registers the mock
provider for `x-demo-lang` from the `LPP_MOCK_PROVIDER` environment
variable (the CI/test hook).
