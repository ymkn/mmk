import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import ts from 'typescript';

const source = readFileSync(new URL('../src/filterSettings.ts', import.meta.url), 'utf8');
const { outputText } = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } });
const { readFilterSections, writeFilterSections, FILTER_SECTIONS_STORAGE_KEY: KEY, DEFAULT_FILTER_SECTIONS } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`);
function storage(raw = null) {
  const values = new Map([[KEY, raw]]);
  return { getItem: key => values.get(key) ?? null, setItem: (key, value) => values.set(key, value) };
}

test('tags are expanded by default and less-used sections are collapsed', () => {
  assert.deepEqual(readFilterSections(storage()), { tags: true, rating: false, authors: false, publishers: false, labels: false });
});

test('every section restores its independently saved open or closed state', () => {
  const s = storage();
  const sections = { tags: false, rating: true, authors: true, publishers: false, labels: true };
  writeFilterSections(sections, s);
  assert.deepEqual(readFilterSections(s), sections);
  writeFilterSections(DEFAULT_FILTER_SECTIONS, s);
  assert.deepEqual(readFilterSections(s), DEFAULT_FILTER_SECTIONS);
});

test('partial and invalid settings cannot replace defaults with non-boolean values', () => {
  assert.deepEqual(readFilterSections(storage(JSON.stringify({ tags: false, authors: true, rating: 'false', labels: 1 }))), {
    ...DEFAULT_FILTER_SECTIONS, tags: false, authors: true,
  });
  for (const raw of ['{broken', 'null', '42', '[]']) assert.deepEqual(readFilterSections(storage(raw)), DEFAULT_FILTER_SECTIONS);
});

test('inaccessible storage does not prevent the library from opening', () => {
  const blocked = { getItem() { throw Error('blocked'); }, setItem() { throw Error('blocked'); } };
  assert.deepEqual(readFilterSections(blocked), DEFAULT_FILTER_SECTIONS);
  assert.doesNotThrow(() => writeFilterSections(DEFAULT_FILTER_SECTIONS, blocked));
});
