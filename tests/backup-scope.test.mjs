import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import ts from 'typescript';

const source = readFileSync(new URL('../src/domain.ts', import.meta.url), 'utf8');
const { outputText } = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } });
const { booksInLibraryView, groupBooks, seriesKey } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`);
const books = [
  { id: 1, title: 'A 1', effectiveSeriesName: 'シリーズ A', effectiveSeriesIndex: 1 },
  { id: 2, title: 'A 2', effectiveSeriesName: 'シリーズ A', effectiveSeriesIndex: 2 },
  { id: 3, title: 'B 1', effectiveSeriesName: 'シリーズ B', effectiveSeriesIndex: 1 },
  { id: 4, title: '単巻', effectiveSeriesName: null, effectiveSeriesIndex: null },
];
const ids = values => values.map(book => book.id);

test('unfiltered library includes singles and all volumes represented by series cards', () => {
  assert.equal(groupBooks(books).singles.length, 2);
  assert.deepEqual(ids(booksInLibraryView(books, null)), [1, 2, 3, 4]);
});

test('search and other filters limit candidates without restoring excluded volumes', () => {
  const filtered = [books[1], books[3]];
  assert.deepEqual(ids(booksInLibraryView(filtered, null)), [2, 4]);
});

test('an open series includes only that series within the current filters', () => {
  const series = seriesKey('シリーズＡ');
  assert.deepEqual(ids(booksInLibraryView(books, series)), [1, 2]);
  assert.deepEqual(ids(booksInLibraryView([books[1], books[2]], series)), [2]);
});

test('empty search and series results never fall back to the entire library', () => {
  assert.deepEqual(booksInLibraryView([], null), []);
  assert.deepEqual(booksInLibraryView([], seriesKey('シリーズ A')), []);
  assert.deepEqual(booksInLibraryView([books[2], books[3]], seriesKey('シリーズ A')), []);
});
