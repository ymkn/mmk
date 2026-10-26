import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { test } from 'node:test';
import { syncVersions } from '../scripts/sync-version.mjs';

const read = file => readFileSync(new URL(`../${file}`, import.meta.url), 'utf8');

test('package, Tauri and Rust versions agree', () => {
  const { version } = JSON.parse(read('package.json'));
  assert.match(version, /^\d+\.\d+\.\d+$/);
  assert.equal(JSON.parse(read('src-tauri/tauri.conf.json')).version, version);
  assert.equal(read('src-tauri/Cargo.toml').match(/\[package\]\r?\nname = "mmk"\r?\nversion = "([^"]+)"/)[1], version);
  assert.equal(read('src-tauri/Cargo.lock').match(/\[\[package\]\]\r?\nname = "mmk"\r?\nversion = "([^"]+)"/)[1], version);
});

function fixture(t) {
  const dir = mkdtempSync(join(process.env.LOCALAPPDATA ? join(process.env.LOCALAPPDATA, 'Temp', 'opencode') : tmpdir(), 'mmk-version-'));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  mkdirSync(join(dir, 'src-tauri'));
  const root = pathToFileURL(`${dir}/`);
  for (const file of ['package.json', 'src-tauri/tauri.conf.json', 'src-tauri/Cargo.toml', 'src-tauri/Cargo.lock']) {
    writeFileSync(new URL(file, root), read(file));
  }
  return root;
}

test('version synchronization preserves dependencies and is idempotent', t => {
  const root = fixture(t);
  const lock = readFileSync(new URL('src-tauri/Cargo.lock', root), 'utf8');
  writeFileSync(new URL('package.json', root), JSON.stringify({ version: '1.2.3' }));
  syncVersions(root);
  assert.equal(JSON.parse(readFileSync(new URL('src-tauri/tauri.conf.json', root), 'utf8')).version, '1.2.3');
  assert.match(readFileSync(new URL('src-tauri/Cargo.toml', root), 'utf8'), /name = "mmk"\r?\nversion = "1.2.3"/);
  const updatedLock = readFileSync(new URL('src-tauri/Cargo.lock', root), 'utf8');
  assert.equal(updatedLock, lock.replace(/(name = "mmk"\r?\nversion = ")[^"]+"/, (_, prefix) => `${prefix}1.2.3"`));
  syncVersions(root);
  assert.equal(readFileSync(new URL('src-tauri/Cargo.lock', root), 'utf8'), updatedLock);
});

test('missing MMK lock entry aborts before writing other files', t => {
  const root = fixture(t);
  const config = readFileSync(new URL('src-tauri/tauri.conf.json', root), 'utf8');
  writeFileSync(new URL('package.json', root), JSON.stringify({ version: '1.2.3' }));
  writeFileSync(new URL('src-tauri/Cargo.lock', root), '[[package]]\nname = "another"\nversion = "0.1.0"\n');
  assert.throws(() => syncVersions(root), /Expected one MMK version/);
  assert.equal(readFileSync(new URL('src-tauri/tauri.conf.json', root), 'utf8'), config);
});

test('version synchronization supports Windows line endings', t => {
  const root = fixture(t);
  writeFileSync(new URL('package.json', root), JSON.stringify({ version: '1.2.3-rc.1' }));
  for (const file of ['src-tauri/Cargo.toml', 'src-tauri/Cargo.lock']) {
    const source = readFileSync(new URL(file, root), 'utf8').replace(/\r?\n/g, '\r\n');
    writeFileSync(new URL(file, root), source);
  }
  syncVersions(root);
  for (const file of ['src-tauri/Cargo.toml', 'src-tauri/Cargo.lock']) {
    assert.match(readFileSync(new URL(file, root), 'utf8'), /name = "mmk"\r\nversion = "1.2.3-rc.1"/);
  }
});

test('missing or malformed versions do not change Tauri configuration', t => {
  const root = fixture(t);
  const config = readFileSync(new URL('src-tauri/tauri.conf.json', root), 'utf8');
  for (const version of [undefined, '1.2', 'v1.2.3', '1.2.3\n', 123]) {
    writeFileSync(new URL('package.json', root), JSON.stringify({ version }));
    assert.throws(() => syncVersions(root), /semantic version/);
    assert.equal(readFileSync(new URL('src-tauri/tauri.conf.json', root), 'utf8'), config);
  }
});

test('settings footer uses the running Tauri app version', () => {
  const app = read('src/App.tsx');
  assert.match(app, /import \{ getVersion \} from "@tauri-apps\/api\/app"/);
  assert.match(app, /getVersion\(\)\.then/);
  assert.match(app, /<footer className="settings-version muted"[\s\S]*バージョン \{appVersion\}/);
});
