import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import ts from 'typescript';

async function loadModule(path) {
  const source = readFileSync(new URL(path, import.meta.url), 'utf8');
  const { outputText } = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } });
  return import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`);
}
const { matchesNoteFilter, matchesRatingFilters, booksInLibraryView, groupBooks, seriesKey } = await loadModule('../src/domain.ts');
const { createSeriesPreferencesStore } = await loadModule('../src/seriesPreferencesStore.ts');
const input = (note, key = 'series') => ({ key, scoreDelta: 0, favorite: null, note });
const initial = { key: 'series', score: 2, favorite: true, note: '' };
function deferred() {
  let resolve;
  const promise = new Promise(done => { resolve = done; });
  return { promise, resolve };
}

test('note filters handle missing metadata, empty notes, whitespace, and Japanese text', () => {
  for (const preferences of [undefined, { note: '' }, { note: ' \n\t　' }]) {
    assert.ok(matchesNoteFilter(preferences, 'all'));
    assert.ok(matchesNoteFilter(preferences, 'without-note'));
    assert.equal(matchesNoteFilter(preferences, 'with-note'), false);
  }
  for (const note of ['宇宙を巡る冒険もの', ' 読了\n続刊待ち ']) {
    assert.ok(matchesNoteFilter({ note }, 'all'));
    assert.ok(matchesNoteFilter({ note }, 'with-note'));
    assert.equal(matchesNoteFilter({ note }, 'without-note'), false);
  }
});

test('series notes include every volume and note-less books remain in the without-note view', () => {
  const key = seriesKey('星巡り');
  const preferences = new Map([[key, { ...initial, key, note: '続刊待ち' }]]);
  const books = [
    { id: 1, title: '星巡り 1', effectiveSeriesName: '星巡り', effectiveSeriesIndex: 1 },
    { id: 2, title: '星巡り 2', effectiveSeriesName: '星巡り', effectiveSeriesIndex: 2 },
    { id: 3, title: '別シリーズ', effectiveSeriesName: '別シリーズ' },
    { id: 4, title: '単巻', effectiveSeriesName: null },
  ];
  const filter = mode => books.filter(book => matchesNoteFilter(preferences.get(seriesKey(book.effectiveSeriesName ?? '')), mode));
  const withNote = filter('with-note');
  assert.deepEqual(withNote.map(book => book.id), [1, 2]);
  assert.equal(groupBooks(withNote).series.length, 1);
  assert.deepEqual(booksInLibraryView(withNote, key).map(book => book.id), [1, 2]);
  assert.deepEqual(filter('without-note').map(book => book.id), [3, 4]);
  assert.deepEqual(filter('all'), books);
  preferences.get(key).note = '';
  assert.deepEqual(filter('with-note'), []);
});

test('note, favorite, and score filters are intersected', () => {
  const book = { score: 0, favorite: false };
  const matches = preferences => matchesNoteFilter(preferences, 'with-note') && matchesRatingFilters(book, preferences, true, 'positive');
  assert.ok(matches({ ...initial, note: 'メモ' }));
  assert.equal(matches(initial), false);
  assert.equal(matches({ ...initial, note: 'メモ', favorite: false }), false);
  assert.equal(matches({ ...initial, note: 'メモ', score: 0 }), false);
});

test('rapid note edits and clearing save in order, and reopening reads the last edit', async () => {
  const first = deferred();
  const events = [];
  let stored = { ...initial };
  const store = createSeriesPreferencesStore(async () => [{ ...stored }], async value => {
    events.push(value.note);
    if (value.note === 'メ') await first.promise;
    stored = { ...stored, note: value.note };
    return { ...stored };
  });
  const saves = ['メ', 'メモ', ''].map(note => store.save(input(note)));
  const reopened = store.load();
  await Promise.resolve();
  assert.deepEqual(events, ['メ']);
  first.resolve();
  await Promise.all(saves);
  assert.deepEqual(events, ['メ', 'メモ', '']);
  assert.deepEqual(await reopened, [initial]);
});

test('rating updates and series renaming wait for pending note saves', async () => {
  const write = deferred();
  const events = [];
  let stored = { ...initial };
  const store = createSeriesPreferencesStore(async () => [stored], async value => {
    if (value.note !== null) {
      await write.promise;
      stored.note = value.note;
      events.push('note');
    }
    stored.score += value.scoreDelta;
    stored.favorite = value.favorite ?? stored.favorite;
    return { ...stored };
  });
  const saving = store.save(input('続刊待ち'));
  const rating = store.save({ ...input(null), scoreDelta: -1, favorite: false });
  const rename = store.afterPending(async () => { events.push('rename'); stored.key = 'renamed'; });
  await Promise.resolve();
  assert.deepEqual(events, []);
  write.resolve();
  await Promise.all([saving, rating, rename]);
  assert.deepEqual(events, ['note', 'rename']);
  assert.deepEqual(await store.load(), [{ key: 'renamed', score: 1, favorite: false, note: '続刊待ち' }]);
});

test('failed note saves do not prevent later edits or reads', async () => {
  let attempts = 0;
  let stored = { ...initial };
  const store = createSeriesPreferencesStore(async () => [stored], async value => {
    if (++attempts === 1) throw Error('disk error');
    stored = { ...stored, key: value.key, note: value.note };
    return stored;
  });
  const failed = store.save(input('途中'));
  const latest = store.save(input('最新'));
  await assert.rejects(failed, /disk error/);
  await latest;
  assert.equal((await store.load())[0].note, '最新');
});

test('queued edits keep snapshots and series keys separate', async () => {
  const saved = [];
  const store = createSeriesPreferencesStore(async () => saved, async value => {
    const result = { ...initial, key: value.key, note: value.note };
    saved.push(result);
    return result;
  });
  const value = input('最初', 'first');
  const first = store.save(value);
  value.note = '書き換え';
  value.key = 'changed';
  const second = store.save(input('別のメモ', 'second'));
  await Promise.all([first, second]);
  assert.deepEqual(saved.map(({ key, note }) => ({ key, note })), [{ key: 'first', note: '最初' }, { key: 'second', note: '別のメモ' }]);
});
