# Workshop integration coverage

Status: current Wright integration contract

`workshop-rs` owns the canonical Workshop support surface, catalog, parser,
semantic completeness, localization, and real-project corpus. Wright does not
derive a second Workshop feature matrix from source-language output.

Wright verifies its integration through these named tests:

| Wright surface | Test contract |
| --- | --- |
| Analyzer | `crates/wright-analyzer/tests/workshop_integration.rs` runs semantic queries, findings, references, and source-span checks on localized Workshop inputs. |
| Driver | `crates/wright-driver/tests/workshop_contract.rs` compares provider diagnostics with the released owner `Program` API over the owner corpus. |
| CLI | `crates/wright-cli/tests/workshop_real_projects.rs` runs public `check` and `lint` and compares their diagnostics with the owner API. |
| Provider | `crates/wright-driver/tests/workshop_provider.rs` checks successful and malformed Workshop input through the provider boundary. |
| Public consumers | `crates/wright-consumer/tests/consumer.rs` exercises the public embedding workflow on representative Workshop inputs. |

The small local inputs used by these tests are listed in
[`tests/fixtures/README.md`](../../tests/fixtures/README.md). They are
consumer-specific regressions and smoke inputs, not a compatibility corpus or
a source-language support claim.

New Workshop semantic support belongs in `workshop-rs`. New Wright tests should
assert a Wright-owned boundary or cross-check an owner contract rather than
recording a second owner snapshot.
