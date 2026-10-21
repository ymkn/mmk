// Observe ordinary reader UI only. The reading-position indicator is public UI,
// not Kindle's internal state or book data.
(() => {
  const visible = e => {
    if (!e || !e.getClientRects().length) return false;
    const r = e.getBoundingClientRect();
    if (r.width <= 0 || r.height <= 0 || r.bottom <= 0 || r.top >= innerHeight || r.right <= 0 || r.left >= innerWidth) return false;
    let left = Math.max(0, r.left), top = Math.max(0, r.top);
    let right = Math.min(innerWidth, r.right), bottom = Math.min(innerHeight, r.bottom);
    for (let p = e; p; p = p.parentElement || p.getRootNode?.().host) {
      const s = getComputedStyle(p);
      if (s.display === 'none' || s.visibility !== 'visible' || Number(s.opacity) === 0 || s.contentVisibility === 'hidden') return false;
      // A control's own rectangle can remain onscreen while an ancestor clips
      // it completely. Follow the composed ancestry across Ionic shadow roots.
      if (p !== e) {
        const clipX = /^(hidden|clip|scroll|auto)$/.test(s.overflowX);
        const clipY = /^(hidden|clip|scroll|auto)$/.test(s.overflowY);
        if (clipX || clipY) {
          const bounds = p.getBoundingClientRect();
          if (clipX) { left = Math.max(left, bounds.left); right = Math.min(right, bounds.right); }
          if (clipY) { top = Math.max(top, bounds.top); bottom = Math.min(bottom, bounds.bottom); }
          if (right <= left || bottom <= top) return false;
        }
      }
    }
    return true;
  };
  const renderer = document.getElementById('kr-renderer');
  // A header may contain only the current position while the footer contains
  // the full pair. Inspect every ordinary position indicator, not only the first.
  const markers = [...document.querySelectorAll('ion-title.position, .footer-label.position')];
  const candidates = markers.flatMap(marker => {
    const texts = [marker.textContent, marker.getAttribute?.('aria-label')].filter(Boolean);
    return texts.map(value => {
      const text = value.normalize('NFKC').replace(/[\u200B-\u200D\uFEFF]/gu, '').trim();
      const pair = text.match(/(?:位置|locations?|ページ|pages?)\s*[:：]?\s*([\d,]+)(?:\s*[-–−~〜～]\s*([\d,]+))?\s*(?:\/|of)\s*([\d,]+)/i)
        // A fraction without a prefix is accepted only in these known UI markers.
        || text.match(/^([\d,]+)(?:\s*[-–−~〜～]\s*([\d,]+))?\s*(?:\/|of)\s*([\d,]+)(?:\s|$|[●・(（])/i);
      const single = text.match(/(?:位置|locations?|ページ|pages?)\s*[:：]?\s*([\d,]+)/i);
      const number = value => {
        if (!value || !/^(?:\d+|\d{1,3}(?:,\d{3})+)$/.test(value)) return null;
        const n = Number(value.replaceAll(',', ''));
        return Number.isSafeInteger(n) && n > 0 && n <= 4294967295 ? n : null;
      };
      const current = number(pair?.[1] || single?.[1]);
      const total = number(pair?.[3]);
      // A spread screen shows a start-end range; single pages report no end.
      const end = number(pair?.[2]);
      // A reversed range like 4-2 is invalid and fails closed.
      const invalidRange = end != null && current != null && end < current;
      return {
        text: text.slice(0, 120),
        visible: visible(marker),
        current: invalidRange ? null : current,
        end: !invalidRange && current && end && end >= current ? end : null,
        total: !invalidRange && current && total >= current ? total : null,
      };
    });
  });
  const visibleCandidates = candidates.filter(candidate => candidate.visible);
  // Do not read an offscreen stale value when a visible indicator is available.
  const available = visibleCandidates.length ? visibleCandidates : candidates;
  const full = available.filter(candidate => candidate.current && candidate.total);
  const conflict = full.some(candidate => candidate.current !== full[0].current || candidate.total !== full[0].total);
  const selected = conflict ? null : full[0] || available.find(candidate => candidate.current);
  const toolbar = document.getElementById('top-menu-bar') || document.getElementById('reader-header');
  // Inspect rendered controls, not the stationary header or Ionic host boxes.
  const controls = (root, depth = 0) => {
    if (!root || depth > 8) return [];
    const found = new Set(root.querySelectorAll?.('button, ion-button, [role="button"], a[href]') || []);
    if (root.shadowRoot) for (const button of controls(root.shadowRoot, depth + 1)) found.add(button);
    for (const element of root.querySelectorAll?.('*') || []) {
      if (!element.shadowRoot) continue;
      const rendered = controls(element.shadowRoot, depth + 1);
      if (rendered.length) found.delete(element);
      for (const button of rendered) found.add(button);
    }
    return [...found];
  };
  const toolbarControls = controls(toolbar);
  const visibleControls = toolbarControls.filter(visible);
  const chromeVisible = visibleControls.length > 0;
  const tocCloseVisible = visible(document.querySelector('button.side-menu-close-button'));
  // Ionic moves the close button offscreen before its closing animation ends.
  // The menu's transparent backdrop still intercepts body clicks until
  // show-menu is removed, so the button rectangle alone is not closure evidence.
  const tocOpen = tocCloseVisible || visible(document.querySelector('ion-menu.side-menu.show-menu'));
  return {
    known: Boolean(renderer && document.querySelector('.kr-interaction-layer-fullpage')),
    chromeVisible,
    toolbar: { controlCount: toolbarControls.length, visibleControlCount: visibleControls.length, hostVisible: visible(toolbar) },
    tocOpen,
    tocCloseVisible,
    position: selected?.current ?? null,
    positionEnd: selected?.end ?? null,
    total: selected?.total ?? null,
    positionTexts: available.slice(0, 6).map(candidate => candidate.text),
    centerX: .5,
    centerY: .5,
  };
})()
