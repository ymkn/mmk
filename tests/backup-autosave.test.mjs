import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import ts from 'typescript';

const source = readFileSync(new URL('../src/backupSettingsStore.ts', import.meta.url), 'utf8');
const { outputText } = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } });
const { createBackupSettingsStore } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`);
const initial = { pageLayout: 'single', configPath: 'test.json' };
function deferred() {
  let resolve;
  const promise = new Promise(done => { resolve = done; });
  return { promise, resolve };
}

test('rapid changes save in order, and reopening waits for the latest write', async () => {
  const events = [];
  const first = deferred();
  let stored = { ...initial };
  const store = createBackupSettingsStore(async () => { events.push('read'); return { ...stored }; }, async value => {
    events.push(`start:${value.pageLayout}`);
    if (value.pageLayout === 'spread') await first.promise;
    stored = { ...value };
    events.push(`saved:${value.pageLayout}`);
  });
  const save1 = store.save({ ...initial, pageLayout: 'spread' });
  const save2 = store.save(initial);
  const reopened = store.load();
  await Promise.resolve();
  assert.deepEqual(events, ['start:spread']);
  first.resolve();
  await Promise.all([save1, save2]);
  assert.deepEqual(await reopened, initial);
  assert.deepEqual(events, ['start:spread', 'saved:spread', 'start:single', 'saved:single', 'read']);
});

test('backup start runs only after queued settings writes finish', async () => {
  const write = deferred();
  const events = [];
  const store = createBackupSettingsStore(async () => initial, async () => {
    await write.promise;
    events.push('saved');
  });
  const saving = store.save(initial);
  const starting = store.afterPending(async () => { events.push('started'); });
  await Promise.resolve();
  assert.deepEqual(events, []);
  write.resolve();
  await Promise.all([saving, starting]);
  assert.deepEqual(events, ['saved', 'started']);
});

test('failed writes report the error without preventing later saves or reads', async () => {
  let attempts = 0;
  let stored = { ...initial };
  const store = createBackupSettingsStore(async () => stored, async value => {
    if (++attempts === 1) throw Error('disk error');
    stored = value;
  });
  const failed = store.save({ ...initial, pageLayout: 'spread' });
  const retry = store.save(initial);
  await assert.rejects(failed, /disk error/);
  await retry;
  assert.equal((await store.load()).pageLayout, 'single');
});

test('queued settings use a snapshot rather than the callers mutable object', async () => {
  let saved;
  const store = createBackupSettingsStore(async () => initial, async value => { saved = value; });
  const value = { ...initial };
  const pending = store.save(value);
  value.pageLayout = 'spread';
  await pending;
  assert.equal(saved.pageLayout, 'single');
});
