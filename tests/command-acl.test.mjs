import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { test } from 'node:test';

const read = path => readFileSync(new URL(path, import.meta.url), 'utf8');
const manifest = new Set([...read('../src-tauri/build.rs').matchAll(/"([a-z_]+)"/g)].map(match => match[1]));
const handler = read('../src-tauri/src/lib.rs').match(/tauri::generate_handler!\[([\s\S]*?)\]/)?.[1];
const registered = new Set(handler?.split(',').map(command => command.trim()).filter(Boolean));
const main = JSON.parse(read('../src-tauri/capabilities/default.json'));
const permission = command => `allow-${command.replaceAll('_', '-')}`;
const backupCommands = ['get_backup_settings', 'save_backup_settings', 'list_pending_backups'];

test('every registered command is included in the build-time ACL manifest', () => {
  assert.ok(handler, 'invoke handler was not found');
  assert.deepEqual([...manifest].sort(), [...registered].sort());
});

test('every frontend API command has a main-window permission', () => {
  const commands = [...read('../src/api.ts').matchAll(/invoke(?:<[^;\n]*?>)?\("([a-z_]+)"/g)].map(match => match[1]);
  assert.ok(commands.length > 0, 'frontend commands were not found');
  for (const command of commands) {
    assert.ok(manifest.has(command), `${command} is missing from build.rs`);
    assert.ok(main.permissions.includes(permission(command)), `${command} is missing from the main-window capability`);
  }
});

test('backup commands are granted only to the local main window', () => {
  assert.deepEqual(main.windows, ['main']);
  assert.notEqual(main.local, false);
  assert.equal(main.remote, undefined);
  for (const command of backupCommands) assert.ok(main.permissions.includes(permission(command)));
  for (const name of readdirSync(new URL('../src-tauri/capabilities/', import.meta.url))) {
    if (!name.endsWith('.json') || name === 'default.json') continue;
    const capability = JSON.parse(read(`../src-tauri/capabilities/${name}`));
    for (const command of backupCommands) assert.ok(!capability.permissions.includes(permission(command)), `${name} must not grant ${command}`);
  }
});
