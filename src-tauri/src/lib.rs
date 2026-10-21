mod amazon;
mod backup_settings;
mod capture;
mod capture_layout;
mod capture_log;
mod capture_output;
mod database;
mod kindle;
mod labels;
mod models;
mod series;

use database::Database;
use models::{
    AmazonContinueSeriesItemInput, AmazonContinueSeriesItemView, BookMetadataInput, BookView,
    SeriesMetadataInput, SeriesPreferences, SeriesPreferencesInput, SyncResult,
};
use tauri::{Emitter, Manager, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_opener::OpenerExt;

#[tauri::command]
fn list_books(
    window: WebviewWindow,
    database: State<'_, Database>,
) -> Result<Vec<BookView>, String> {
    require_main(&window)?;
    database.list_books()
}

#[tauri::command]
fn sync_kindle(
    window: WebviewWindow,
    database: State<'_, Database>,
    profile_path: Option<String>,
) -> Result<SyncResult, String> {
    require_main(&window)?;
    let source = kindle::locate_metadata(profile_path.as_deref())?;
    let (books, excluded) = kindle::load_books(&source)?;
    database.sync(&source, &books, excluded)
}

#[tauri::command]
fn save_book_metadata(
    window: WebviewWindow,
    database: State<'_, Database>,
    input: BookMetadataInput,
) -> Result<(), String> {
    require_main(&window)?;
    database.save_metadata(input)
}

#[tauri::command]
fn adjust_score(
    window: WebviewWindow,
    database: State<'_, Database>,
    book_id: i64,
    delta: i64,
) -> Result<i64, String> {
    require_main(&window)?;
    database.adjust_score(book_id, delta)
}

#[tauri::command]
fn set_favorite(
    window: WebviewWindow,
    database: State<'_, Database>,
    book_id: i64,
    favorite: bool,
) -> Result<(), String> {
    require_main(&window)?;
    database.set_favorite(book_id, favorite)
}

#[tauri::command]
fn update_series_metadata(
    window: WebviewWindow,
    database: State<'_, Database>,
    input: SeriesMetadataInput,
) -> Result<(), String> {
    require_main(&window)?;
    database.update_series_metadata(input)
}

#[tauri::command]
fn list_series_preferences(
    window: WebviewWindow,
    database: State<'_, Database>,
) -> Result<Vec<SeriesPreferences>, String> {
    require_main(&window)?;
    database.list_series_preferences()
}

#[tauri::command]
fn update_series_preferences(
    window: WebviewWindow,
    database: State<'_, Database>,
    input: SeriesPreferencesInput,
) -> Result<SeriesPreferences, String> {
    require_main(&window)?;
    database.update_series_preferences(input)
}

#[tauri::command]
fn open_in_kindle(
    window: WebviewWindow,
    app: tauri::AppHandle,
    asin: String,
    kind: String,
) -> Result<(), String> {
    require_main(&window)?;
    validate_asin(&asin)?;
    let url = kindle_reader_url(&asin, &kind)?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|error| error.to_string())
}

fn kindle_reader_url(asin: &str, kind: &str) -> Result<String, String> {
    match kind {
        "book" => Ok(format!("https://read.amazon.co.jp/?asin={asin}")),
        "manga" => Ok(format!("https://read.amazon.co.jp/manga/{asin}")),
        _ => Err("KindleのURL種別が不正です".to_owned()),
    }
}

#[tauri::command]
fn open_amazon_product_page(
    window: WebviewWindow,
    app: tauri::AppHandle,
    asin: String,
) -> Result<(), String> {
    require_main(&window)?;
    validate_asin(&asin)?;
    let url = format!("https://www.amazon.co.jp/dp/{asin}");
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|error| error.to_string())
}

fn validate_asin(asin: &str) -> Result<(), String> {
    if asin.len() != 10
        || !asin
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
    {
        return Err("ASIN が不正です".to_owned());
    }
    Ok(())
}

