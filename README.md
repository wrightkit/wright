# Wright

Wright is a Rust-based compiler and tooling project for the Overwatch Workshop / OverPy ecosystem.

## Status

Wright is in the early project-bootstrap stage. The repository does not yet publish a stable compiler binary, CLI, library API, or release workflow. Those contracts will be introduced incrementally and documented when they become available.

## Project direction

Wright is intended to provide a clear, testable foundation for working with Workshop-oriented source code and generated output. The project prioritizes:

* semantic correctness over text-only transformations;
* measurable compatibility with supported reference behavior;
* explicit diagnostics for invalid or unsupported input;
* deterministic output and inspectable implementation boundaries.

## Development

Read [`AGENTS.md`](AGENTS.md) before making changes. It describes the repository boundary, source-of-truth order, compatibility expectations, validation policy, and delivery workflow.

There is no public installation or build command yet. Once the Rust workspace and command surface are established, this section will document the supported local setup and verification commands.

## License

Wright is distributed under the [GNU Affero General Public License v3.0 or later](LICENSE).

AGPL permits commercial use, but a modified version that users interact with
over a network must offer those users access to its corresponding source code.
See the license text for the exact requirements.
