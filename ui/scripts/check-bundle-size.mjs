
import { gzipSync } from 'node:zlib'
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { dirname } from 'node:path'

const BUDGET_KIB = 100
const uiDir = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const distDir = join(uiDir, 'dist')

function* bundleFiles(dir) {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry)
    if (statSync(path).isDirectory()) {
      yield* bundleFiles(path)
    } else if (/\.(js|css)$/.test(entry)) {
      yield path
    }
  }
}

let files
try {
  files = [...bundleFiles(distDir)]
} catch {
  console.error('dist/ not found; run `npm run build` first')
  process.exit(1)
}
if (files.length === 0) {
  console.error('dist/ contains no JavaScript or CSS; run `npm run build` first')
  process.exit(1)
}

let total = 0
for (const file of files.sort()) {
  const size = gzipSync(readFileSync(file), { level: 9 }).length
  total += size
  console.log(`${file.slice(distDir.length + 1)}: ${(size / 1024).toFixed(1)} KiB gzipped`)
}
console.log(`total: ${(total / 1024).toFixed(1)} KiB gzipped (budget ${BUDGET_KIB} KiB)`)

if (total > BUDGET_KIB * 1024) {
  console.error('bundle exceeds the 100 KiB gzipped budget')
  process.exit(1)
}
