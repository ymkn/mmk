import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { test } from 'node:test';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import ts from 'typescript';

const source = readFileSync(new URL('../src/CardMetadata.tsx', import.meta.url), 'utf8');
const { outputText } = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX } });
const exports = {};
new Function('require', 'exports', outputText)(createRequire(import.meta.url), exports);
const render = props => renderToStaticMarkup(createElement(exports.CardMetadata, props));
const visibleText = html => html.replace(/<[^>]*>/g, '');

test('series scores show only a nonzero signed number without a label', () => {
  for (const [score, expected] of [[3, '+3'], [-2, '-2']]) {
    const html = render({ score, favorite: false, series: true });
    assert.equal(visibleText(html), expected);
    assert.match(html, /title="シリーズ評価/);
    assert.doesNotMatch(html, /★|☆|<svg/);
  }
});

test('favorites are shown only when registered on book and series cards', () => {
  for (const series of [false, true]) {
    assert.equal(render({ score: 0, favorite: false, series }), '');
    const html = render({ score: 0, favorite: true, series });
    assert.equal(visibleText(html), '★');
    assert.doesNotMatch(html, /title="(?:シリーズ)?評価/);
    assert.match(html, /aria-label="お気に入り"/);
    assert.doesNotMatch(html, /☆/);
  }
});

test('nonempty notes have an accessible icon instead of visible label text', () => {
  const html = render({ score: 1, favorite: true, note: '続刊待ち', series: true });
  assert.match(html, /<svg/);
  assert.match(html, /aria-label="メモあり"/);
  assert.match(html, /title="メモあり"/);
  assert.equal(visibleText(html), '+1★');
  assert.doesNotMatch(html, /続刊待ち/);
});

test('missing, empty, and whitespace-only notes show no icon or label', () => {
  for (const note of [undefined, '', ' \n\t　']) {
    const html = render({ score: 0, favorite: false, note, series: true });
    assert.equal(html, '');
    assert.doesNotMatch(html, /<svg|メモあり/);
  }
});

test('zero scores are hidden while note and favorite indicators remain visible', () => {
  for (const series of [false, true]) {
    const html = render({ score: 0, favorite: true, note: '続刊待ち', series });
    assert.equal(visibleText(html), '★');
    assert.match(html, /aria-label="メモあり"/);
    assert.doesNotMatch(html, /title="(?:シリーズ)?評価/);
  }
  const noteOnly = render({ score: 0, favorite: false, note: 'メモ', series: true });
  assert.equal(visibleText(noteOnly), '');
  assert.match(noteOnly, /<svg/);
});
