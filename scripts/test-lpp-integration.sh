#!/usr/bin/env bash
# Run Wright's required LPP client integration suites against the mock provider.

set -euo pipefail

cargo test --locked -p wright-lpp --test mock_provider
cargo test --locked -p wright-driver --test lpp
cargo test --locked -p wright-driver --test provider_edit