#[tauri::command]
fn open_kindle_library(window: WebviewWindow, app: tauri::AppHandle) -> Result<(), String> {
    require_main(&window)?;
    app.opener()
        .open_url("https://read.amazon.co.jp/", None::<&str>)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn open_kofi_donation(window: WebviewWindow, app: tauri::AppHandle) -> Result<(), String> {
    require_main(&window)?;
    app.opener()
        .open_url("https://ko-fi.com/ymkn3/donate", None::<&str>)
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn open_amazon_continue_series(
    window: WebviewWindow,
    app: tauri::AppHandle,
) -> Result<(), String> {
    require_main(&window)?;
    if let Some(existing) = app.get_webview_window(amazon::AMAZON_CONTINUE_SERIES_LABEL) {
        existing.reload().map_err(|error| error.to_string())?;
        return Ok(());
    }

    let data_directory = amazon_data_directory(&app)?;
    let url =
        tauri::Url::parse(&amazon::continue_series_url()).map_err(|error| error.to_string())?;
    let mut builder = WebviewWindowBuilder::new(
        &app,
        amazon::AMAZON_CONTINUE_SERIES_LABEL,
        WebviewUrl::External(url),
    )
    .title("Amazon 新刊チェック")
    .inner_size(1200.0, 800.0)
    .data_directory(data_directory)
    .initialization_script(amazon::AMAZON_CONTINUE_SERIES_IMPORT_SCRIPT)
    .visible(false);
    if let Some(main) = app.get_webview_window("main") {
        builder = builder.parent(&main).map_err(|error| error.to_string())?;
    }
    builder.build().map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
fn list_amazon_continue_series(
    window: WebviewWindow,
    database: State<'_, Database>,
) -> Result<Vec<AmazonContinueSeriesItemView>, String> {
    require_main(&window)?;
    database.list_amazon_continue_series()
}

#[tauri::command]
fn show_amazon_login(window: WebviewWindow) -> Result<(), String> {
    require_amazon_window(&window)?;
    let url = window.url().map_err(|error| error.to_string())?;
    if !amazon::is_sign_in_url(&url) {
        return Err("Amazonログイン画面以外からの表示要求を拒否しました".to_owned());
    }
    window.show().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())
}

#[tauri::command]
fn store_amazon_continue_series(
    window: WebviewWindow,
    app: tauri::AppHandle,
    database: State<'_, Database>,
    items: Vec<AmazonContinueSeriesItemInput>,
) -> Result<usize, String> {
    require_amazon_window(&window)?;
    let stored = database.replace_amazon_continue_series(items)?;
    app.emit("amazon-continue-series-updated", stored)
        .map_err(|error| error.to_string())?;
    window.close().map_err(|error| error.to_string())?;
    Ok(stored)
}

#[tauri::command]
fn clear_amazon_session(
    window: WebviewWindow,
    app: tauri::AppHandle,
    database: State<'_, Database>,
) -> Result<(), String> {
    require_main(&window)?;
    if let Some(amazon_window) = app.get_webview_window(amazon::AMAZON_CONTINUE_SERIES_LABEL) {
        amazon_window
            .clear_all_browsing_data()
            .map_err(|error| error.to_string())?;
        amazon_window.close().map_err(|error| error.to_string())?;
    } else {
        let clear_window = WebviewWindowBuilder::new(
            &app,
            amazon::AMAZON_CONTINUE_SERIES_LABEL,
            WebviewUrl::External(
                tauri::Url::parse("about:blank").map_err(|error| error.to_string())?,
            ),
        )
        .data_directory(amazon_data_directory(&app)?)
        .visible(false)
        .build()
        .map_err(|error| error.to_string())?;
        clear_window
            .clear_all_browsing_data()
            .map_err(|error| error.to_string())?;
        clear_window.close().map_err(|error| error.to_string())?;
    }
    database.clear_amazon_continue_series()
}

fn amazon_data_directory(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?
        .join("amazon-webview"))
}

fn require_amazon_window(window: &WebviewWindow) -> Result<(), String> {
    if window.label() == amazon::AMAZON_CONTINUE_SERIES_LABEL {
        Ok(())
    } else {
        Err("Amazon候補の取り込み元が不正です".to_owned())
    }
}

fn require_main(window: &WebviewWindow) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("この操作はメイン画面からのみ実行できます".to_owned())
    }
}

