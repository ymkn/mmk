// Confirm the loaded document and rendered page before starting the next book.
// The navigation URL alone may update while the previous document is still visible.
(() => {
  const expected = __MMK_EXPECTED_ASIN__;
  const asin = new URL(location.href).searchParams.get('asin') || location.pathname.match(/^\/manga\/([^/]+)/)?.[1];
  if (location.protocol !== 'https:' || location.hostname !== 'read.amazon.co.jp' || asin?.toUpperCase() !== expected.toUpperCase() || document.readyState !== 'complete') return false;
  if (!document.getElementById('kr-renderer') || !document.querySelector('.kr-interaction-layer-fullpage')) return false;
  const layout = __MMK_READER_LAYOUT__;
  return Boolean(layout.rect && !layout.error);
})()
