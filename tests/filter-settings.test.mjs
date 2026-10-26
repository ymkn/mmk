import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import ts from 'typescript';

const source = readFileSync(new URL('../src/filterSettings.ts', import.meta.url), 'utf8');
const { outputText } = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } });
const { readFilterSections, writeFilterSections, FILTER_SECTIONS_STORAGE_KEY: KEY, DEFAULT_FILTER_SECTIONS } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`);
const { readLibraryFilters, writeLibraryFilters, LIBRARY_FILTERS_STORAGE_KEY: FILTERS_KEY } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`);
function storage(raw = null) {
  const values = new Map([[KEY, raw]]);
  return { getItem: key => values.get(key) ?? null, setItem: (key, value) => values.set(key, value) };
}

test('tags are expanded by default and less-used sections are collapsed', () => {
  assert.deepEqual(readFilterSections(storage()), { tags: true, rating: false, notes: false, authors: false, publishers: false, labels: false });
});

test('every section restores its independently saved open or closed state', () => {
  const s = storage();
  const sections = { tags: false, rating: true, notes: true, authors: true, publishers: false, labels: true };
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

test('all filter conditions survive saving and reopening, including multiple facets', () => {
  const s = storage();
  const filters = {
    query: ' 星巡り ASIN ',
    favoriteOnly: true,
    scoreFilter: 'negative',
    noteFilter: 'with-note',
    selectedAuthors: new Set(['著者A', '著者B']),
    selectedPublishers: new Set(['出版社A', '出版社B']),
    selectedLabels: new Set(['レーベルA', 'レーベルB']),
    selectedTags: new Set(['読了', '続刊待ち']),
  };
  for (const scoreFilter of ['all', 'positive', 'negative']) {
    for (const noteFilter of ['all', 'with-note', 'without-note']) {
      const selection = { ...filters, scoreFilter, noteFilter };
      writeLibraryFilters(selection, s);
      assert.deepEqual(readLibraryFilters(s), selection);
    }
  }
});

test('latest filter selection replaces the previous one and clearing survives reopening', () => {
  const s = storage();
  const defaults = readLibraryFilters(s);
  writeLibraryFilters({ ...defaults, query: '最初', favoriteOnly: true, selectedTags: new Set(['読了']) }, s);
  const latest = { ...defaults, query: '最後', noteFilter: 'without-note', selectedAuthors: new Set(['著者']) };
  writeLibraryFilters(latest, s);
  assert.deepEqual(readLibraryFilters(s), latest);
  writeLibraryFilters(defaults, s);
  assert.deepEqual(readLibraryFilters(s), defaults);
});

test('filter selections validate each property and discard malformed facet values', () => {
  const s = storage();
  s.setItem(FILTERS_KEY, JSON.stringify({
    query: 42,
    favoriteOnly: 'false',
    scoreFilter: 'unsupported',
    noteFilter: 'invalid',
    selectedAuthors: ['著者', 42, null, '著者', '', '　'],
    selectedPublishers: '出版社',
    selectedLabels: [false, {}, 'レーベル'],
    selectedTags: ['読了', '続刊待ち'],
  }));
  assert.deepEqual(readLibraryFilters(s), {
    ...readLibraryFilters(storage()),
    selectedAuthors: new Set(['著者']),
    selectedLabels: new Set(['レーベル']),
    selectedTags: new Set(['読了', '続刊待ち']),
  });
});

test('missing filter fields default safely and section state stays independent', () => {
  const s = storage();
  const defaults = readLibraryFilters(s);
  s.setItem(FILTERS_KEY, JSON.stringify({ query: '検索', noteFilter: 'with-note' }));
  const filters = { ...defaults, query: '検索', noteFilter: 'with-note' };
  assert.deepEqual(readLibraryFilters(s), filters);
  const sections = { ...DEFAULT_FILTER_SECTIONS, notes: true };
  writeFilterSections(sections, s);
  assert.deepEqual(readLibraryFilters(s), filters);
  writeLibraryFilters(defaults, s);
  assert.deepEqual(readFilterSections(s), sections);
});

test('corrupt or inaccessible filter storage falls back without blocking use', () => {
  const defaults = readLibraryFilters(storage());
  for (const raw of ['{broken', 'null', '42', '[]', '"text"']) {
    const s = storage();
    s.setItem(FILTERS_KEY, raw);
    assert.deepEqual(readLibraryFilters(s), defaults);
  }
  const blocked = { getItem() { throw Error('blocked'); }, setItem() { throw Error('blocked'); } };
  assert.deepEqual(readLibraryFilters(blocked), defaults);
  assert.doesNotThrow(() => writeLibraryFilters(defaults, blocked));
});

test('restored selections are fresh sets, not shared mutable defaults', () => {
  const s = storage();
  const first = readLibraryFilters(s);
  first.selectedAuthors.add('著者');
  assert.equal(first.selectedTags.size, 0);
  assert.equal(readLibraryFilters(s).selectedAuthors.size, 0);
  writeLibraryFilters(first, s);
  const restored = readLibraryFilters(s);
  restored.selectedAuthors.add('別の著者');
  assert.deepEqual(readLibraryFilters(s).selectedAuthors, new Set(['著者']));
});
