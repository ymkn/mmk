import { readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

function replaceVersion(source, pattern, version, file) {
  if ([...source.matchAll(pattern)].length !== 1) {
    throw new Error(`Expected one MMK version in ${file}`);
  }
  return source.replace(pattern, (_, prefix, suffix) => `${prefix}${version}${suffix}`);
}

export function syncVersions(root = new URL('../', import.meta.url)) {
  const read = file => readFileSync(new URL(file, root), 'utf8');
  const { version } = JSON.parse(read('package.json'));
  if (typeof version !== 'string' || version.trim() !== version || !/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(version)) {
    throw new Error('package.json must contain a semantic version');
  }

  const config = JSON.parse(read('src-tauri/tauri.conf.json'));
  config.version = version;
  // Prepare every update before writing, and change only MMK's package entry.
  const updates = [
    ['src-tauri/tauri.conf.json', `${JSON.stringify(config, null, 2)}\n`],
    ['src-tauri/Cargo.toml', replaceVersion(read('src-tauri/Cargo.toml'), /(^\[package\]\r?\nname = "mmk"\r?\nversion = ")[^"]+(")/gm, version, 'Cargo.toml')],
    ['src-tauri/Cargo.lock', replaceVersion(read('src-tauri/Cargo.lock'), /(^\[\[package\]\]\r?\nname = "mmk"\r?\nversion = ")[^"]+(")/gm, version, 'Cargo.lock')],
  ];
  for (const [file, content] of updates) writeFileSync(new URL(file, root), content);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  syncVersions();
}
