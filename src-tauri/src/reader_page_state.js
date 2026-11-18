// Classify what the capture reader is currently showing, as a coarse enum
// only (no URLs or text leave the page). "amazon_error" is Amazon's generic
// error page (the "dogs of Amazon" 404): an exact reader/amazon host, no
// reader DOM, and one of a few distinctive visible-text markers.
(() => {
  const hosts = ['read.amazon.co.jp', 'www.amazon.co.jp', 'amazon.co.jp'];
  if (!hosts.includes(location.hostname)) return 'other';
  if (document.getElementById('kr-renderer')) return 'reader';
  const text = (document.body?.innerText || '').toLowerCase().replace(/’/g, "'");
  if (text.includes("couldn't find that page") || text.includes('dogs of amazon') || text.includes('お探しのページは見つかりません')) return 'amazon_error';
  if (location.hostname !== 'read.amazon.co.jp') return 'amazon_other';
  // On the reader host with the document complete but no reader DOM and no
  // known error text, this is some other terminal page (login, denial,
  // an unrecognized error), not a page still loading.
  return document.readyState === 'complete' ? 'amazon_other' : 'loading';
})()
