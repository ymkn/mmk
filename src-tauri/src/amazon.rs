pub const AMAZON_CONTINUE_SERIES_LABEL: &str = "amazon-continue-series";

const AMAZON_CONTINUE_SERIES_URL_PREFIX: &str = "https://www.amazon.co.jp/amz-books/seeMore/?widgetId=desktop-books-storefront_ContinueSeriesDRS_V2&title=%E8%AA%AD%E3%81%BF%E5%A7%8B%E3%82%81%E3%81%9F%E3%82%B7%E3%83%AA%E3%83%BC%E3%82%BA%E3%81%AE%E7%B6%9A%E3%81%8D%E3%82%92%E8%AA%AD%E3%82%80%E3%81%AB%E3%81%AF%E3%81%93%E3%81%A1%E3%82%89&sourceType=RECS&page=1&filters=v1%3AFORMAT%5Bkindle_edition%5D&metadata=%24deviceID%3A%24deviceFormFactor%3Alarge%24deviceAppType%3ADESKTOP%24deviceTypeID%3A%24deviceFamily%3A%24deviceSurfaceType%3Adesktop%24cardAppType%3ADESKTOP%24cardSurfaceType%3Adesktop%24cardMobileOS%3AUnknown%24sidekickLocale%3Aja-JP%24locale%3Aja-JP%24clientRequestId%3AMMK%24multiFormatWidgetSpClickUrlType%3Adefault&cardFeedType=widgetId&ccs_id=";

pub fn continue_series_url() -> String {
    format!(
        "{}{}",
        AMAZON_CONTINUE_SERIES_URL_PREFIX,
        uuid::Uuid::new_v4()
    )
}

pub fn is_sign_in_url(url: &tauri::Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some("www.amazon.co.jp")
        && url.path().starts_with("/ap/signin")
}

