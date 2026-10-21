use crate::capture::Region;
use serde::{Deserialize, Serialize};
use std::{sync::mpsc, time::Duration};

const READER_LAYOUT_SCRIPT: &str = include_str!("reader_layout.js");

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PageRender {
    pub ready: bool,
    pub token: u64,
    pub error: Option<PageRenderError>,
    #[serde(default)]
    pub waiting_for: Option<PageWaitReason>,
    #[serde(default)]
    pub image_count: Option<usize>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum PageRenderError {
    #[serde(rename = "wrong-book")]
    WrongBook,
    #[serde(rename = "canvas-not-supported")]
    CanvasNotSupported,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PageWaitReason {
    Document,
    LoadingIndicator,
    Animation,
    ImageCount,
    ImageLoad,
    Decode,
    Paint,
}

fn layout_script(single_page: bool) -> String {
    READER_LAYOUT_SCRIPT.replace(
        "__MMK_ALLOW_SPREAD__",
        if single_page { "false" } else { "true" },
    )
}

pub fn reader_page_render(
    reader: &tauri::WebviewWindow,
    asin: &str,
    single_page: bool,
) -> Result<PageRender, String> {
    let script = include_str!("reader_page_render.js")
        .replace(
            "__MMK_EXPECTED_ASIN__",
            &serde_json::to_string(asin).map_err(|e| e.to_string())?,
        )
        .replace(
            "__MMK_ALLOW_SPREAD__",
            if single_page { "false" } else { "true" },
        );
    let (send, receive) = mpsc::sync_channel(1);
    reader
        .eval_with_callback(script, move |result| {
            let _ = send.try_send(result);
        })
        .map_err(|e| e.to_string())?;
    let result = receive
        .recv_timeout(Duration::from_secs(3))
        .map_err(|_| "ページ描画状態の取得が時間切れになりました")?;
    serde_json::from_str(&result)
        .map_err(|_| "ページの読み込み・描画状態を確認できませんでした".into())
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolbarVisibility {
    pub control_count: usize,
    pub visible_control_count: usize,
    pub host_visible: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReaderNavigation {
    pub known: bool,
    pub chrome_visible: bool,
    #[serde(default)]
    pub toolbar: Option<ToolbarVisibility>,
    pub toc_open: bool,
    #[serde(default)]
    pub toc_close_visible: bool,
    pub position: Option<u32>,
    #[serde(default)]
    pub position_end: Option<u32>,
    pub total: Option<u32>,
    #[serde(default)]
    pub position_texts: Vec<String>,
}

pub fn reader_navigation(reader: &tauri::WebviewWindow) -> Result<ReaderNavigation, String> {
    let (send, receive) = mpsc::sync_channel(1);
    reader
        .eval_with_callback(include_str!("reader_navigation.js"), move |result| {
            let _ = send.try_send(result);
        })
        .map_err(|e| e.to_string())?;
    let result = receive
        .recv_timeout(Duration::from_secs(3))
        .map_err(|_| "リーダーの表示状態の取得が時間切れになりました")?;
    serde_json::from_str(&result).map_err(|_| "リーダーの表示状態を確認できませんでした".into())
}

pub fn reader_ready(
    reader: &tauri::WebviewWindow,
    asin: &str,
    single_page: bool,
) -> Result<bool, String> {
    let expected = serde_json::to_string(asin).map_err(|e| e.to_string())?;
    let script = include_str!("reader_ready.js")
        .replace("__MMK_EXPECTED_ASIN__", &expected)
        .replace("__MMK_READER_LAYOUT__", &layout_script(single_page));
    let (send, receive) = mpsc::sync_channel(1);
    reader
        .eval_with_callback(script, move |result| {
            let _ = send.try_send(result);
        })
        .map_err(|e| e.to_string())?;
    let result = receive
        .recv_timeout(Duration::from_secs(3))
        .map_err(|_| "本文の読み込み確認が時間切れになりました")?;
    serde_json::from_str(&result).map_err(|_| "本文の読み込み状態を確認できませんでした".into())
}

fn close_reader_toc(reader: &tauri::WebviewWindow) -> Result<(), String> {
    // Ionic ignores close() while its opening transition is still running.
    // During closing, wait for the backdrop as well as the close button to leave.
    close_reader_toc_with(
        || reader_navigation(reader),
        || reader.eval("document.querySelector('button.side-menu-close-button')?.click()")
            .map_err(|e| e.to_string()),
        || std::thread::sleep(Duration::from_millis(200)),
    )
}

fn close_reader_toc_with(
    mut observe: impl FnMut() -> Result<ReaderNavigation, String>,
    mut close: impl FnMut() -> Result<(), String>,
    mut pause: impl FnMut(),
) -> Result<(), String> {
    for _ in 0..10 {
        let ui = observe()?;
        if !ui.toc_open {
            return Ok(());
        }
        if ui.toc_close_visible {
            close()?;
        }
        pause();
    }
    if observe()?.toc_open {
        Err("目次パネルが閉じませんでした。本文を表示してから再試行してください".into())
    } else {
        Ok(())
    }
}

pub fn decline_resume_prompt(reader: &tauri::WebviewWindow) -> Result<bool, String> {
    let (send, receive) = mpsc::sync_channel(1);
    reader
        .eval_with_callback(include_str!("reader_resume_prompt.js"), move |result| {
            let _ = send.try_send(result);
        })
        .map_err(|e| e.to_string())?;
    let result = receive
        .recv_timeout(Duration::from_secs(3))
        .map_err(|_| "読書位置確認の応答が時間切れになりました")?;
    match serde_json::from_str::<String>(&result).ok().as_deref() {
        Some("absent") => Ok(false),
        Some("dismissed" | "pending") => Ok(true),
        _ => Err("読書位置の確認画面を判定できませんでした".into()),
    }
}

fn first_page_script(book_title: &str, menu_opened: bool) -> String {
    include_str!("reader_first_page.js").replace(
        "__MMK_READER_START_OPTIONS__",
        &serde_json::json!({
            "bookTitle": book_title,
            "menuOpened": menu_opened,
        })
        .to_string(),
    )
}

pub fn try_go_to_cover(reader: &tauri::WebviewWindow, book_title: &str) -> Result<bool, String> {
    let mut menu_opened = false;
    for _ in 0..8 {
        if decline_resume_prompt(reader)? {
            std::thread::sleep(Duration::from_millis(350));
            continue;
        }
        let (send, receive) = mpsc::sync_channel(1);
        reader
            .eval_with_callback(first_page_script(book_title, menu_opened), move |result| {
                let _ = send.try_send(result);
            })
            .map_err(|e| e.to_string())?;
        let result = receive
            .recv_timeout(Duration::from_secs(3))
            .map_err(|_| "先頭移動の応答が時間切れになりました")?;
        match serde_json::from_str::<String>(&result).ok().as_deref() {
            Some("cover" | "title") => { close_reader_toc(reader)?; return Ok(true); },
            Some("ambiguous") => return Err("目次内に同じ書籍名の項目が複数あるため自動選択できません。先頭の項目を手動で選んでください".into()),
            Some("menu") => {
                menu_opened = true;
                std::thread::sleep(Duration::from_millis(350));
            }
            Some("waiting") if menu_opened => std::thread::sleep(Duration::from_millis(350)),
            _ if menu_opened => break,
            _ => return Ok(false),
        }
    }
    Err("目次の表紙・書籍名の項目を特定できません。リーダーで書籍の目次と表示設定を確認してください".into())
}

pub fn portrait_size(
    work_width: i32,
    work_height: i32,
    border_width: i32,
    border_height: i32,
) -> Result<(i32, i32), String> {
    if work_width < 240
        || work_height < 360
        || border_width < 0
        || border_height < 0
        || border_width >= work_width
        || border_height >= work_height
    {
        return Err("縦長表示に必要な画面領域がありません".into());
    }
    // The official reader reserves about 10% of viewport width on each side.
    // A 0.68-wide window made a typical 0.70 page width-limited, even at full
    // height. Leave room for those gutters without altering the reader DOM.
    let width = (((work_height - border_height) as f64 * 0.90).round() as i32 + border_width)
        .min(work_width);
    Ok((width, work_height))
}

#[derive(Deserialize)]
struct Viewport {
    width: f64,
    height: f64,
}
#[derive(Deserialize)]
struct PageRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}
#[derive(Deserialize)]
struct Layout {
    error: Option<String>,
    viewport: Option<Viewport>,
    rect: Option<PageRect>,
}

pub fn reader_region(
    reader: &tauri::WebviewWindow,
    width: u32,
    height: u32,
    single_page: bool,
) -> Result<Option<Region>, String> {
    let url = reader.url().map_err(|e| e.to_string())?;
    if url.scheme() != "https" || url.host_str() != Some("read.amazon.co.jp") {
        return Err("内蔵リーダーで漫画の本文を表示してから自動調整してください".into());
    }
    let script = layout_script(single_page);
    let (send, receive) = mpsc::sync_channel(1);
    reader
        .eval_with_callback(&script, move |result| {
            let _ = send.try_send(result);
        })
        .map_err(|e| e.to_string())?;
    let json = receive
        .recv_timeout(Duration::from_secs(3))
        .map_err(|_| "ページ領域の取得が時間切れになりました")?;
    let layout: Layout =
        serde_json::from_str(&json).map_err(|_| "ページの表示位置を取得できませんでした")?;
    convert_layout(layout, width, height, single_page)
}

fn convert_layout(
    layout: Layout,
    width: u32,
    height: u32,
    single_page: bool,
) -> Result<Option<Region>, String> {
    match layout.error.as_deref() {
        Some("spread") => {
            return Err(
                "見開き表示を検出しました。設定を「見開き」にするか、リーダーを単ページ表示にしてください".into(),
            )
        }
        Some("not-reader") => return Err("漫画の本文を表示してください".into()),
        Some(_) => return Ok(None),
        None => {}
    }
    let (Some(viewport), Some(rect)) = (layout.viewport, layout.rect) else {
        return Ok(None);
    };
    let values = [
        viewport.width,
        viewport.height,
        rect.x,
        rect.y,
        rect.width,
        rect.height,
    ];
    if values.iter().any(|v| !v.is_finite())
        || viewport.width <= 0.0
        || viewport.height <= 0.0
        || rect.width <= 0.0
        || rect.height <= 0.0
        || rect.x < 0.0
        || rect.y < 0.0
        || rect.x + rect.width > viewport.width + 1.0
        || rect.y + rect.height > viewport.height + 1.0
    {
        return Err("取得したページ領域が画面外、または不正です".into());
    }
    // Use physical client size, not devicePixelRatio: this also handles WebView zoom/DPI.
    let sx = width as f64 / viewport.width;
    let sy = height as f64 / viewport.height;
    let x = (rect.x * sx).floor() as i32;
    let y = (rect.y * sy).floor() as i32;
    let right = ((rect.x + rect.width) * sx).ceil().min(width as f64) as i32;
    let bottom = ((rect.y + rect.height) * sy).ceil().min(height as f64) as i32;
    let region = Region {
        x,
        y,
        width: right - x,
        height: bottom - y,
    };
    validate_page_region(region, width, height, single_page)?;
    Ok(Some(region))
}

#[allow(dead_code)]
pub fn validate_single_page(region: Region, width: u32, height: u32) -> Result<(), String> {
    validate_page_region(region, width, height, true)
}

pub fn validate_page_region(
    region: Region,
    width: u32,
    height: u32,
    single_page: bool,
) -> Result<(), String> {
    if region.x < 0
        || region.y < 0
        || region.width < 100
        || region.height < 150
        || region.x as i64 + region.width as i64 > width as i64
        || region.y as i64 + region.height as i64 > height as i64
    {
        return Err(
            "ページ領域を安全に確定できません。ページ表示設定とページ背景色を確認してください"
                .into(),
        );
    }
    let aspect = region.width as f64 / region.height as f64;
    if single_page {
        if aspect > 1.05 {
            return Err("見開き表示を検出しました。単ページ表示に切り替えてください".into());
        }
        if !(0.35..=0.95).contains(&aspect) {
            return Err("縦長の単ページ領域を判定できませんでした".into());
        }
    } else if !(1.0..=2.4).contains(&aspect) {
        return Err(
            "見開きのページ領域を判定できませんでした。リーダーを見開き表示にしてください".into(),
        );
    }
    Ok(())
}

/// Find an actual rectangular page/background boundary, never the bounding box
/// of dark artwork. An indistinguishable white page on a white background fails closed.
pub fn pixel_region(
    width: u32,
    height: u32,
    rgb: &[u8],
    single_page: bool,
) -> Result<Region, String> {
    // A fitted portrait page often touches the left/right edges. In that case
    // top/bottom gutters, not side gutters, establish the page rectangle.
    let horizontal = pixel_region_axis(width, height, rgb, false, single_page);
    match horizontal {
        Ok(region) => Ok(region),
        Err(error) if error.contains("見開き") => Err(error),
        Err(_) => pixel_region_axis(width, height, rgb, true, single_page),
    }
}

/// A Kindle image element can include viewer-colored padding in its own bounds.
/// Refine it only when the visible background corroborates a smaller rectangle,
/// and every pixel being removed is that same background color.
pub fn refine_reader_region(
    width: u32,
    height: u32,
    rgb: &[u8],
    region: Region,
    single_page: bool,
) -> Result<Region, String> {
    validate_page_region(region, width, height, single_page)?;
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(3));
    if expected != Some(rgb.len()) {
        return Err("ページ境界判定用の画像データが不正です".into());
    }
    let pixel = |x: i32, y: i32| -> [u8; 3] {
        let offset = (y as usize * width as usize + x as usize) * 3;
        rgb[offset..offset + 3].try_into().unwrap()
    };
    let similar = |a: [u8; 3], b: [u8; 3]| a.into_iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 12);
    let mut samples = Vec::new();
    for i in 1..=9 {
        let y = region.y + region.height * i / 10;
        if region.x >= 4 {
            samples.push(pixel(region.x / 2, y));
        }
        let right = region.x + region.width;
        if width as i32 - right >= 4 {
            samples.push(pixel((right + width as i32) / 2, y));
        }
    }
    let Some(&background) = samples.iter().max_by_key(|color| {
        samples
            .iter()
            .filter(|sample| similar(**color, **sample))
            .count()
    }) else {
        return Ok(region);
    };
    // A colored backdrop is distinct from white paper and monochrome artwork.
    // White/gray/black alone cannot safely identify embedded page padding.
    if samples.len() < 9
        || samples
            .iter()
            .filter(|color| similar(**color, background))
            .count()
            * 4
            < samples.len() * 3
        || background.iter().max().unwrap() - background.iter().min().unwrap() < 16
    {
        return Ok(region);
    }
    let Ok(candidate) = pixel_region(width, height, rgb, single_page) else {
        return Ok(region);
    };
    if candidate.x < region.x
        || candidate.y < region.y
        || candidate.x + candidate.width > region.x + region.width
        || candidate.y + candidate.height > region.y + region.height
    {
        return Ok(region);
    }
    if candidate == region {
        return Ok(region);
    }
    // Check the entire discarded area, not merely the artwork's bounding box.
    // An internal stripe matching the background must never crop off white paper.
    let padded = Region {
        x: (candidate.x - 2).max(region.x),
        y: (candidate.y - 2).max(region.y),
        width: 0,
        height: 0,
    };
    let padded = Region {
        width: (candidate.x + candidate.width + 2).min(region.x + region.width) - padded.x,
        height: (candidate.y + candidate.height + 2).min(region.y + region.height) - padded.y,
        ..padded
    };
    for candidate in [candidate, padded] {
        let mut safe = true;
        'rows: for y in region.y..region.y + region.height {
            let outside_y = y < candidate.y || y >= candidate.y + candidate.height;
            for x in region.x..region.x + region.width {
                if (outside_y || x < candidate.x || x >= candidate.x + candidate.width)
                    && !similar(pixel(x, y), background)
                {
                    safe = false;
                    break 'rows;
                }
            }
        }
        // The second candidate leaves two extra pixels for antialiased edges.
        if safe {
            return Ok(candidate);
        }
    }
    Ok(region)
}

