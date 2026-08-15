#!/usr/bin/env node
const { getBinaryPath } = require('../index.js');
const { spawnSync } = require('child_process');

let binPath;
try {
  binPath = getBinaryPath('wright');
} catch (err) {
  console.error(err.message);
  process.exit(1);
}

const result = spawnSync(binPath, process.argv.slice(2), {
  stdio: 'inherit',
  windowsHide: true,
});

if (result.error) {
  console.error(`Failed to execute wright at "${binPath}": ${result.error.message}`);
  process.exit(1);
}

if (result.signal) {
  process.kill(process.pid, result.signal);
} else {
  process.exit(result.status ?? 0);
}
