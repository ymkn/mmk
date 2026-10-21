// Operate ordinary, visible navigation controls only. Never access reader internals.
((options) => {
  if (location.protocol !== 'https:' || location.hostname !== 'read.amazon.co.jp') return 'unavailable';
  const controls = [];
  const normalize = text => String(text ?? '').normalize('NFKC').replace(/[\s\u200B-\u200D\uFEFF]+/gu, '').toLocaleLowerCase();
  const expectedTitle = normalize(options.bookTitle);
  const tocScope = element => {
    for (let parent = element, depth = 0; parent && depth < 64; depth++) {
      const role = parent.getAttribute?.('role');
      const identity = `${parent.id ?? ''} ${parent.getAttribute?.('class') ?? ''} ${parent.getAttribute?.('aria-label') ?? ''} ${parent.getAttribute?.('data-testid') ?? ''}`;
      if (['menu', 'tree', 'listbox'].includes(role)
          || (!['button', 'menuitem', 'treeitem', 'option', 'listitem'].includes(role)
            && !['button', 'a', 'li', 'ion-button', 'ion-item'].includes(parent.localName)
            && !/toc[-_]?(?:item|entry)|goto[-_]?(?:item|entry)/i.test(identity)
            && /table[-_ ]?of[-_ ]?contents|(?:^|[-_\s])toc(?:$|[-_\s])|toc(?:menu|panel|list|container|entries)|kindlereader[_-]goto|go[-_]?to(?:menu|panel|dialog|list)|目次/i.test(identity))) return parent;
      parent = parent.parentElement || parent.getRootNode?.().host;
    }
    return null;
  };
  const innermost = items => items.filter(item => !items.some(other => other !== item && item.element.contains?.(other.element)));
  let count = 0;
  const visit = (root, win, depth) => {
    if (depth > 8) return;
    for (const element of root.querySelectorAll('*')) {
      if (++count > 20000) return;
      if (element.shadowRoot) visit(element.shadowRoot, win, depth + 1);
      if (element.localName === 'iframe') {
        try { if (element.contentDocument) visit(element.contentDocument, element.contentWindow, depth + 1); } catch {}
      }
      const scope = tocScope(element);
      const interactive = element.matches('button, a, [role="button"], [role="menuitem"], [role="option"], [role="treeitem"], [role="menuitemradio"]');
      // Some Kindle TOCs use delegated click handlers on ordinary list rows or spans.
      const row = Boolean(scope) && element !== scope && element.matches('li, div, span, [role="listitem"]');
      if (!interactive && !row) continue;
      if (element.disabled || element.getAttribute('aria-disabled') === 'true') continue;
      let visible = element.getClientRects().length > 0;
      for (let parent = element; visible && parent; parent = parent.parentElement) {
        const style = win.getComputedStyle(parent);
        visible = style.visibility === 'visible' && style.display !== 'none' && Number(style.opacity) !== 0;
      }
      const rect = element.getBoundingClientRect();
      if (!visible || rect.width <= 0 || rect.height <= 0) continue;
      const labels = [element.getAttribute('aria-label'), element.getAttribute('title'), element.textContent].filter(Boolean).map(text => text.trim());
      controls.push({ element, labels, scope, inViewport: rect.bottom > 0 && rect.top < win.innerHeight });
    }
  };
  visit(document, window, 0);
  const selectEntry = entry => {
    entry.element.click();
    // The real Kindle reader leaves its side panel open after a TOC selection.
    // Closing a dedicated side-panel close button is not the same as toggling
    // the toolbar's TOC button (which could open it again).
    const close = controls.find(({element, inViewport}) => inViewport && element.matches('button.side-menu-close-button'));
    if (close && close.element.isConnected !== false && close.element.getClientRects().length > 0) close.element.click();
  };
  // A proper TOC lists the book's beginning at its very top, whatever the
  // entry is labeled. The host confirms the earliest reachable reading position
  // before saving; that position need not be numbered 1.
  const entries = innermost(controls.filter(({ element, scope, labels }) => scope && element !== scope
    && !element.closest?.('header, [role="banner"]')
    && labels.some(text => text.trim() && !/^(移動|目次|移動先|閉じる|戻る|go to|table of contents|close|back)$/i.test(text))));
  // A previously scrolled TOC must not make a later chapter look like its start.
  let scrolled = false;
  for (const entry of entries) {
    for (let parent = entry.element, depth = 0; parent && depth < 64; depth++) {
      if (parent.scrollTop > 0) { parent.scrollTop = 0; scrolled = true; }
      parent = parent.parentElement || parent.getRootNode?.().host;
    }
  }
  if (scrolled) return 'menu'; // Re-measure after the normal scrolling redraw.
  const visibleEntries = entries.filter(({ inViewport }) => inViewport);
  if (visibleEntries.length) {
    const topOf = entry => entry.element.getBoundingClientRect().top;
    const topmost = visibleEntries.reduce((best, entry) => (topOf(entry) < topOf(best) ? entry : best));
    selectEntry(topmost);
    return 'cover';
  }
  const candidates = controls.filter(({ element, scope, labels }) => element !== scope
    && (scope || (options.menuOpened && !element.closest?.('header, [role="banner"]')))
    && labels.some(text => normalize(text) === expectedTitle && expectedTitle));
  const titles = innermost(candidates);
  if (titles.length === 1 && titles[0].inViewport) { selectEntry(titles[0]); return 'title'; }
  if (titles.length > 1) return 'ambiguous';
  if (options.menuOpened) return 'waiting';
  if (controls.some(({ scope }) => scope)) return 'menu';
  const goTo = controls.find(({ labels, inViewport }) => inViewport && labels.some(text => /^(移動|目次|移動先|go to|table of contents)$/i.test(text)));
  if (goTo) { goTo.element.click(); return 'menu'; }
  return 'unavailable';
})(__MMK_READER_START_OPTIONS__)
