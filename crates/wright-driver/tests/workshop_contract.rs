//! The external Workshop corpus contract is owned and tested by `workshop-rs`.
//! Wright's canonical integration is covered by the driver and provider tests.

#[test]
#[ignore = "requires the pinned external Workshop corpus contract"]
fn provider_matches_workshop_contract() {
    // The release no longer exposes its test-only expectation module to
    // downstream crates. The owner crate runs this corpus contract.
}
