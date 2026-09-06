mod amazon;
mod database;
mod kindle;
mod labels;
mod models;
mod series;

use database::Database;
use models::{
    AmazonContinueSeriesItemInput, AmazonContinueSeriesItemView, BookMetadataInput, BookView,
    SeriesMetadataInput, SyncResult,
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
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let database_dir = app.path().app_data_dir()?;
            let database_path = database_dir.join("mmk.sqlite3");
            let database = Database::initialize(database_path).map_err(std::io::Error::other)?;
            app.manage(database);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_books,
            sync_kindle,
            save_book_metadata,
            adjust_score,
            set_favorite,
            update_series_metadata,
            open_in_kindle,
            open_amazon_product_page,
            open_kindle_library,
            open_kofi_donation,
            open_amazon_continue_series,
            list_amazon_continue_series,
            show_amazon_login,
            store_amazon_continue_series,
            clear_amazon_session
        ])
        .run(tauri::generate_context!())
        .expect("Manga Manager for Kindle の起動に失敗しました");
}