// This runs in the dedicated Amazon webview. It transfers only the recommendation
// cards needed by MMK; cookies, request metadata, and page HTML stay in the webview.
pub const AMAZON_CONTINUE_SERIES_IMPORT_SCRIPT: &str = r#"
(() => {
  if (window.location.hostname !== 'www.amazon.co.jp') return;

  const maxItems = 500;
  const checkId = __MMK_CHECK_ID__;
  const invoke = (command, args = {}) => {
    const bridge = window.__TAURI_INTERNALS__;
    if (!bridge || typeof bridge.invoke !== 'function') return Promise.reject(new Error('Tauri bridge unavailable'));
    return bridge.invoke(command, { ...args, checkId });
  };

  if (window.location.pathname.startsWith('/ap/signin')) {
    const showLogin = () => invoke('show_amazon_login').catch(() => {});
    if (document.readyState === 'loading') {
      window.addEventListener('DOMContentLoaded', showLogin, { once: true });
    } else {
      showLogin();
    }
    return;
  }

  const balancedJson = (text, start, open, close) => {
    let depth = 0;
    let quoted = false;
    let escaped = false;
    for (let index = start; index < text.length; index += 1) {
      const character = text[index];
      if (quoted) {
        if (escaped) escaped = false;
        else if (character === '\\') escaped = true;
        else if (character === '"') quoted = false;
        continue;
      }
      if (character === '"') {
        quoted = true;
      } else if (character === open) {
        depth += 1;
      } else if (character === close && --depth === 0) {
        return text.slice(start, index + 1);
      }
    }
    return null;
  };

  const send = (items) => invoke('store_amazon_continue_series', { items });

  const parsePrice = (value) => {
    if (typeof value === 'number') return Number.isFinite(value) ? value : null;
    if (typeof value === 'string') {
      const normalized = value.replace(/[^0-9.-]/g, '');
      if (!/[0-9]/.test(normalized)) return null;
      const parsed = Number(normalized);
      return Number.isFinite(parsed) ? Math.round(parsed) : null;
    }
    if (!value || typeof value !== 'object') return null;
    const whole = Number(value.wholeValue);
    const fractional = value.fractionalValue == null ? 0 : Number(value.fractionalValue);
    if (Number.isFinite(whole) && Number.isFinite(fractional)) {
      return Math.round(whole + fractional / 100);
    }
    return parsePrice(value.altText || value.displayString || '');
  };

  // seeMore cards use averageRatingValue / reviewInfo.reviewStarInfo, while
  // storefront cards use starCountAlt etc. Neither format uses "rating" alone.
  const ratingKeys = new Set([
    'rating', 'ratingValue', 'starRating', 'reviewRating', 'averageRating', 'customerRating',
    'averageRatingValue', 'reviewStarInfo', 'starCountAlt', 'starCountShortDisplayString', 'fullStarCount',
  ]);
  const reviewCountKeys = new Set(['reviewCount', 'numberReviews', 'ratingCount']);
  const containsRatingValue = (value, depth = 0) => {
    if (depth > 3 || value == null) return false;
    if (typeof value === 'number') return Number.isFinite(value) && value > 0 && value <= 5;
    if (typeof value === 'string') {
      const text = value.normalize('NFKC').trim();
      if (/^\d+(?:\.\d+)?$/.test(text)) return containsRatingValue(Number(text));
      const match = text.match(/(?:5つ星のうち|5つのうち)\s*(\d+(?:\.\d+)?)/)
        || text.match(/(\d+(?:\.\d+)?)\s*(?:out of\s*5|\/\s*5)/i);
      return Boolean(match && containsRatingValue(Number(match[1])));
    }
    if (typeof value !== 'object') return false;
    // Do not mistake maxValue=5, a review URL or a placeholder label for a rating.
    return ['value', 'ratingValue', 'averageRatingValue', 'alt', 'altText', 'displayString', 'shortDisplayString', 'numberOfFilledStar']
      .some((key) => containsRatingValue(value[key], depth + 1));
  };
  const containsReviewCount = (value) => {
    if (typeof value === 'string') {
      const text = value.normalize('NFKC').trim();
      if (!/^(?:\d+|\d{1,3}(?:,\d{3})+)$/.test(text)) return false;
      value = Number(text.replace(/,/g, ''));
    }
    return typeof value === 'number' && Number.isSafeInteger(value) && value > 0;
  };

  const cardHasRating = (card) => {
    const visit = (value, key, depth) => {
      if (depth > 5 || value == null) return false;
      if (ratingKeys.has(key) && containsRatingValue(value)) return true;
      if (reviewCountKeys.has(key) && containsReviewCount(value)) return true;
      if (typeof value !== 'object') return false;
      return Object.entries(value).some(([childKey, child]) => visit(child, childKey, depth + 1));
    };
    if (visit(card, '', 0)) return true;

    const cardElement = Array.from(document.querySelectorAll('[data-asin]')).find((element) => element.getAttribute('data-asin') === String(card.asin));
    return Boolean(cardElement?.querySelector('.a-icon-star, .a-icon-star-small, [aria-label*="星"], [aria-label*="star" i]'));
  };

  const parseImageUrl = (card) => {
    const normalize = (value) => {
      if (typeof value !== 'string') return null;
      const trimmed = value.trim();
      if (trimmed.startsWith('//')) return `https:${trimmed}`;
      return trimmed.startsWith('https://') ? trimmed : null;
    };
    const preferred = [
      card.imageUrl,
      card.imageURL,
      card.coverImageUrl,
      card.coverUrl,
      card.image?.url,
      card.image?.src,
      card.image?.large,
      card.image?.hiRes,
    ].map(normalize).find(Boolean);
    if (preferred) return preferred;

    const candidates = [];
    const visit = (value, path, depth) => {
      if (depth > 5 || candidates.length >= 50 || value == null) return;
      if (typeof value === 'string') {
        const normalized = normalize(value);
        if (normalized && /(?:media-amazon\.com|ssl-images-amazon\.com)\//i.test(normalized)) candidates.push(normalized);
        else if (normalized && /image|cover/i.test(path)) candidates.push(normalized);
        return;
      }
      if (typeof value !== 'object') return;
      for (const [key, child] of Object.entries(value)) visit(child, `${path}.${key}`, depth + 1);
    };
    visit(card, '', 0);
    return candidates.find((url) => /(?:media-amazon\.com|ssl-images-amazon\.com)\//i.test(url)) || candidates[0] || null;
  };

  const loginRequired = () => {
    const account = document.querySelector('#nav-link-accountList');
    const text = (account?.textContent || '').replace(/\s+/g, '');
    const href = account?.getAttribute('href') || '';
    return text.includes('ログイン') || href.includes('/ap/signin');
  };

  const navigateToLogin = () => {
    const account = document.querySelector('#nav-link-accountList');
    let signIn;
    try {
      signIn = new URL(account?.getAttribute('href') || '/ap/signin', window.location.origin);
    } catch (_) {
      return false;
    }
    if (signIn.origin !== 'https://www.amazon.co.jp' || !signIn.pathname.startsWith('/ap/signin')) return false;
    signIn.searchParams.set('openid.return_to', window.location.href);
    window.location.replace(signIn.toString());
    return true;
  };

  const collect = async () => {
    if (done || collecting) return;
    collecting = true;
    try {
      if (document.querySelector('#captchacharacters, form[action*="validateCaptcha"]') || window.location.pathname.includes('validateCaptcha')) {
        done = true;
        await invoke('report_amazon_check_error', { reason: 'verification' });
        return;
      }
      if (loginRequired()) {
        done = navigateToLogin();
        return;
      }
      for (const script of Array.from(document.scripts)) {
        const text = script.textContent || '';
        if (!text.includes('CBBE_BooksCardsReactAssets_cardsToRender')) continue;
        const marker = text.indexOf('asinList');
        if (marker < 0) continue;
        const start = text.indexOf('[', marker);
        if (start < 0) continue;
        const json = balancedJson(text, start, '[', ']');
        if (!json) continue;
        let cards;
        try {
          cards = JSON.parse(json);
        } catch (_) {
          // Amazon may replace the card payload while the page is rendering.
          continue;
        }
        if (!Array.isArray(cards)) continue;
        const items = cards.slice(0, maxItems).map((card) => ({
          asin: String(card.asin || ''),
          title: String(card.bookTitle || ''),
          price: parsePrice(card.price),
          hasRating: cardHasRating(card),
          imageUrl: parseImageUrl(card),
        })).filter((item) => item.asin && item.title);
        await send(items);
        done = true;
        return;
      }
    } catch (_) {
      done = true;
      await invoke('report_amazon_check_error', { reason: 'bridge' }).catch(() => {});
    } finally {
      collecting = false;
      if (done) window.clearInterval(timer);
    }
  };

  let attempts = 0;
  let done = false;
  let collecting = false;
  const timer = window.setInterval(() => {
    collect();
    if (++attempts >= 120 && !done && !collecting) {
      done = true;
      window.clearInterval(timer);
      invoke('report_amazon_check_error', { reason: 'unreadable' }).catch(() => {});
    }
  }, 500);
  window.addEventListener('load', collect, { once: true });
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_amazon_japan_sign_in_urls() {
        assert!(is_sign_in_url(
            &tauri::Url::parse("https://www.amazon.co.jp/ap/signin?openid.return_to=x").unwrap()
        ));
        assert!(!is_sign_in_url(
            &tauri::Url::parse("https://www.amazon.co.jp/amz-books/seeMore/").unwrap()
        ));
        assert!(!is_sign_in_url(
            &tauri::Url::parse("https://example.com/ap/signin").unwrap()
        ));
    }
}