fn pixel_region_axis(
    width: u32,
    height: u32,
    rgb: &[u8],
    transpose: bool,
    single_page: bool,
) -> Result<Region, String> {
    let original_w = width as usize;
    let original_h = height as usize;
    let (w, h) = if transpose {
        (original_h, original_w)
    } else {
        (original_w, original_h)
    };
    if w < 160 || h < 240 || rgb.len() != w * h * 3 {
        return Err("自動判定に必要な画面サイズがありません".into());
    }
    let pixel = |x: usize, y: usize| -> [u8; 3] {
        let offset = if transpose {
            x * original_w + y
        } else {
            y * original_w + x
        } * 3;
        rgb[offset..offset + 3].try_into().unwrap()
    };
    let similar = |a: [u8; 3], b: [u8; 3]| a.into_iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 12);
    // Multiple rows must independently agree on both page edges. Toolbars and
    // isolated drawing strokes cannot establish a page rectangle.
    let rows: Vec<usize> = (0..17).map(|i| h * (20 + i * 4) / 100).collect();
    let mut spans = Vec::new();
    for y in rows {
        let left_bg = pixel(w / 100, y);
        let right_bg = pixel(w - 1 - w / 100, y);
        if left_bg.iter().all(|v| *v >= 245) {
            continue;
        }
        if !similar(left_bg, right_bg) {
            continue;
        }
        let margin = (w / 100).max(3);
        if !(0..margin)
            .all(|x| similar(pixel(x, y), left_bg) && similar(pixel(w - 1 - x, y), left_bg))
        {
            continue;
        }
        let left = (margin..w / 2).find(|x| !similar(pixel(*x, y), left_bg));
        let right = (w / 2..w - margin)
            .rev()
            .find(|x| !similar(pixel(*x, y), left_bg));
        if let (Some(left), Some(right)) = (left, right) {
            if right > left + w / 4 {
                spans.push((left, right + 1, left_bg));
            }
        }
    }
    let Some(&(left, right, background)) = spans.iter().max_by_key(|(left, right, _)| {
        spans
            .iter()
            .filter(|(l, r, _)| l.abs_diff(*left) <= 2 && r.abs_diff(*right) <= 2)
            .count()
    }) else {
        return Err(
            "ページと背景の境界が見つかりません。本文の表示とページ背景色を確認してください".into(),
        );
    };
    if spans
        .iter()
        .filter(|(l, r, _)| l.abs_diff(left) <= 2 && r.abs_diff(right) <= 2)
        .count()
        < 10
    {
        return Err("ページ左右の境界が安定しません。リーダーの表示設定を確認してください".into());
    }
    let columns: Vec<usize> = (0..13)
        .map(|i| left + (right - left) * (8 + i * 7) / 100)
        .collect();
    let line_is_page = |y: usize| {
        similar(pixel(left.saturating_sub(2), y), background)
            && similar(pixel((right + 1).min(w - 1), y), background)
            && columns
                .iter()
                .filter(|x| !similar(pixel(**x, y), background))
                .count()
                >= 11
    };
    let center = h / 2;
    // Only consider the continuous central rectangle. Floating top/bottom controls
    // remain outside. Page edges, including white margins, must contrast with gutters.
    if !line_is_page(center) {
        return Err(
            "ページ上下の境界を判定できません。リーダーの表示設定を確認してください".into(),
        );
    }
    let top = (0..=center)
        .rev()
        .find(|y| !line_is_page(*y))
        .map_or(0, |y| y + 1);
    let bottom = (center..h).find(|y| !line_is_page(*y)).unwrap_or(h);
    let mut region = Region {
        x: left as i32,
        y: top as i32,
        width: (right - left) as i32,
        height: (bottom - top) as i32,
    };
    if transpose {
        region = Region {
            x: region.y,
            y: region.x,
            width: region.height,
            height: region.width,
        };
    }
    validate_page_region(region, width, height, single_page)?;
    if region.height < height as i32 * 65 / 100 {
        return Err(
            "ページ上下の境界を安全に確定できません。リーダーの表示設定を確認してください".into(),
        );
    }
    // Check the border all the way around, rather than trimming into the artwork.
    // A genuine rectangle should have uniform surrounding gutters on both sides.
    let good = (0..20)
        .filter(|i| {
            let y = top + (bottom - top - 1) * i / 19;
            similar(pixel(left.saturating_sub(2), y), background)
                && similar(pixel((right + 1).min(w - 1), y), background)
        })
        .count();
    if good < 18 {
        return Err("周囲の背景が一定でないため自動判定を中止しました".into());
    }
    Ok(region)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn toc_navigation(open: bool, close_visible: bool) -> ReaderNavigation {
        serde_json::from_value(serde_json::json!({
            "known": true, "chromeVisible": true, "tocOpen": open,
            "tocCloseVisible": close_visible, "position": 2, "total": 195,
        })).unwrap()
    }

    #[test]
    fn toc_close_waits_for_backdrop_after_close_button_leaves_viewport() {
        use std::cell::Cell;
        let mut states = [
            toc_navigation(true, true),
            toc_navigation(true, false),
            toc_navigation(false, false),
        ].into_iter();
        let clicks = Cell::new(0);
        let pauses = Cell::new(0);
        close_reader_toc_with(
            || Ok(states.next().expect("must stop once the backdrop is gone")),
            || { clicks.set(clicks.get() + 1); Ok(()) },
            || pauses.set(pauses.get() + 1),
        ).unwrap();
        assert_eq!(clicks.get(), 1, "do not click an offscreen close button");
        assert_eq!(pauses.get(), 2, "the button disappearing is not closure");
    }

    #[test]
    fn toc_close_times_out_if_an_invisible_backdrop_still_blocks_input() {
        let error = close_reader_toc_with(
            || Ok(toc_navigation(true, false)),
            || panic!("do not click an offscreen close button"),
            || {},
        ).unwrap_err();
        assert!(error.contains("目次パネルが閉じません"));
    }

    #[test]
    fn toc_close_retries_a_visible_close_button_during_opening() {
        use std::cell::Cell;
        let mut states = [
            toc_navigation(true, true),
            toc_navigation(true, true),
            toc_navigation(false, false),
        ].into_iter();
        let clicks = Cell::new(0);
        close_reader_toc_with(
            || Ok(states.next().unwrap()),
            || { clicks.set(clicks.get() + 1); Ok(()) },
            || {},
        ).unwrap();
        assert_eq!(clicks.get(), 2);
    }

    #[test]
    fn portrait_window_uses_full_work_height_and_accounts_for_frame() {
        assert_eq!(portrait_size(1920, 1040, 16, 40).unwrap(), (916, 1040));
        assert_eq!(portrait_size(600, 1200, 16, 40).unwrap(), (600, 1200));
        assert!(portrait_size(10, 10, 16, 40).is_err());
    }
    #[test]
    fn portrait_width_allows_a_typical_page_to_fill_height_with_reader_gutters() {
        let (width, height) = portrait_size(3840, 2064, 26, 71).unwrap();
        let client_height = height - 71;
        let renderer_width = (width - 26) as f64 * 0.8;
        assert!(renderer_width >= client_height as f64 * 0.70);
        assert!(width < height);
    }
    #[test]
    fn maps_css_geometry_to_physical_pixels() {
        let layout: Layout = serde_json::from_str(r#"{"viewport":{"width":600,"height":900},"rect":{"x":50,"y":80,"width":500,"height":750}}"#).unwrap();
        let r = convert_layout(layout, 1200, 1800, true).unwrap().unwrap();
        assert_eq!(
            r,
            Region {
                x: 100,
                y: 160,
                width: 1000,
                height: 1500
            }
        );
    }
    #[test]
    fn spread_layouts_are_accepted_only_in_spread_mode() {
        let json = r#"{"viewport":{"width":1200,"height":900},"rect":{"x":40,"y":60,"width":1120,"height":780}}"#;
        let layout: Layout = serde_json::from_str(json).unwrap();
        let region = convert_layout(layout, 1200, 900, false).unwrap().unwrap();
        assert_eq!(region.width, 1120);
        assert!(convert_layout(serde_json::from_str(json).unwrap(), 1200, 900, true).is_err());
    }
    #[test]
    fn rejects_spreads_and_invalid_geometry() {
        for json in [
            r#"{"error":"spread"}"#,
            r#"{"viewport":{"width":600,"height":900},"rect":{"x":-1,"y":0,"width":500,"height":750}}"#,
        ] {
            assert!(convert_layout(serde_json::from_str(json).unwrap(), 600, 900, true).is_err());
        }
    }
    fn image(page: Region, background: u8) -> Vec<u8> {
        let mut rgb = vec![background; 600 * 900 * 3];
        for y in page.y..page.y + page.height {
            for x in page.x..page.x + page.width {
                let o = (y as usize * 600 + x as usize) * 3;
                rgb[o..o + 3].fill(255);
            }
        }
        // Artwork is deliberately much smaller than the full white page.
        for y in 220..650 {
            for x in 180..420 {
                let o = (y * 600 + x) * 3;
                rgb[o..o + 3].fill(0);
            }
        }
        rgb
    }
    #[test]
    fn preserves_page_white_margins() {
        let page = Region {
            x: 60,
            y: 90,
            width: 480,
            height: 720,
        };
        assert_eq!(
            pixel_region(600, 900, &image(page, 48), true).unwrap(),
            page
        );
    }
    #[test]
    fn rejects_ambiguous_white_on_white_instead_of_cropping_artwork() {
        assert!(pixel_region(
            600,
            900,
            &image(
                Region {
                    x: 60,
                    y: 90,
                    width: 480,
                    height: 720
                },
                255
            ),
            true
        )
        .is_err());
    }
    #[test]
    fn detects_a_page_touching_both_horizontal_edges() {
        let page = Region {
            x: 0,
            y: 22,
            width: 600,
            height: 856,
        };
        assert_eq!(
            pixel_region(600, 900, &image(page, 48), true).unwrap(),
            page
        );
    }
    #[test]
    fn detects_a_page_touching_both_vertical_edges() {
        let page = Region {
            x: 30,
            y: 0,
            width: 540,
            height: 900,
        };
        assert_eq!(
            pixel_region(600, 900, &image(page, 48), true).unwrap(),
            page
        );
    }
    #[test]
    fn no_gutters_is_ambiguous_without_element_geometry() {
        let page = Region {
            x: 0,
            y: 0,
            width: 600,
            height: 900,
        };
        assert!(pixel_region(600, 900, &image(page, 48), true).is_err());
    }

    fn green_background(page: Region) -> Vec<u8> {
        let mut rgb = vec![0; 600 * 900 * 3];
        for pixel in rgb.chunks_exact_mut(3) {
            pixel.copy_from_slice(&[202, 231, 209]);
        }
        for y in page.y..page.y + page.height {
            for x in page.x..page.x + page.width {
                let offset = (y as usize * 600 + x as usize) * 3;
                rgb[offset..offset + 3].fill(255);
            }
        }
        // Black artwork occupies only the middle, leaving original paper margins.
        for y in 220..650 {
            for x in 180..420 {
                let offset = (y * 600 + x) * 3;
                rgb[offset..offset + 3].fill(0);
            }
        }
        rgb
    }

    #[test]
    fn refines_a_padded_image_using_green_background_and_keeps_white_paper() {
        let page = Region {
            x: 60,
            y: 90,
            width: 480,
            height: 720,
        };
        let padded = Region {
            x: 60,
            y: 0,
            width: 480,
            height: 900,
        };
        let rgb = green_background(page);
        assert_eq!(pixel_region(600, 900, &rgb, true).unwrap(), page);
        assert_eq!(
            refine_reader_region(600, 900, &rgb, padded, true).unwrap(),
            page
        );
        assert_eq!(
            refine_reader_region(600, 900, &rgb, page, true).unwrap(),
            page
        );
    }

    #[test]
    fn does_not_crop_at_green_artwork_or_an_internal_green_stripe() {
        let page = Region {
            x: 60,
            y: 90,
            width: 480,
            height: 720,
        };
        let padded = Region {
            x: 60,
            y: 0,
            width: 480,
            height: 900,
        };
        let mut rgb = green_background(page);
        for y in 100..110 {
            for x in 60..540 {
                let offset = (y * 600 + x) * 3;
                rgb[offset..offset + 3].copy_from_slice(&[202, 231, 209]);
            }
        }
        // A center-out detector would stop at the internal stripe. Its proposed
        // crop is rejected because it would discard the paper above that stripe.
        assert_eq!(
            refine_reader_region(600, 900, &rgb, padded, true).unwrap(),
            padded
        );
    }

    #[test]
    fn does_not_guess_padding_from_white_or_gray_backgrounds() {
        let page = Region {
            x: 60,
            y: 90,
            width: 480,
            height: 720,
        };
        let padded = Region {
            x: 60,
            y: 0,
            width: 480,
            height: 900,
        };
        for background in [255, 0, 48] {
            assert_eq!(
                refine_reader_region(600, 900, &image(page, background), padded, true).unwrap(),
                padded
            );
        }
        assert!(refine_reader_region(600, 900, &[], padded, true).is_err());
    }

    #[test]
    fn refuses_to_trim_a_toolbar_or_other_non_background_pixels() {
        let page = Region {
            x: 60,
            y: 90,
            width: 480,
            height: 720,
        };
        let padded = Region {
            x: 60,
            y: 0,
            width: 480,
            height: 900,
        };
        let mut rgb = green_background(page);
        let offset = (10 * 600 + 300) * 3;
        rgb[offset..offset + 3].fill(0);
        assert_eq!(
            refine_reader_region(600, 900, &rgb, padded, true).unwrap(),
            padded
        );
    }
}
