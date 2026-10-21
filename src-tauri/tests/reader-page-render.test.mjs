import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';
import { test } from 'node:test';
import assert from 'node:assert/strict';

const script = readFileSync(new URL('../src/reader_page_render.js', import.meta.url), 'utf8')
  .replace('__MMK_EXPECTED_ASIN__', JSON.stringify('B000000001'));
function fixture({ href = 'https://read.amazon.co.jp/?asin=B000000001' } = {}) {
  const frames = [];
  let callback;
  const state = { loading: false, animations: [], canvas: false, imageCount: 1, decodeCalls: 0, domReady: 'complete' };
  const box = { x: 0, y: 0, left: 0, top: 0, right: 600, bottom: 900, width: 600, height: 900 };
  const image = { parentElement: null, complete: true, naturalWidth: 1200, naturalHeight: 1800,
    currentSrc: 'blob:test-page-1', src: 'blob:test-page-1',
    getClientRects: () => [{}], getBoundingClientRect: () => box,
    decode: () => { state.decodeCalls++; return state.decode ? state.decode() : Promise.resolve(); },
  };
  const loader = { parentElement: null, get childElementCount() { return state.loading ? 1 : 0; }, getAttribute: () => null, getClientRects: () => [{}], getBoundingClientRect: () => box };
  const renderer = { querySelectorAll: selector => selector === 'img' ? Array(state.imageCount).fill(image) : state.canvas ? [image] : [], getAnimations: () => state.animations };
  const window = {};
  state.renderer = renderer;
  const document = { get readyState() { return state.domReady; }, getElementById: () => state.renderer, querySelectorAll: () => [loader] };
  const context = { URL, location: new URL(href), innerWidth: 600, innerHeight: 900, window, document,
    getComputedStyle: () => ({ display: 'block', visibility: 'visible', opacity: '1', backgroundImage: 'none', content: 'none' }),
    MutationObserver: class { constructor(cb) { callback = cb; } observe() {} disconnect() {} },
    requestAnimationFrame: cb => frames.push(cb),
  };
  const poll = () => JSON.parse(JSON.stringify(runInNewContext(script, context)));
  // Drain cross-realm Promise adoption before advancing a simulated frame.
  const tick = () => new Promise(resolve => setImmediate(resolve));
  const paint = async () => { await tick(); const callbacks = frames.splice(0); callbacks.forEach(cb => cb()); await tick(); };
  const ready = async () => { poll(); await paint(); await paint(); return poll(); };
  return { state, image, box, poll, paint, ready, tick, mutation: (attributeName = 'src') => callback([{ type: 'attributes', attributeName }]) };
}

test('waits for image decode and two painted animation frames, not a fixed duration', async () => {
  const f = fixture();
  assert.equal(f.poll().ready, false);
  await f.paint();
  assert.equal(f.poll().ready, false);
  await f.paint();
  assert.equal(f.poll().ready, true);
  assert.equal(f.state.decodeCalls, 1);
});

test('reports why readiness is blocked without returning content or URLs', async () => {
  for (const [blocked, expected] of [
    ['document', 'document'], ['loader', 'loading-indicator'],
    ['animation', 'animation'], ['multiple', 'image-count'], ['incomplete', 'image-load'],
  ]) {
    const f = fixture();
    if (blocked === 'document') f.state.domReady = 'loading';
    if (blocked === 'loader') f.state.loading = true;
    if (blocked === 'animation') f.state.animations = [{ playState: 'running' }];
    if (blocked === 'multiple') f.state.imageCount = 2;
    if (blocked === 'incomplete') f.image.complete = false;
    const result = f.poll();
    assert.equal(result.waitingFor, expected, blocked);
    assert.doesNotMatch(JSON.stringify(result), /blob:|test-page/);
  }
  const f = fixture();
  assert.equal(f.poll().waitingFor, 'decode');
  await f.paint();
  assert.equal(f.poll().waitingFor, 'paint');
  await f.paint();
  assert.equal(f.poll().waitingFor, null);
});

test('does not accept complete=false, broken images, a loader, or a running transition', async () => {
  for (const blocked of ['incomplete', 'broken', 'loader', 'animation', 'multiple']) {
    const f = fixture();
    if (blocked === 'incomplete') f.image.complete = false;
    if (blocked === 'broken') f.image.naturalWidth = 0;
    if (blocked === 'loader') f.state.loading = true;
    if (blocked === 'animation') f.state.animations = [{ playState: 'running' }];
    if (blocked === 'multiple') f.state.imageCount = 2;
    assert.equal((await f.ready()).ready, false, blocked);
    assert.equal(f.state.decodeCalls, 0, blocked);
  }
});

test('a pending decode cannot become ready until its promise resolves', async () => {
  const f = fixture(); let finish;
  f.state.decode = () => new Promise(resolve => { finish = resolve; });
  f.poll(); await f.paint(); await f.paint();
  assert.equal(f.poll().ready, false);
  finish(); await f.paint(); await f.paint();
  assert.equal(f.poll().ready, true);
});

test('page replacement invalidates the previous token even at identical dimensions', async () => {
  const f = fixture();
  const first = await f.ready();
  f.image.currentSrc = 'blob:test-page-2';
  f.mutation();
  const loading = f.poll();
  assert.notEqual(loading.token, first.token);
  assert.equal(loading.ready, false);
  assert.equal((await f.ready()).ready, true);
});

test('the same image URL assigned again still records a new page generation', async () => {
  const f = fixture(); const first = await f.ready();
  f.mutation();
  const next = await f.ready();
  assert.equal(next.ready, true);
  assert.notEqual(next.token, first.token);
});

test('replacing the entire reader renderer preserves monotonically increasing page tokens', async () => {
  const f = fixture(); const first = await f.ready();
  f.state.renderer = { ...f.state.renderer };
  const second = await f.ready();
  assert.equal(second.ready, true);
  assert.ok(second.token > first.token);
});

test('late decode of an old image cannot certify a newer image', async () => {
  const f = fixture(); let oldFinish;
  f.state.decode = () => new Promise(resolve => { oldFinish = resolve; });
  f.poll();
  f.image.currentSrc = 'blob:replacement'; f.mutation(); f.image.complete = false;
  f.poll(); oldFinish(); await f.paint(); await f.paint();
  assert.equal(f.poll().ready, false);
});

test('detects a transition that starts after decode but before the paint check', async () => {
  const f = fixture(); f.poll(); await f.paint();
  f.state.animations = [{ playState: 'running' }];
  await f.paint();
  assert.equal(f.poll().ready, false);
  f.state.animations = [];
  assert.equal((await f.ready()).ready, true);
});

test('does not return source URLs or read image bytes', async () => {
  const f = fixture();
  const result = await f.ready();
  assert.deepEqual(Object.keys(result).sort(), ['imageCount', 'ready', 'token', 'waitingFor']);
  assert.doesNotMatch(JSON.stringify(result), /blob:|test-page/);
});

test('fails closed for other books, login screens, insecure URLs, or canvas-only content', () => {
  for (const href of ['https://read.amazon.co.jp/?asin=B000000002', 'https://www.amazon.co.jp/ap/signin', 'http://read.amazon.co.jp/?asin=B000000001']) {
    assert.equal(fixture({ href }).poll().error, 'wrong-book');
  }
  const f = fixture(); f.state.imageCount = 0; f.state.canvas = true;
  assert.equal(f.poll().error, 'canvas-not-supported');
});
