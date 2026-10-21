import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import ts from 'typescript';

const source = readFileSync(new URL('../src/captureSettings.ts', import.meta.url), 'utf8');
const { outputText } = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } });
const { readCaptureRoot, writeCaptureRoot } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`);
const ROOT = 'mmk.capture-root.v1';
const SINGLE = 'mmk.page-capture.v1';
const BATCH = 'mmk.batch-capture.v1';
function storage(entries = []) {
  const values = new Map(entries);
  return { getItem: key => values.get(key) ?? null, setItem: (key, value) => values.set(key, value) };
}

test('one persistent global value overrides both legacy settings, including an explicit default', () => {
  for (const root of ['', 'D:\\Shared']) {
    const s = storage([[ROOT, root], [SINGLE, JSON.stringify({ outputDirectory: 'D:\\Single' })], [BATCH, JSON.stringify({ outputDirectory: 'D:\\Batch' })]]);
    assert.equal(readCaptureRoot(s), root);
  }
});
test('migration prefers the batch custom root and falls back to the single custom root', () => {
  const s = storage([[SINGLE, JSON.stringify({ outputDirectory: 'D:\\Single' })], [BATCH, JSON.stringify({ outputDirectory: 'D:\\Batch' })]]);
  assert.equal(readCaptureRoot(s), 'D:\\Batch');
  s.setItem(BATCH, JSON.stringify({ outputDirectory: '' }));
  assert.equal(readCaptureRoot(s), 'D:\\Single');
  s.setItem(BATCH, '{broken');
  assert.equal(readCaptureRoot(s), 'D:\\Single');
});
test('changing and resetting the global root survives a later read without reviving legacy roots', () => {
  const s = storage([[BATCH, JSON.stringify({ outputDirectory: 'D:\\Old' })]]);
  writeCaptureRoot('E:\\New', s);
  assert.equal(readCaptureRoot(s), 'E:\\New');
  writeCaptureRoot('', s);
  assert.equal(readCaptureRoot(s), '');
});
test('missing, corrupt, and inaccessible settings fall back safely', () => {
  assert.equal(readCaptureRoot(storage()), '');
  assert.equal(readCaptureRoot(storage([[BATCH, 'null'], [SINGLE, '{broken']])), '');
  const blocked = { getItem() { throw Error('blocked'); }, setItem() { throw Error('blocked'); } };
  assert.equal(readCaptureRoot(blocked), '');
  assert.doesNotThrow(() => writeCaptureRoot('D:\\New', blocked));
});
