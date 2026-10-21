import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import ts from 'typescript';

const source = readFileSync(new URL('../src/domain.ts', import.meta.url), 'utf8');
const { outputText } = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } });
const { matchesRatingFilters } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`);

test('series favorites include volumes while retaining individual favorites', () => {
  assert.ok(matchesRatingFilters({ score: 0, favorite: false }, { score: 0, favorite: true }, true, 'all'));
  assert.ok(matchesRatingFilters({ score: 0, favorite: true }, undefined, true, 'all'));
  assert.equal(matchesRatingFilters({ score: 0, favorite: false }, { score: 0, favorite: false }, true, 'all'), false);
});

test('score filters accept individual or series scores without overwriting either', () => {
  const book = { score: -2, favorite: false };
  const series = { score: 3, favorite: true };
  assert.ok(matchesRatingFilters(book, series, false, 'positive'));
  assert.ok(matchesRatingFilters(book, series, false, 'negative'));
  assert.equal(book.score, -2);
  assert.equal(series.score, 3);
  assert.ok(matchesRatingFilters({ score: 1, favorite: true }, undefined, true, 'positive'));
  assert.equal(matchesRatingFilters({ score: 0, favorite: false }, undefined, false, 'negative'), false);
});

test('favorite and score filters are intersected and missing series metadata defaults to neutral', () => {
  assert.equal(matchesRatingFilters({ score: 2, favorite: false }, undefined, true, 'positive'), false);
  assert.equal(matchesRatingFilters({ score: 0, favorite: true }, undefined, true, 'positive'), false);
  assert.ok(matchesRatingFilters({ score: 0, favorite: false }, undefined, false, 'all'));
});
