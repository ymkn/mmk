import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import ts from 'typescript';

const source = readFileSync(new URL('../src/domain.ts', import.meta.url), 'utf8');
const { outputText } = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } });
const { filterLibraryBooks, groupBooks, booksInLibraryView, seriesKey } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`);
const books = [
  { id: 1, title: 'A 2', effectiveSeriesName: 'シリーズ A', effectiveSeriesIndex: 2 },
  { id: 2, title: '単巻', effectiveSeriesName: null },
  { id: 3, title: 'A 1', effectiveSeriesName: 'シリーズＡ', effectiveSeriesIndex: 1 },
  { id: 4, title: 'B 1', effectiveSeriesName: 'シリーズ B', effectiveSeriesIndex: 1 },
  { id: 5, title: '未設定', effectiveSeriesName: '　 ' },
];
const ids = values => values.map(book => book.id);

test('all retains the original books and their order without mutating them', () => {
  const original = [...books];
  assert.equal(filterLibraryBooks(books, 'all'), books);
  assert.deepEqual(ids(filterLibraryBooks(books, 'series')), [1, 3]);
  assert.deepEqual(books, original);
});

test('book and series filters match the existing shelf cards', () => {
  const singles = filterLibraryBooks(books, 'books');
  const series = filterLibraryBooks(books, 'series');
  assert.deepEqual(ids(singles), [2, 4, 5]);
  assert.equal(groupBooks(singles).series.length, 0);
  assert.deepEqual(ids(series), [1, 3]);
  assert.equal(groupBooks(series).series.length, 1);
  assert.equal(groupBooks(series).singles.length, 0);
});

test('other filters are applied first, so one remaining volume is displayed as a book', () => {
  const matching = [books[0], books[1], books[3]];
  assert.deepEqual(filterLibraryBooks(matching, 'books'), matching);
  assert.deepEqual(filterLibraryBooks(matching, 'series'), []);
});

test('bulk backup includes only the selected type and opened series volumes', () => {
  const singles = filterLibraryBooks(books, 'books');
  const series = filterLibraryBooks(books, 'series');
  assert.deepEqual(ids(booksInLibraryView(singles, null)), [2, 4, 5]);
  assert.deepEqual(ids(booksInLibraryView(series, null)), [1, 3]);
  assert.deepEqual(ids(booksInLibraryView(series, seriesKey('シリーズ A'))), [1, 3]);
  assert.deepEqual(booksInLibraryView(series, seriesKey('シリーズ B')), []);
});

test('empty libraries and missing types never fall back to all books', () => {
  for (const filter of ['all', 'books', 'series']) {
    assert.deepEqual(filterLibraryBooks([], filter), []);
  }
  assert.deepEqual(filterLibraryBooks([books[1]], 'series'), []);
  assert.deepEqual(filterLibraryBooks([books[0], books[2]], 'books'), []);
});
