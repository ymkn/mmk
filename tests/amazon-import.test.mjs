import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import vm from 'node:vm';

const rust = readFileSync(new URL('../src-tauri/src/amazon.rs', import.meta.url), 'utf8');
const script = rust.match(/AMAZON_CONTINUE_SERIES_IMPORT_SCRIPT: &str = r#"([\s\S]*?)"#;/)[1].replaceAll('__MMK_CHECK_ID__', '7');

function fixture({ cards, account = null, pathname = '/amz-books/seeMore/', captcha = false, rejectStore = false, pendingStore = false } = {}) {
  const calls = [];
  let interval;
  let cleared = false;
  let redirected;
  const location = { hostname: 'www.amazon.co.jp', origin: 'https://www.amazon.co.jp', pathname, href: `https://www.amazon.co.jp${pathname}`, replace: url => { redirected = url; } };
  const window = {
    location,
    setInterval: fn => { interval = fn; return 1; },
    clearInterval: () => { cleared = true; },
    addEventListener() {},
    __TAURI_INTERNALS__: { invoke(command, args) {
      calls.push({ command, args });
      if (command === 'store_amazon_continue_series') {
        if (rejectStore) return Promise.reject(new Error('database failed'));
        if (pendingStore) return new Promise(() => {});
      }
      return Promise.resolve();
    } },
  };
  const document = {
    readyState: 'complete',
    scripts: cards === undefined ? [] : [{ textContent: `CBBE_BooksCardsReactAssets_cardsToRender = {"asinList":${JSON.stringify(cards)}}` }],
    querySelector(selector) {
      if (selector === '#nav-link-accountList') return account;
      if (selector.includes('captchacharacters')) return captcha ? {} : null;
      return null;
    },
    querySelectorAll: () => [],
  };
  vm.runInNewContext(script, { window, document, URL });
  return {
    calls, get cleared() { return cleared; }, get redirected() { return redirected; },
    async ticks(count = 1) {
      for (let n = 0; n < count && !cleared; n++) {
        interval?.();
        for (let step = 0; step < 8; step++) await Promise.resolve();
      }
    },
  };
}

test('recognized empty payload is a successful zero-item retrieval', async () => {
  const f = fixture({ cards: [] });
  await f.ticks();
  assert.equal(f.calls.length, 1);
  assert.equal(f.calls[0].command, 'store_amazon_continue_series');
  assert.equal(f.calls[0].args.checkId, 7);
  assert.equal(f.calls[0].args.items.length, 0);
  assert.equal(f.cleared, true);
});

test('missing payload times out explicitly and never overwrites saved candidates with an empty array', async () => {
  const f = fixture();
  await f.ticks(120);
  assert.equal(f.calls.length, 1);
  assert.equal(f.calls[0].command, 'report_amazon_check_error');
  assert.equal(f.calls[0].args.reason, 'unreadable');
  assert.equal(f.cleared, true);
});

test('failed storage is reported rather than treated as completed retrieval', async () => {
  const f = fixture({ cards: [], rejectStore: true });
  await f.ticks();
  assert.equal(f.calls[0].command, 'store_amazon_continue_series');
  assert.equal(f.calls[1].command, 'report_amazon_check_error');
  assert.equal(f.cleared, true);
});

test('pending submission is not sent multiple times', async () => {
  const f = fixture({ cards: [], pendingStore: true });
  await f.ticks(10);
  assert.equal(f.calls.length, 1);
});

test('logged-out page redirects to Amazon sign-in and transfers no recommendations', async () => {
  const f = fixture({ account: { textContent: 'こんにちは、ログイン', getAttribute: () => '/ap/signin?x=1' } });
  await f.ticks();
  assert.match(f.redirected, /^https:\/\/www\.amazon\.co\.jp\/ap\/signin/);
  assert.equal(new URL(f.redirected).searchParams.get('openid.return_to'), 'https://www.amazon.co.jp/amz-books/seeMore/');
  assert.equal(f.calls.length, 0);
});

test('sign-in page shows login with the current check ID', () => {
  const f = fixture({ pathname: '/ap/signin' });
  assert.equal(f.calls[0].command, 'show_amazon_login');
  assert.equal(f.calls[0].args.checkId, 7);
});

test('CAPTCHA is reported as verification required, not a successful empty retrieval', async () => {
  const f = fixture({ captcha: true, cards: [] });
  await f.ticks();
  assert.equal(f.calls.length, 1);
  assert.equal(f.calls[0].args.reason, 'verification');
});

const releasedCard = {
  asin: 'B0HH8M5NFB',
  bookTitle: '剣聖の幼馴染がパワハラで俺につらく当たるので、絶縁して辺境で魔剣士として出直すことにした。（コミック） ： 9 (モンスターコミックス)',
  price: 773,
};

async function importedRating(fields) {
  const f = fixture({ cards: [{ ...releasedCard, ...fields }] });
  await f.ticks();
  const result = f.calls.find(call => call.command === 'store_amazon_continue_series');
  assert.ok(result, 'the card must be imported, not discarded');
  return result.args.items[0].hasRating;
}

test('seeMore rating fields observed on Amazon identify a released volume with reviews', async () => {
  // Field names match the live seeMore payload. Values reproduce the reported
  // volume's product-page rating; no account data or tracking URLs are included.
  assert.equal(await importedRating({
    averageRatingValue: 4.6,
    reviewInfo: {
      reviewCount: 33,
      reviewStarInfo: { alt: '5つ星のうち4.6', hasHalfFilledStar: true, numberOfFilledStar: 4, shortDisplayString: '4.6' },
      reviewUrl: '/product-reviews/B0HH8M5NFB',
    },
  }), true);
});

test('each supported seeMore and storefront field can independently confirm a rating', async () => {
  for (const fields of [
    { averageRatingValue: 4.6 },
    { averageRatingValue: '4.6' },
    { reviewInfo: { reviewCount: 33 } },
    { reviewInfo: { reviewStarInfo: { alt: '5つ星のうち4.6' } } },
    { reviewInfo: { reviewStarInfo: { shortDisplayString: '4.6' } } },
    { reviewInfo: { reviewStarInfo: { numberOfFilledStar: 4 } } },
    { starCountAlt: '5つ星のうち4.6' },
    { starCountShortDisplayString: '4.6' },
    { fullStarCount: 4 },
    { numberReviews: 33 },
    { ratingValue: 4.6 },
    { customerRating: { value: 4.6 } },
    { starRating: '4.6 out of 5 stars' },
  ]) assert.equal(await importedRating(fields), true, JSON.stringify(fields));
});

test('zero, empty, missing or invalid rating data and review links alone are not ratings', async () => {
  for (const fields of [
    {},
    { averageRatingValue: 0, reviewInfo: { reviewCount: 0, reviewStarInfo: { alt: '', numberOfFilledStar: 0, shortDisplayString: '' }, reviewUrl: '/product-reviews/B0HH8M5NFB' } },
    { averageRatingValue: null, reviewInfo: null },
    { ratingValue: 0 },
    { ratingValue: '0' },
    { ratingValue: '評価なし' },
    { ratingValue: '5つ星のうち0' },
    { ratingValue: { value: 0, maxValue: 5 } },
    { ratingValue: -1 },
    { ratingValue: 6 },
    { reviewInfo: { reviewCount: -1 } },
    { reviewInfo: { reviewUrl: '/product-reviews/B0HH8M5NFB' } },
    { fullStarCount: 0, hasHalfStar: false, numberReviews: 0 },
  ]) assert.equal(await importedRating(fields), false, JSON.stringify(fields));
});
