// ADR-0032 decision 10: the production bundle's JavaScript and CSS together
// must stay under 100 KiB gzipped.

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { gzipSync } from 'node:zlib';

const DIST = resolve(dirname(fileURLToPath(import.meta.url)), '..', 'dist');
const BUDGET = 100 * 1024;

function* walk(dir) {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) {
      yield* walk(path);
    } else {
      yield path;
    }
  }
}

let total = 0;
for (const path of walk(DIST)) {
  if (!path.endsWith('.js') && !path.endsWith('.css')) continue;
  const size = gzipSync(readFileSync(path)).length;
  total += size;
  console.log(`${size} B gzipped  ${path.slice(DIST.length + 1)}`);
}
console.log(`total ${total} B gzipped (budget ${BUDGET} B)`);
if (total > BUDGET) {
  console.error('bundle exceeds the 100 KiB gzipped budget');
  process.exitCode = 1;
}
