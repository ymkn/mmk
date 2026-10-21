import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';
import { test } from 'node:test';
import assert from 'node:assert/strict';

const script = readFileSync(new URL('../src/reader_resume_prompt.js', import.meta.url), 'utf8');
function fixture({ title = '前回読んでいたページ', message = 'あなたは今1にいます。最後の位置は196です。196に移動しますか？', hidden = false, disabled = false, host = 'read.amazon.co.jp' } = {}) {
  const element = (tag, text, parentElement) => ({
    localName: tag, textContent: text, parentElement, disabled: false, clicks: 0,
    matches: selectors => selectors.split(',').some(s => s.trim() === tag),
    getAttribute: () => null, getClientRects: () => [{}],
    getBoundingClientRect: () => ({width:200,height:40}),
    click() { this.clicks++; },
  });
  const modal = element('div', title + message + 'いいえはい');
  modal.hiddenFixture = hidden;
  const heading = element('h2', title, modal);
  const body = element('p', message, modal);
  const no = element('button', 'いいえ', modal);
  no.disabled = disabled;
  const yes = element('button', 'はい', modal);
  const window = {getComputedStyle: e => ({display:e.hiddenFixture ? 'none' : 'block',visibility:'visible',opacity:'1'})};
  const context = {location:{protocol:'https:',hostname:host},window,document:{querySelectorAll:()=>[modal,heading,body,no,yes]}};
  return {no,yes,window,run:()=>runInNewContext(script,context)};
}
test('declines the real Japanese resume-position prompt without jumping to the synced position', () => {
  const f = fixture();
  assert.equal(f.run(), 'dismissed');
  assert.equal(f.no.clicks, 1);
  assert.equal(f.yes.clicks, 0);
  assert.equal(f.window[Symbol.for('mmk.capture.resume-guard')].dismissed, 1);
});
test('does not click arbitrary confirmation dialogs', () => {
  const f = fixture({title:'確認',message:'削除しますか？'});
  assert.equal(f.run(), 'absent');
  assert.equal(f.no.clicks, 0);
  assert.equal(f.yes.clicks, 0);
});
test('requires both the specific title and reading-position question', () => {
  const f = fixture({message:'この操作を続けますか？'});
  assert.equal(f.run(), 'absent');
  assert.equal(f.no.clicks, 0);
});
test('ignores hidden and disabled prompt controls', () => {
  for (const options of [{hidden:true},{disabled:true}]) {
    const f = fixture(options);
    assert.equal(f.run(), 'absent');
    assert.equal(f.no.clicks, 0);
  }
});
test('does not operate sign-in or store pages', () => {
  const f = fixture({host:'www.amazon.co.jp'});
  assert.equal(f.run(), 'absent');
  assert.equal(f.no.clicks, 0);
});
test('does not repeatedly click a prompt while dismissal is pending', () => {
  const f = fixture();
  assert.equal(f.run(), 'dismissed');
  assert.equal(f.run(), 'pending');
  assert.equal(f.no.clicks, 1);
});
