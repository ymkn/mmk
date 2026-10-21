// Observe only ordinary DOM/image loading and browser paint scheduling.
// Never fetch image URLs, read pixels, or inspect Kindle application state.
((expected) => {
  const allowSpread = typeof __MMK_ALLOW_SPREAD__ === 'boolean' && __MMK_ALLOW_SPREAD__;
  const asin = new URL(location.href).searchParams.get('asin') || location.pathname.match(/^\/manga\/([^/]+)/)?.[1];
  if (location.protocol !== 'https:' || location.hostname !== 'read.amazon.co.jp' || asin?.toUpperCase() !== expected.toUpperCase()) return { ready: false, token: 0, error: 'wrong-book' };
  const renderer = document.getElementById('kr-renderer');
  if (!renderer || document.readyState !== 'complete') return { ready: false, token: 0, waitingFor: 'document', imageCount: 0 };
  const key = '__mmkPageRender_v1';
  let state = window[key];
  if (!state || state.renderer !== renderer) {
    const previousToken = state?.token || 0;
    state?.observer.disconnect();
    state = { renderer, token: previousToken, revision: 0, seenRevision: -1, signature: '', ready: false, pending: false, phase: 'decode', ids: new WeakMap(), nextId: 1 };
    state.observer = new MutationObserver(records => {
      if (records.some(record => record.type === 'childList' || ['src', 'srcset'].includes(record.attributeName))) {
        state.revision++;
        state.ready = false;
      }
    });
    state.observer.observe(renderer, { subtree: true, childList: true, attributes: true, attributeFilter: ['src', 'srcset'] });
    Object.defineProperty(window, key, { value: state, configurable: true });
  }
  const visible = element => {
    if (!element.getClientRects().length) return false;
    const r = element.getBoundingClientRect();
    if (r.width <= 0 || r.height <= 0 || r.right <= 0 || r.bottom <= 0 || r.left >= innerWidth || r.top >= innerHeight) return false;
    for (let e = element; e; e = e.parentElement) {
      const s = getComputedStyle(e);
      if (s.display === 'none' || s.visibility !== 'visible' || Number(s.opacity) === 0) return false;
    }
    return true;
  };
  const collect = () => {
    const images = [...renderer.querySelectorAll('img')].filter(image => {
      const r = image.getBoundingClientRect();
      return visible(image) && r.width >= innerWidth * .18 && r.height >= innerHeight * .35;
    });
    const canvas = [...renderer.querySelectorAll('canvas')].some(visible);
    const loading = [...document.querySelectorAll('.kr-interaction-layer-fullpage .loader, #kr-renderer [aria-busy="true"]')].some(element => {
      if (!visible(element)) return false;
      if (element.getAttribute('aria-busy') === 'true') return true;
      // The real reader keeps an empty, viewport-sized .loader div even after
      // loading. Its rectangle alone is not a loading indicator.
      if (element.childElementCount > 0 || getComputedStyle(element).backgroundImage !== 'none') return true;
      return ['::before', '::after'].some(pseudo => {
        const style = getComputedStyle(element, pseudo);
        return !['none', 'normal'].includes(style.content) && style.display !== 'none';
      });
    });
    const animating = renderer.getAnimations({ subtree: true }).some(animation => ['running', 'pending'].includes(animation.playState) || animation.pending);
    const signature = images.map(image => {
      if (!state.ids.has(image)) state.ids.set(image, state.nextId++);
      const r = image.getBoundingClientRect();
      // URLs are compared locally only, and are never returned to the host.
      return JSON.stringify([state.ids.get(image), image.currentSrc || image.src, image.naturalWidth, image.naturalHeight, r.x, r.y, r.width, r.height]);
    }).join('|');
    return { images, signature, blocked: loading || animating || images.length < 1 || images.length > (allowSpread ? 2 : 1), canvas, loading, animating };
  };
  const page = collect();
  if (page.signature !== state.signature || state.revision !== state.seenRevision) {
    state.token++;
    state.signature = page.signature;
    state.seenRevision = state.revision;
    state.ready = false;
    state.pending = false;
  }
  if (!page.images.length && page.canvas) return { ready: false, token: state.token, error: 'canvas-not-supported' };
  if (page.blocked || !page.images.every(image => image.complete && image.naturalWidth > 0 && image.naturalHeight > 0)) {
    state.ready = false;
    const waitingFor = page.loading ? 'loading-indicator' : page.animating ? 'animation'
      : page.images.length < 1 || page.images.length > (allowSpread ? 2 : 1) ? 'image-count' : 'image-load';
    return { ready: false, token: state.token, waitingFor, imageCount: page.images.length };
  }
  if (!state.ready && !state.pending) {
    state.pending = true;
    state.phase = 'decode';
    const token = state.token, revision = state.revision, signature = page.signature;
    Promise.all(page.images.map(image => image.decode())).then(() => {
      if (state.token === token && state.revision === revision) state.phase = 'paint';
      requestAnimationFrame(() => requestAnimationFrame(() => {
        if (state.token !== token || state.revision !== revision) return;
        const current = collect();
        state.pending = false;
        state.ready = !current.blocked && current.signature === signature
          && current.images.every(image => image.complete && image.naturalWidth > 0 && image.naturalHeight > 0);
      }));
    }).catch(() => {
      if (state.token === token) { state.pending = false; state.ready = false; }
    });
  }
  return { ready: state.ready, token: state.token, waitingFor: state.ready ? null : state.phase, imageCount: page.images.length };
})(__MMK_EXPECTED_ASIN__)
