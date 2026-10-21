import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';
import { test } from 'node:test';
import assert from 'node:assert/strict';

const script = readFileSync(new URL('../src/reader_layout.js', import.meta.url), 'utf8');
const spreadScript = script.replaceAll('__MMK_ALLOW_SPREAD__', 'true');
const defaultStyle = { visibility: 'visible', display: 'block', opacity: '1', objectFit: 'fill', objectPosition: '50% 50%', overflowX: 'visible', overflowY: 'visible' };
const node = (name, x, y, width, height, extra = {}) => ({
  localName: name, parentElement: null, clientLeft: 0, clientTop: 0,
  clientWidth: width, clientHeight: height, naturalWidth: width, naturalHeight: height,
  complete: true, getBoundingClientRect: () => ({ left: x, top: y, width, height }),
  getClientRects: () => [{}], closest: () => null, ...extra,
});
const root = nodes => ({ querySelectorAll: () => nodes });
const windowFor = (width, height) => ({ innerWidth: width, innerHeight: height, getComputedStyle: e => ({ ...defaultStyle, ...e.style }) });
const measure = (nodes, source = script) => JSON.parse(JSON.stringify(runInNewContext(source, {
  location: { protocol: 'https:', hostname: 'read.amazon.co.jp' },
  innerWidth: 600, innerHeight: 900, window: windowFor(600, 900), document: root(nodes),
})));

test('keeps the whole portrait page, including its white margins', () => {
  const result = measure([node('img', 60, 90, 480, 720)]);
  assert.deepEqual(result.rect, { x: 60, y: 90, width: 480, height: 720, kind: 'img' });
});
test('accounts for object-fit contain letterboxing', () => {
  const result = measure([node('img', 0, 0, 600, 900, {
    naturalWidth: 600, naturalHeight: 1000, style: { objectFit: 'contain' },
  })]);
  assert.deepEqual(result.rect, { x: 30, y: 0, width: 540, height: 900, kind: 'img' });
});
test('rejects two side-by-side pages rather than choosing one half', () => {
  assert.equal(measure([node('img', 25, 200, 260, 390), node('img', 315, 200, 260, 390)]).error, 'spread');
});
test('spread mode captures two side-by-side pages as one landscape rect', () => {
  const result = measure([node('img', 25, 200, 260, 390), node('img', 315, 200, 260, 390)], spreadScript);
  assert.deepEqual(result.rect, { x: 25, y: 200, width: 550, height: 390, kind: 'img' });
});
test('spread mode accepts a single landscape image and rejects excessive widths', () => {
  assert.deepEqual(measure([node('img', 10, 200, 580, 400)], spreadScript).rect, { x: 10, y: 200, width: 580, height: 400, kind: 'img' });
  assert.equal(measure([node('img', 0, 100, 900, 300)], spreadScript).error, 'not-found');
});
test('rejects a landscape spread image', () => {
  assert.equal(measure([node('img', 10, 200, 580, 400)]).error, 'spread');
});
test('ignores hidden and clipped pre-rendered pages', () => {
  const parent = node('div', 0, 0, 600, 400, { style: { overflowY: 'hidden' } });
  const result = measure([
    node('img', 60, 90, 480, 720, { style: { opacity: '0' } }),
    node('img', 60, 90, 480, 720, { parentElement: parent }),
  ]);
  assert.equal(result.error, 'not-found');
});
test('maps same-origin iframe geometry into the top-level viewport', () => {
  const frame = node('iframe', 0, 0, 600, 900, {
    contentWindow: windowFor(300, 450), contentDocument: root([node('img', 30, 45, 240, 360)]),
  });
  assert.deepEqual(measure([frame]).rect, { x: 60, y: 90, width: 480, height: 720, kind: 'img' });
});
test('inspects open shadow roots', () => {
  const host = node('div', 0, 0, 600, 900, { shadowRoot: root([node('canvas', 60, 90, 480, 720)]) });
  assert.equal(measure([host]).rect.kind, 'canvas');
});
test('does not mistake a viewport-sized canvas for a page boundary', () => {
  assert.equal(measure([node('canvas', 0, 0, 600, 900)]).error, 'not-found');
});
test('accepts viewport-sized images without requiring visible gutters', () => {
  assert.deepEqual(measure([node('img', 0, 0, 600, 900)]).rect, { x: 0, y: 0, width: 600, height: 900, kind: 'img' });
});
test('accepts full-window canvas only with an explicit page container', () => {
  assert.equal(measure([node('canvas', 0, 0, 600, 900, { closest: selector => selector.includes('[data-page-number]') ? {} : null })]).rect.kind, 'canvas');
});
test('keeps the real Kindle manga image even when its visible parent is aria-hidden', () => {
  const page = node('img', 60, 0, 480, 900, {
    closest: selector => selector.includes('[aria-hidden="true"]') ? {} : null,
  });
  assert.deepEqual(measure([page]).rect, {x:60,y:0,width:480,height:900,kind:'img'});
});
