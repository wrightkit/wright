# @wrightkit/wright

Official npm distribution package for the native [Wright](https://github.com/wrightkit/wright) toolchain.

Wright is a tooling-first semantic platform and compiler for the Overwatch Workshop and OverPy ecosystem, natively written in Rust.

**Note**: This package is a distribution wrapper for the native Rust binary, **not** a JavaScript reimplementation. When installed via npm, it downloads the precompiled native binary for your platform without requiring Rust or a local build step.

## Installation

```bash
# Local dependency
npm install @wrightkit/wright

# Run via npx
npx wright --version
npx wright check main.opy
```

## Programmatic API

Downstream Node.js and TypeScript tools can import `@wrightkit/wright` to locate the native binary:

```javascript
const { getBinaryPath } = require('@wrightkit/wright');

// Get path to native 'wright' executable
const wrightBin = getBinaryPath('wright');

// Get path to native 'wright-lsp' executable
const lspBin = getBinaryPath('wright-lsp');
```

## Supported Platforms

- macOS Apple Silicon (arm64): `@wrightkit/wright-darwin-arm64`
- macOS Intel (x64): `@wrightkit/wright-darwin-x64`
- Linux (x64): `@wrightkit/wright-linux-x64`
- Windows (x64): `@wrightkit/wright-win32-x64`

## License

AGPL-3.0-or-later