#[tauri::command]
fn list_capture_windows(
    window: WebviewWindow,
    app: tauri::AppHandle,
) -> Result<Vec<capture::CaptureWindow>, String> {
    require_main(&window)?;
    capture::list_windows(capture_reader_handle(&app))
}

const CAPTURE_READER_LABEL: &str = "kindle-capture-reader";

#[derive(Clone, Default)]
struct CaptureReaderSession(std::sync::Arc<std::sync::Mutex<Option<String>>>);

fn capture_reader_url(asin: &str) -> String {
    format!("https://read.amazon.co.jp/?asin={asin}")
}

fn reader_url_asin(url: &tauri::Url) -> Option<String> {
    if url.scheme() != "https" || url.host_str() != Some("read.amazon.co.jp") {
        return None;
    }
    if let Some((_, asin)) = url
        .query_pairs()
        .find(|(name, _)| name.eq_ignore_ascii_case("asin"))
    {
        return Some(asin.to_ascii_uppercase());
    }
    url.path()
        .strip_prefix("/manga/")
        .and_then(|path| path.split('/').next())
        .filter(|asin| !asin.is_empty())
        .map(str::to_ascii_uppercase)
}

fn reader_is_book(url: &tauri::Url, asin: &str) -> bool {
    reader_url_asin(url).is_some_and(|current| current.eq_ignore_ascii_case(asin))
}

fn capture_reader_handle(app: &tauri::AppHandle) -> Option<usize> {
    #[cfg(windows)]
    {
        app.get_webview_window(CAPTURE_READER_LABEL)?
            .hwnd()
            .ok()
            .map(|h| h.0 as usize)
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        None
    }
}

fn capture_reader_navigation_allowed(url: &tauri::Url) -> bool {
    url.scheme() == "https"
        && matches!(
            url.host_str(),
            Some("read.amazon.co.jp" | "www.amazon.co.jp" | "amazon.co.jp")
        )
}

#[tauri::command]
async fn open_capture_reader(
    window: WebviewWindow,
    app: tauri::AppHandle,
    manager: State<'_, capture::CaptureManager>,
    asin: String,
    portrait: Option<bool>,
) -> Result<capture::CaptureWindow, String> {
    require_main(&window)?;
    if manager.status()?.running {
        return Err("バックアップを停止してから本を開いてください".into());
    }
    open_capture_reader_window(&app, &asin, portrait.unwrap_or(true))
}

fn open_capture_reader_window(
    app: &tauri::AppHandle,
    asin: &str,
    portrait: bool,
) -> Result<capture::CaptureWindow, String> {
    validate_asin(asin)?;
    let url = tauri::Url::parse(&capture_reader_url(asin)).map_err(|e| e.to_string())?;
    let session = app.state::<CaptureReaderSession>().inner().clone();
    *session.0.lock().map_err(|e| e.to_string())? = Some(asin.to_owned());
    let reader = if let Some(reader) = app.get_webview_window(CAPTURE_READER_LABEL) {
        reader.navigate(url).map_err(|e| e.to_string())?;
        reader
    } else {
        // No initialization script or remote capability: the reader cannot invoke MMK commands.
        WebviewWindowBuilder::new(app, CAPTURE_READER_LABEL, WebviewUrl::External(url))
            .title("Kindle バックアップ用リーダー")
            .initialization_script(include_str!("reader_resume_prompt.js"))
            .inner_size(1200.0, 900.0)
            .data_directory(
                app.path()
                    .app_local_data_dir()
                    .map_err(|e| e.to_string())?
                    .join("kindle-reader-webview"),
            )
            .on_navigation(move |url| {
                if !capture_reader_navigation_allowed(url) {
                    return false;
                }
                let Ok(expected) = session.0.lock() else {
                    return false;
                };
                match (expected.as_deref(), reader_url_asin(url)) {
                    (Some(expected), Some(actual)) => actual.eq_ignore_ascii_case(expected),
                    _ => true, // Login redirects are allowed; capture rechecks the book URL.
                }
            })
            .build()
            .map_err(|e| e.to_string())?
    };
    reader.unminimize().map_err(|e| e.to_string())?;
    reader.show().map_err(|e| e.to_string())?;
    let handle = capture_reader_handle(app).ok_or("内蔵リーダーのウィンドウを取得できません")?;
    let result = capture::prepare_window(
        &capture::CaptureSetup {
            handle: handle.to_string(),
            process_id: std::process::id(),
            portrait,
        },
        Some(handle),
    )?;
    reader.set_focus().map_err(|e| e.to_string())?;
    Ok(result)
}

