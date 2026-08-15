const path = require('path');
const fs = require('fs');

const PLATFORMS = {
  'darwin-arm64': '@wrightkit/wright-darwin-arm64',
  'darwin-x64': '@wrightkit/wright-darwin-x64',
  'linux-x64': '@wrightkit/wright-linux-x64',
  'win32-x64': '@wrightkit/wright-win32-x64',
};

function getPlatformKey() {
  return `${process.platform}-${process.arch}`;
}

function getPlatformPackageName() {
  const key = getPlatformKey();
  return PLATFORMS[key] || null;
}

function findPlatformPackageDir(pkgName) {
  // 1. Standard require.resolve from this package and current working directory
  try {
    const manifestPath = require.resolve(`${pkgName}/package.json`, {
      paths: [__dirname, process.cwd()],
    });
    return path.dirname(manifestPath);
  } catch {}

  // 2. Sibling lookup in node_modules/@wrightkit/<name>
  const shortName = pkgName.replace('@wrightkit/', '');
  const siblingDir = path.resolve(__dirname, '..', shortName);
  if (fs.existsSync(path.join(siblingDir, 'package.json'))) {
    return siblingDir;
  }

  // 3. Nested layout (e.g. node_modules/@wrightkit/wright/node_modules/@wrightkit/...)
  const nestedDir = path.resolve(__dirname, 'node_modules', pkgName);
  if (fs.existsSync(path.join(nestedDir, 'package.json'))) {
    return nestedDir;
  }

  return null;
}

function getBinaryPath(binName = 'wright') {
  const key = getPlatformKey();
  const pkgName = PLATFORMS[key];
  if (!pkgName) {
    throw new Error(
      `Unsupported platform/architecture for @wrightkit/wright: ${process.platform} (${process.arch}).\n` +
      `Supported platforms: ${Object.keys(PLATFORMS).join(', ')}`
    );
  }

  const pkgDir = findPlatformPackageDir(pkgName);
  if (!pkgDir) {
    throw new Error(
      `Platform package "${pkgName}" for ${process.platform} ${process.arch} is not installed.\n` +
      `Ensure optionalDependencies are enabled during installation, or install it explicitly:\n` +
      `  npm install ${pkgName}`
    );
  }

  const exeName = process.platform === 'win32' ? `${binName}.exe` : binName;
  const binPath = path.join(pkgDir, exeName);

  if (!fs.existsSync(binPath)) {
    throw new Error(
      `Executable "${exeName}" was not found at "${binPath}". The platform package "${pkgName}" appears incomplete or corrupted.`
    );
  }

  return binPath;
}

module.exports = {
  getBinaryPath,
  getPlatformPackageName,
  getPlatformKey,
  PLATFORMS,
};
