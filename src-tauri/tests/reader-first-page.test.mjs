import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';
import { test } from 'node:test';
import assert from 'node:assert/strict';

const script = readFileSync(new URL('../src/reader_first_page.js', import.meta.url), 'utf8');
const style = { visibility: 'visible', display: 'block', opacity: '1' };
function control(text, extra = {}) {
  const item = {
    localName: 'button', parentElement: null, disabled: false, textContent: text,
    matches: selector => selector.split(',').some(part => {
      const s = part.trim();
      const role = s.match(/^\[role="([^"]+)"\]$/);
      if (role) return item.getAttribute('role') === role[1];
      const match = s.match(/^([\w-]+)(?:\.([\w-]+))?$/);
      return Boolean(match && item.localName === match[1] && (!match[2] || (item.className || '').split(/\s+/).includes(match[2])));
    }),
    getAttribute: name => item.attributes?.[name] ?? (name === 'class' ? item.className ?? null : null), getClientRects: () => [{}],
    getBoundingClientRect: () => ({ top: 20, bottom: 60, width: 100, height: 40 }),
    click: () => { item.clicked = true; }, ...extra,
  };
  return item;
}
function invoke(nodes, host = 'read.amazon.co.jp', options = {}) {
  const window = { innerHeight: 900, getComputedStyle: e => ({ ...style, ...e.style }) };
  return runInNewContext(script.replace('__MMK_READER_START_OPTIONS__', JSON.stringify({ bookTitle: 'テスト漫画 第1巻', menuOpened: false, ...options })), { location: { protocol: 'https:', hostname: host }, window, document: { querySelectorAll: () => nodes } });
}
test('selects the topmost entry of a TOC regardless of its label', () => {
  const scope = toc();
  const cover = control('表紙', { parentElement: scope });
  const second = control('第1話', { parentElement: scope });
  const third = control('第2話', { parentElement: scope });
  assert.equal(invoke([cover, second, third]), 'cover');
  assert.equal(cover.clicked, true);
  assert.equal(second.clicked, undefined);
  assert.equal(third.clicked, undefined);
});
test('selects an unlabeled topmost entry without needing cover-like text', () => {
  const scope = toc();
  const first = control('1', { parentElement: scope });
  const second = control('2', { parentElement: scope });
  assert.equal(invoke([first, second]), 'cover');
  assert.equal(first.clicked, true);
  assert.equal(second.clicked, undefined);
});
test('opens the visible go-to menu when cover is not yet visible', () => {
  const button = control('Go to');
  assert.equal(invoke([button]), 'menu');
  assert.equal(button.clicked, true);
});
test('does not activate disabled or hidden controls', () => {
  assert.equal(invoke([control('表紙', { disabled: true }), control('Cover', { style: { display: 'none' } })]), 'unavailable');
});
test('does not treat book text as an actionable navigation control', () => {
  assert.equal(invoke([control('表紙', { matches: () => false })]), 'unavailable');
});
test('never operates controls outside the reader origin', () => {
  const button = control('Cover');
  assert.equal(invoke([button], 'www.amazon.co.jp'), 'unavailable');
  assert.equal(button.clicked, undefined);
});

