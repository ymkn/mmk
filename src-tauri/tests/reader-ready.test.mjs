import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';
import { test } from 'node:test';
import assert from 'node:assert/strict';

const template = readFileSync(new URL('../src/reader_ready.js', import.meta.url), 'utf8');
function ready({ href = 'https://read.amazon.co.jp/?asin=B000000001', state = 'complete', renderer = true, interaction = true, layout = { rect: { width: 600, height: 900 } } } = {}) {
  const script = template.replace('__MMK_EXPECTED_ASIN__', JSON.stringify('B000000001'))
    .replace('__MMK_READER_LAYOUT__', `(${JSON.stringify(layout)})`);
  return runInNewContext(script, {
    URL, location: new URL(href),
    document: { readyState: state, getElementById: () => renderer, querySelector: () => interaction },
  });
}
test('waits for the requested book document and a rendered page', () => {
  assert.equal(ready(), true);
  assert.equal(ready({ href: 'https://read.amazon.co.jp/manga/B000000001' }), true);
  assert.equal(ready({ href: 'https://read.amazon.co.jp/?asin=B000000002' }), false);
  assert.equal(ready({ state: 'loading' }), false);
  assert.equal(ready({ renderer: false }), false);
  assert.equal(ready({ interaction: false }), false);
  assert.equal(ready({ layout: { error: 'not-found' } }), false);
  assert.equal(ready({ layout: { rect: {}, error: 'spread' } }), false);
});
test('does not accept login pages, other origins, or insecure pages', () => {
  for (const href of ['https://www.amazon.co.jp/ap/signin', 'https://example.com/?asin=B000000001', 'http://read.amazon.co.jp/?asin=B000000001']) {
    assert.equal(ready({ href }), false);
  }
});
test('the actual page-layout script can be embedded without syntax errors', () => {
  const layout = readFileSync(new URL('../src/reader_layout.js', import.meta.url), 'utf8');
  const script = template.replace('__MMK_EXPECTED_ASIN__', JSON.stringify('B000000001')).replace('__MMK_READER_LAYOUT__', layout);
  const result = runInNewContext(script, {
    URL, location: new URL('https://read.amazon.co.jp/?asin=B000000001'), innerWidth: 600, innerHeight: 900,
    document: { readyState: 'complete', getElementById: () => ({}), querySelector: () => ({}), querySelectorAll: () => [] },
    window: {},
  });
  assert.equal(result, false);
});