#[tauri::command]
async fn preview_capture_layout(
    window: WebviewWindow,
    app: tauri::AppHandle,
    setup: capture::CaptureSetup,
) -> Result<capture::CapturePreview, String> {
    require_main(&window)?;
    let reader_handle = capture_reader_handle(&app);
    let reader = if reader_handle.map(|h| h.to_string()).as_deref() == Some(setup.handle.as_str()) {
        app.get_webview_window(CAPTURE_READER_LABEL)
    } else {
        None
    };
    let manager = app.state::<capture::CaptureManager>().inner().clone();
    let result =
        tauri::async_runtime::spawn_blocking(move || manager.preview(setup, reader_handle, reader))
            .await
            .map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())?;
    result
}

#[tauri::command]
async fn choose_capture_root(
    window: WebviewWindow,
    app: tauri::AppHandle,
    current: String,
) -> Result<Option<String>, String> {
    require_main(&window)?;
    use tauri_plugin_dialog::DialogExt;
    let mut dialog = app
        .dialog()
        .file()
        .set_title("バックアップの保存先ルートフォルダー")
        .set_parent(&window);
    if !current.trim().is_empty() && std::path::Path::new(current.trim()).is_dir() {
        dialog = dialog.set_directory(current.trim());
    }
    tauri::async_runtime::spawn_blocking(move || {
        dialog
            .blocking_pick_folder()
            .map(|path| {
                path.into_path()
                    .map(|p| p.to_string_lossy().into_owned())
                    .map_err(|e| e.to_string())
            })
            .transpose()
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn clear_capture_reader_session(
    window: WebviewWindow,
    app: tauri::AppHandle,
    manager: State<'_, capture::CaptureManager>,
) -> Result<(), String> {
    require_main(&window)?;
    if manager.status()?.running {
        return Err("バックアップを停止してから連携を解除してください".into());
    }
    let reader = if let Some(reader) = app.get_webview_window(CAPTURE_READER_LABEL) {
        reader
    } else {
        WebviewWindowBuilder::new(
            &app,
            CAPTURE_READER_LABEL,
            WebviewUrl::External(tauri::Url::parse("about:blank").unwrap()),
        )
        .data_directory(
            app.path()
                .app_local_data_dir()
                .map_err(|e| e.to_string())?
                .join("kindle-reader-webview"),
        )
        .visible(false)
        .build()
        .map_err(|e| e.to_string())?
    };
    reader
        .clear_all_browsing_data()
        .map_err(|e| e.to_string())?;
    reader.close().map_err(|e| e.to_string())
}

#[tauri::command]
fn start_page_capture(
    window: WebviewWindow,
    app: tauri::AppHandle,
    manager: State<'_, capture::CaptureManager>,
    database: State<'_, Database>,
    mut options: capture::CaptureOptions,
) -> Result<(), String> {
    require_main(&window)?;
    let book = database
        .list_books()?
        .into_iter()
        .find(|book| {
            book.source_status == "active" && book.asin.eq_ignore_ascii_case(&options.asin)
        })
        .ok_or("バックアップ対象の書籍がライブラリにありません")?;
    options.asin = book.asin;
    options.book_title = book.title;
    options.series_name = book.effective_series_name;
    let directory = app
        .path()
        .picture_dir()
        .map_err(|e| e.to_string())?
        .join("MMK");
    let handle = capture_reader_handle(&app);
    let reader = if handle.map(|h| h.to_string()).as_deref() == Some(options.handle.as_str()) {
        app.get_webview_window(CAPTURE_READER_LABEL)
    } else {
        None
    };
    manager.start(options, directory, handle, reader)
}

#[tauri::command]
fn get_backup_settings(
    window: WebviewWindow,
    app: tauri::AppHandle,
) -> Result<serde_json::Value, String> {
    require_main(&window)?;
    let path = app
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?
        .join("backup-settings.json");
    let settings = backup_settings::load(&path)?;
    Ok(serde_json::json!({ "pageLayout": settings.page_layout, "configPath": path }))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupSettingsInput {
    page_layout: backup_settings::PageLayout,
}

#[tauri::command]
fn save_backup_settings(
    window: WebviewWindow,
    app: tauri::AppHandle,
    settings: BackupSettingsInput,
) -> Result<(), String> {
    require_main(&window)?;
    let path = app
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?
        .join("backup-settings.json");
    backup_settings::update_visible_settings(&path, settings.page_layout)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupRequest {
    book_ids: Vec<i64>,
    output_directory: String,
}

fn backup_root(
    app: &tauri::AppHandle,
    output_directory: &str,
) -> Result<std::path::PathBuf, String> {
    if output_directory.trim().is_empty() {
        return Ok(app
            .path()
            .picture_dir()
            .map_err(|e| e.to_string())?
            .join("MMK"));
    }
    let root = std::path::PathBuf::from(output_directory.trim());
    if !root.is_absolute() {
        return Err("保存先には絶対パスを指定してください".into());
    }
    Ok(root)
}

#[tauri::command]
async fn list_pending_backups(
    window: WebviewWindow,
    app: tauri::AppHandle,
    database: State<'_, Database>,
    output_directory: String,
    book_ids: Vec<i64>,
) -> Result<Vec<i64>, String> {
    require_main(&window)?;
    let books = database.list_books()?;
    let root = backup_root(&app, &output_directory)?;
    let selected: std::collections::HashSet<_> = book_ids.into_iter().collect();
    tauri::async_runtime::spawn_blocking(move || {
        let mut pending = Vec::new();
        for book in books
            .into_iter()
            .filter(|b| b.source_status == "active" && selected.contains(&b.id))
        {
            let path = capture_output::archive_path(
                &root,
                book.effective_series_name.as_deref(),
                &book.title,
                &book.asin,
            )?;
            if !capture_output::has_backup(&path, &book.asin)? {
                pending.push(book.id);
            }
        }
        Ok(pending)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn start_batch_capture(
    window: WebviewWindow,
    app: tauri::AppHandle,
    manager: State<'_, capture::CaptureManager>,
    database: State<'_, Database>,
    options: BackupRequest,
) -> Result<(), String> {
    require_main(&window)?;
    let config = app
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?
        .join("backup-settings.json");
    let settings = backup_settings::load(&config)?;
    let options = capture::BatchCaptureOptions {
        wait_timeout_ms: settings.wait_timeout_ms,
        book_ids: options.book_ids,
        output_directory: options.output_directory,
        key: settings.key,
        max_pages: settings.max_pages,
        portrait: settings.page_layout == backup_settings::PageLayout::Single,
    };
    if options.book_ids.is_empty() || options.book_ids.len() > 10_000 {
        return Err("バックアップ対象は1〜10000冊で指定してください".into());
    }
    let mut available: std::collections::HashMap<_, _> = database
        .list_books()?
        .into_iter()
        .filter(|book| book.source_status == "active")
        .map(|book| (book.id, book))
        .collect();
    let mut books = Vec::new();
    for id in &options.book_ids {
        let book = available
            .remove(id)
            .ok_or("対象に重複、またはライブラリに存在しない書籍があります")?;
        books.push(capture::BatchBook {
            asin: book.asin,
            title: book.title,
            series_name: book.effective_series_name,
        });
    }
    let directory = app
        .path()
        .picture_dir()
        .map_err(|e| e.to_string())?
        .join("MMK");
    manager.start_batch(options, books, directory, app)
}

#[tauri::command]
fn page_capture_status(
    window: WebviewWindow,
    manager: State<'_, capture::CaptureManager>,
) -> Result<capture::CaptureStatus, String> {
    require_main(&window)?;
    manager.status()
}

#[tauri::command]
fn stop_page_capture(
    window: WebviewWindow,
    manager: State<'_, capture::CaptureManager>,
) -> Result<(), String> {
    require_main(&window)?;
    manager.stop();
    Ok(())
}

#[tauri::command]
fn open_capture_directory(
    window: WebviewWindow,
    app: tauri::AppHandle,
    manager: State<'_, capture::CaptureManager>,
) -> Result<(), String> {
    require_main(&window)?;
    let directory = manager.status()?.output_directory;
    if directory.is_empty() {
        return Err("保存先はまだありません".into());
    }
    let path = std::path::Path::new(&directory);
    let directory = if path.is_file() {
        path.parent().ok_or("保存先が不正です")?
    } else {
        path
    };
    app.opener()
        .open_path(directory.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_reader_urls_for_books_and_manga() {
        assert_eq!(
            kindle_reader_url("B012345678", "book").unwrap(),
            "https://read.amazon.co.jp/?asin=B012345678"
        );
        assert_eq!(
            kindle_reader_url("B012345678", "manga").unwrap(),
            "https://read.amazon.co.jp/manga/B012345678"
        );
        assert!(kindle_reader_url("B012345678", "unknown").is_err());
    }

    #[test]
    fn capture_reader_only_navigates_to_amazon_japan_https() {
        for url in [
            "https://read.amazon.co.jp/manga/B012345678",
            "https://www.amazon.co.jp/ap/signin",
        ] {
            assert!(capture_reader_navigation_allowed(
                &tauri::Url::parse(url).unwrap()
            ));
        }
        for url in [
            "https://example.com",
            "https://www.amazon.co.jp.evil.test",
            "http://read.amazon.co.jp",
            "file:///C:/secret",
            "https://tauri.localhost",
        ] {
            assert!(!capture_reader_navigation_allowed(
                &tauri::Url::parse(url).unwrap()
            ));
        }
    }

    #[test]
    fn capture_reader_uses_classic_single_book_url() {
        assert_eq!(
            capture_reader_url("B012345678"),
            "https://read.amazon.co.jp/?asin=B012345678"
        );
        assert!(!capture_reader_url("B012345678").contains("/manga/"));
    }

    #[test]
    fn reader_guard_rejects_different_volume_and_non_book_urls() {
        assert!(reader_is_book(
            &tauri::Url::parse("https://read.amazon.co.jp/?asin=B012345678").unwrap(),
            "B012345678"
        ));
        for url in [
            "https://read.amazon.co.jp/?asin=B087654321",
            "https://read.amazon.co.jp/manga/B087654321",
            "https://read.amazon.co.jp/",
            "https://www.amazon.co.jp/ap/signin",
        ] {
            assert!(!reader_is_book(
                &tauri::Url::parse(url).unwrap(),
                "B012345678"
            ));
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(capture::CaptureManager::default())
        .manage(CaptureReaderSession::default())
        .setup(|app| {
            let database_dir = app.path().app_data_dir()?;
            let database_path = database_dir.join("mmk.sqlite3");
            let database = Database::initialize(database_path).map_err(std::io::Error::other)?;
            app.manage(database);
            app.state::<capture::CaptureManager>()
                .configure_logging(database_dir.join("logs").join("backup"));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_books,
            sync_kindle,
            save_book_metadata,
            adjust_score,
            set_favorite,
            update_series_metadata,
            list_series_preferences,
            update_series_preferences,
            open_in_kindle,
            open_amazon_product_page,
            open_kindle_library,
            open_kofi_donation,
            open_amazon_continue_series,
            list_amazon_continue_series,
            show_amazon_login,
            store_amazon_continue_series,
            clear_amazon_session,
            list_capture_windows,
            open_capture_reader,
            clear_capture_reader_session,
            start_page_capture,
            start_batch_capture,
            page_capture_status,
            stop_page_capture,
            open_capture_directory,
            preview_capture_layout,
            choose_capture_root,
            get_backup_settings,
            save_backup_settings,
            list_pending_backups
        ])
        .run(tauri::generate_context!())
        .expect("Manga Manager for Kindle の起動に失敗しました");
}
