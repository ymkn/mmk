// The capture-only reader must not jump back to a synced reading position.
// Decline only the specifically identified resume-position prompt, never a
// generic yes/no dialog. No cookies or reader-internal state are accessed.
(() => {
  if (location.protocol !== 'https:' || location.hostname !== 'read.amazon.co.jp') return 'absent';
  const key = Symbol.for('mmk.capture.resume-guard');
  if (window[key]) return window[key].scan();
  const normalize = text => String(text ?? '').replace(/\s+/gu, '').toLowerCase();
  const parentOf = element => element.parentElement || element.getRootNode?.().host;
  const contains = (ancestor, element) => {
    for (let node = element, depth = 0; node && depth < 64; node = parentOf(node), depth++) if (node === ancestor) return true;
    return false;
  };
  const visible = (element, win) => {
    const rect = element.getBoundingClientRect();
    if (element.getClientRects().length === 0 || rect.width <= 0 || rect.height <= 0) return false;
    for (let node = element, depth = 0; node && depth < 64; node = parentOf(node), depth++) {
      const style = win.getComputedStyle(node);
      if (style.display === 'none' || style.visibility !== 'visible' || Number(style.opacity) === 0) return false;
    }
    return true;
  };
  let clicked;
  const scan = () => {
    const nodes = [];
    const visit = (root, win, depth) => {
      if (depth > 8 || nodes.length > 20000) return;
      for (const element of root.querySelectorAll('*')) {
        nodes.push({ element, win });
        if (element.shadowRoot) visit(element.shadowRoot, win, depth + 1);
        if (nodes.length > 20000) break;
      }
    };
    visit(document, window, 0);
    const buttons = nodes.filter(({element, win}) => element.matches('button, ion-button, [role="button"]') && !element.disabled && element.getAttribute('aria-disabled') !== 'true' && visible(element, win));
    for (const {element, win} of nodes) {
      if (!element.matches('h1, h2, h3, [role="heading"], strong, div, span, p') || !visible(element, win)) continue;
      if (!/^(前回読んでいたページ|最後に読んだページ|furthestpageread|lastpageread|previouslyreadpage)$/i.test(normalize(element.textContent))) continue;
      for (let container = parentOf(element), depth = 0; container && depth < 8; container = parentOf(container), depth++) {
        const text = container.textContent || '';
        if (!/(最後の位置|最後に読んだ|furthest|last.*(?:location|position))/i.test(text) || !/(移動|move|go to)/i.test(text)) continue;
        const within = buttons.filter(button => contains(container, button.element));
        const label = e => normalize(e.getAttribute('aria-label') || e.textContent);
        const no = within.find(({element}) => /^(いいえ|no)$/i.test(label(element)));
        const yes = within.find(({element}) => /^(はい|yes)$/i.test(label(element)));
        if (!no || !yes) continue;
        if (clicked === no.element) return 'pending';
        clicked = no.element;
        state.dismissed++;
        no.element.click();
        return 'dismissed';
      }
    }
    return 'absent';
  };
  const state = { scan, dismissed: 0 };
  window[key] = state;
  if (typeof MutationObserver === 'function') {
    let timer;
    const schedule = () => { clearTimeout(timer); timer = setTimeout(scan, 120); };
    new MutationObserver(schedule).observe(document, { childList: true, subtree: true, characterData: true, attributes: true, attributeFilter: ['class', 'style', 'hidden', 'aria-hidden', 'open'] });
    document.addEventListener('DOMContentLoaded', schedule, {once:true});
  }
  return scan();
})()
