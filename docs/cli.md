# Wright CLI and Driver Contract

Status: accepted baseline (living driver and CLI contract)
Scope: `wright` executable, `wright-driver` crate, and their machine-readable
contracts

This file is the stable CLI/driver entry point. Detailed contracts are split by
responsibility so implementation and review tasks can load only the relevant
surface.

## CLI presentation and completion (#164, #186)

See [presentation and completion](cli/presentation.md).

## Architecture

See [architecture, commands, and conversion](cli/commands.md).

## Commands

See [architecture, commands, and conversion](cli/commands.md).

## `wright convert` and the reconstruction surface (#126)

See [architecture, commands, and conversion](cli/commands.md).

## `wright lint` and the lint configuration

See [lint configuration and findings](cli/lint.md).

## Exit codes

See [machine-readable CLI contracts](cli/machine-contract.md).

## `wright update` (self-update)

See [self-update](cli/update.md).

## stdout / stderr ownership

See [machine-readable CLI contracts](cli/machine-contract.md).

## `wright-result/v1` envelope

See [machine-readable CLI contracts](cli/machine-contract.md).

## Determinism

See [machine-readable CLI contracts](cli/machine-contract.md).

## The `.opy` source implementation

See [source-provider and library integration](cli/integration.md).

## Library reuse

See [source-provider and library integration](cli/integration.md).
