// Generates the TypeScript types the UI compiles against (ADR-0032 decision
// 8): `wright-agent-v1` request/response shapes from this repository's
// committed schema, and `rule-content-v1` nodes from the schema that ships in
// the published workshop-rs crate at the version Cargo.lock pins.
// No request or node type is written by hand.
//
// Usage: npm run generate      (rewrites src/generated/)
// CI fails the job when a regenerated file differs from the committed copy.

import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { gunzipSync } from 'node:zlib';
import { compile } from 'json-schema-to-typescript';

const UI_DIR = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const REPO_ROOT = resolve(UI_DIR, '..');
const GENERATED_DIR = join(UI_DIR, 'src', 'generated');
const BANNER = (source) =>
  `// Generated from ${source} by ui/scripts/generate-types.mjs — do not edit.\n` +
  '/* eslint-disable */\n\n';

// --- workshop-rs at the version Cargo.lock pins ----------------------------

function pinnedWorkshopVersion(lockfileText) {
  const entry = lockfileText.match(
    /\[\[package\]\]\s*\nname = "workshop-rs"\s*\nversion = "([^"]+)"/,
  );
  if (!entry) {
    throw new Error('workshop-rs not found in Cargo.lock');
  }
  return entry[1];
}

// A .crate is a gzipped tar; return the named member's UTF-8 text.
function readTarMember(tar, suffix) {
  let offset = 0;
  while (offset + 512 <= tar.length) {
    const header = tar.subarray(offset, offset + 512);
    const name = header.subarray(0, 100).toString('utf8').replaceAll('\0', '');
    const size = parseInt(header.subarray(124, 136).toString('utf8').trim(), 8);
    if (!name || Number.isNaN(size)) break;
    if (name.endsWith(suffix)) {
      return tar.subarray(offset + 512, offset + 512 + size).toString('utf8');
    }
    offset += 512 + Math.ceil(size / 512) * 512;
  }
  throw new Error(`no tar member ends with ${suffix}`);
}

async function fetchWorkshopRuleContentSchema(version) {
  const url = `https://static.crates.io/crates/workshop-rs/workshop-rs-${version}.crate`;
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`cannot download ${url}: ${response.status}`);
  }
  const tar = gunzipSync(Buffer.from(await response.arrayBuffer()));
  return readTarMember(tar, 'schemas/rule-content-v1.schema.json');
}

// --- schema -> TypeScript --------------------------------------------------

async function emit(name, schema, source) {
  const ts = await compile(schema, name, {
    bannerComment: '',
    unreachableDefinitions: true,
    strictIndexSignatures: true,
  });
  const path = join(GENERATED_DIR, `${name}.ts`);
  await writeFile(path, BANNER(source) + ts);
  return path;
}

const lock = await readFile(join(REPO_ROOT, 'Cargo.lock'), 'utf8');
const version = pinnedWorkshopVersion(lock);
const agentSchema = JSON.parse(
  await readFile(join(REPO_ROOT, 'schemas', 'wright-agent-v1.schema.json'), 'utf8'),
);
// The committed root $ref reaches only the request side; the page also needs
// `ToolResponse` and the result types it references.
delete agentSchema.$ref;
agentSchema.oneOf = [
  { $ref: '#/$defs/ToolRequest' },
  { $ref: '#/$defs/ToolResponse' },
];
const ruleContentSchema = JSON.parse(await fetchWorkshopRuleContentSchema(version));

await mkdir(GENERATED_DIR, { recursive: true });
for (const path of [
  await emit('wright-agent-v1', agentSchema, 'schemas/wright-agent-v1.schema.json'),
  await emit(
    'rule-content-v1',
    ruleContentSchema,
    `workshop-rs ${version} schemas/rule-content-v1.schema.json`,
  ),
]) {
  console.log(`wrote ${path}`);
}
