// Read visible element geometry only. Never read image bytes, canvas pixels,
// book text, cookies, or internal Kindle application state.
(() => {
  try {
    if (location.protocol !== 'https:' || location.hostname !== 'read.amazon.co.jp') return { error: 'not-reader' };
    const viewport = { width: innerWidth, height: innerHeight };
    const candidates = [];
    let visited = 0;
    const intersect = (a, b) => {
      const x = Math.max(a.x, b.x), y = Math.max(a.y, b.y);
      return { x, y, width: Math.max(0, Math.min(a.x + a.width, b.x + b.width) - x), height: Math.max(0, Math.min(a.y + a.height, b.y + b.height) - y) };
    };
    const scan = (root, win, offsetX, offsetY, scaleX, scaleY, clip, depth) => {
      if (depth > 8 || visited > 20000) return;
      for (const element of root.querySelectorAll('*')) {
        if (++visited > 20000) break;
        const style = win.getComputedStyle(element);
        if (style.visibility !== 'visible' || style.display === 'none' || Number(style.opacity) === 0) continue;
        const bounds = element.getBoundingClientRect();
        if (bounds.width <= 0 || bounds.height <= 0) continue;
        const rect = { x: offsetX + bounds.left * scaleX, y: offsetY + bounds.top * scaleY, width: bounds.width * scaleX, height: bounds.height * scaleY };
        if (element.shadowRoot) scan(element.shadowRoot, win, offsetX, offsetY, scaleX, scaleY, intersect(clip, rect), depth + 1);
        if (element.localName === 'iframe') {
          try {
            const child = element.contentWindow;
            if (element.contentDocument && child.innerWidth > 0 && child.innerHeight > 0) {
              scan(element.contentDocument, child, rect.x + element.clientLeft * scaleX, rect.y + element.clientTop * scaleY,
                scaleX * element.clientWidth / child.innerWidth, scaleY * element.clientHeight / child.innerHeight, intersect(clip, rect), depth + 1);
            }
          } catch { /* Cross-origin frames are not inspected. */ }
        }
        if (!['img', 'canvas', 'image'].includes(element.localName)) continue;
        // Kindle marks its visible manga image aria-hidden for screen readers.
        // Accessibility visibility is not CSS/visual visibility.
        if (element.closest('button, nav, header, [role="dialog"], [role="menu"]')) continue;
        // Hidden ancestors and clipped/offscreen pre-rendered pages aren't candidates.
        let visible = true;
        let elementClip = clip;
        for (let parent = element.parentElement; parent; parent = parent.parentElement) {
          const ps = win.getComputedStyle(parent);
          if (ps.display === 'none' || ps.visibility !== 'visible' || Number(ps.opacity) === 0) { visible = false; break; }
          if (/(hidden|clip|scroll|auto)/.test(`${ps.overflowX} ${ps.overflowY}`)) {
            const pr = parent.getBoundingClientRect();
            elementClip = intersect(elementClip, { x: offsetX + pr.left * scaleX, y: offsetY + pr.top * scaleY, width: pr.width * scaleX, height: pr.height * scaleY });
          }
        }
        if (!visible || element.getClientRects().length === 0) continue;
        let page = rect;
        const naturalWidth = element.naturalWidth || element.width?.baseVal?.value || element.width;
        const naturalHeight = element.naturalHeight || element.height?.baseVal?.value || element.height;
        if (element.localName === 'img' && (!element.complete || !(naturalWidth > 0 && naturalHeight > 0))) continue;
        if (['contain', 'scale-down'].includes(style.objectFit) && naturalWidth > 0 && naturalHeight > 0) {
          const factor = Math.min(rect.width / naturalWidth, rect.height / naturalHeight, style.objectFit === 'scale-down' ? 1 : Infinity);
          const width = naturalWidth * factor, height = naturalHeight * factor;
          const position = style.objectPosition.split(' ').map(v => v.endsWith('%') ? parseFloat(v) / 100 : NaN);
          if (position.length !== 2 || position.some(v => !Number.isFinite(v))) continue;
          page = { x: rect.x + (rect.width - width) * position[0], y: rect.y + (rect.height - height) * position[1], width, height };
        }
        if (page.width < viewport.width * .18 || page.height < viewport.height * .35) continue;
        const clipped = intersect(page, elementClip);
        if (clipped.width < page.width - 2 || clipped.height < page.height - 2) continue;
        // A viewport-sized canvas doesn't disclose the page rectangle drawn inside it.
        // Leave it to the conservative visible-background detector instead.
        if (element.localName === 'canvas' && page.width > viewport.width * .97 && page.height > viewport.height * .95
            && !element.closest('[data-page-number], [data-page-index], [data-page], .page, [id^="kindleReader_page"]')) continue;
        candidates.push({ ...page, kind: element.localName });
      }
    };
    scan(document, window, 0, 0, 1, 1, { x: 0, y: 0, ...viewport }, 0);
    candidates.sort((a, b) => b.width * b.height - a.width * a.height);
    const allowSpread = typeof __MMK_ALLOW_SPREAD__ === 'boolean' && __MMK_ALLOW_SPREAD__;
    const best = candidates[0];
    if (!best) return { viewport, error: 'not-found' };
    const aspect = best.width / best.height;
    if (!allowSpread && aspect > 1.05) return { viewport, error: 'spread' };
    for (const other of candidates.slice(1)) {
      const overlap = intersect(best, other);
      const common = overlap.width * overlap.height / Math.min(best.width * best.height, other.width * other.height);
      if (common < .1 && other.width * other.height > best.width * best.height * .55
          && Math.abs(other.y - best.y) < best.height * .15) {
        if (!allowSpread) return { viewport, error: 'spread' };
        // Two visible portrait pages side by side: capture the whole spread.
        const left = Math.min(best.x, other.x), right = Math.max(best.x + best.width, other.x + other.width);
        const top = Math.min(best.y, other.y), bottom = Math.max(best.y + best.height, other.y + other.height);
        const spread = { x: left, y: top, width: right - left, height: bottom - top, kind: best.kind };
        return spread.width / spread.height <= 2.4 ? { viewport, rect: spread } : { viewport, error: 'not-found' };
      }
    }
    if (allowSpread) {
      if (aspect < 1.0 || aspect > 2.4) return { viewport, error: 'not-found' };
      return { viewport, rect: best };
    }
    if (aspect < .35 || aspect > .95) return { viewport, error: 'not-found' };
    return { viewport, rect: best };
  } catch { return { error: 'not-found' }; }
})()