const toc = () => ({ id: 'kindleReader_goToMenu', getAttribute: () => null });
test('selects the manga title entry in an already-open TOC', () => {
  const title = control('テスト漫画 第1巻', { parentElement: toc() });
  assert.equal(invoke([title]), 'cover');
  assert.equal(title.clicked, true);
});
test('ignores a matching title button in the reader header', () => {
  const header = control('テスト漫画 第1巻');
  const title = control('テスト漫画 第1巻', { parentElement: toc() });
  assert.equal(invoke([header, title]), 'cover');
  assert.equal(header.clicked, undefined);
  assert.equal(title.clicked, true);
});
test('selects the topmost entry regardless of width and line-break differences', () => {
  const scope = toc();
  const chapter = control('第2章', { parentElement: scope });
  const title = control('テスト漫画\n第１巻', { parentElement: scope, getBoundingClientRect: () => ({ top: 8, bottom: 48, width: 100, height: 40 }) });
  assert.equal(invoke([chapter, title]), 'cover');
  assert.equal(title.clicked, true);
  assert.equal(chapter.clicked, undefined);
});
test('supports a single title-only entry with locally overridden metadata', () => {
  const title = control('Kindle側の書籍名', { parentElement: toc() });
  assert.equal(invoke([title], 'read.amazon.co.jp', { bookTitle: '手動で設定したタイトル' }), 'cover');
  assert.equal(title.clicked, true);
});
test('selects the topmost entry of a multi-chapter TOC', () => {
  const scope = toc();
  const first = control('表紙', { parentElement: scope });
  const second = control('第2章', { parentElement: scope });
  assert.equal(invoke([first, second], 'read.amazon.co.jp', { menuOpened: true }), 'cover');
  assert.equal(first.clicked, true);
  assert.equal(second.clicked, undefined);
});
test('prefers the topmost visible entry over offscreen ones', () => {
  const scope = toc();
  const first = control('第1章', { parentElement: scope });
  const second = control('第2話', { parentElement: scope, getBoundingClientRect: () => ({ top: 1000, bottom: 1040, width: 100, height: 40 }) });
  assert.equal(invoke([first, second], 'read.amazon.co.jp', { menuOpened: true }), 'cover');
  assert.equal(first.clicked, true);
  assert.equal(second.clicked, undefined);
});
test('scrolls a reused TOC back to its actual top before selecting an entry', () => {
  const scope = { ...toc(), scrollTop: 200 };
  const first = control('最初の項目', { parentElement: scope, getBoundingClientRect: () => ({ top: 20 - scope.scrollTop, bottom: 60 - scope.scrollTop, width: 100, height: 40 }) });
  const later = control('後の項目', { parentElement: scope, getBoundingClientRect: () => ({ top: 250 - scope.scrollTop, bottom: 290 - scope.scrollTop, width: 100, height: 40 }) });
  assert.equal(invoke([first, later]), 'menu');
  assert.equal(scope.scrollTop, 0);
  assert.equal(first.clicked, undefined);
  assert.equal(later.clicked, undefined);
  assert.equal(invoke([first, later], 'read.amazon.co.jp', { menuOpened: true }), 'cover');
  assert.equal(first.clicked, true);
  assert.equal(later.clicked, undefined);
});
test('does not toggle the go-to menu closed while waiting for entries', () => {
  const button = control('目次');
  assert.equal(invoke([button], 'read.amazon.co.jp', { menuOpened: true }), 'waiting');
  assert.equal(button.clicked, undefined);
});
test('collapses nested title labels to a single delegated-click entry', () => {
  const scope = toc();
  const row = control('テスト漫画 第1巻', { parentElement: scope });
  const label = control('テスト漫画 第1巻', { localName: 'span', parentElement: row, matches: selector => selector.startsWith('li,') });
  row.contains = element => element === label;
  assert.equal(invoke([row, label]), 'cover');
  assert.equal(label.clicked, true);
  assert.equal(row.clicked, undefined);
});
test('selects the topmost of duplicate title entries', () => {
  const scope = toc();
  const first = control('テスト漫画 第1巻', { parentElement: scope, getBoundingClientRect: () => ({ top: 10, bottom: 50, width: 100, height: 40 }) });
  const second = control('テスト漫画 第1巻', { parentElement: scope });
  assert.equal(invoke([first, second]), 'cover');
  assert.equal(first.clicked, true);
  assert.equal(second.clicked, undefined);
});
test('does not interpolate a book title as JavaScript code', () => {
  const titleText = '\"); throw new Error("injected"); //';
  const title = control(titleText, { parentElement: toc() });
  assert.equal(invoke([title], 'read.amazon.co.jp', { bookTitle: titleText }), 'cover');
  assert.equal(title.clicked, true);
});
test('does not mistake an Ionic TOC toolbar button for an open TOC panel', () => {
  const host = control('', { localName: 'ion-button', attributes: { 'aria-label': '目次' } });
  const button = control('', { attributes: { 'aria-label': '目次' }, getRootNode: () => ({ host }) });
  host.shadowRoot = { querySelectorAll: () => [button] };
  assert.equal(invoke([host]), 'menu');
  assert.equal(button.clicked, true);
});
test('closes the real Kindle side panel after selecting its title entry', () => {
  const scope = { localName: 'ion-list', getAttribute: name => name === 'role' ? 'listbox' : null };
  const title = control('Kindle側のタイトル', { className: 'toc-item-button', parentElement: scope });
  const close = control('Side Panel Close', { className: 'side-menu-close-button', attributes: { 'aria-label': '閉じる' } });
  assert.equal(invoke([title, close]), 'cover');
  assert.equal(title.clicked, true);
  assert.equal(close.clicked, true);
});
