//! Back up the official reader's visible pages using its public reading-position UI.
//! Optional page cropping uses visible element geometry, never book data or protection APIs.
use crate::{capture_layout, capture_log, capture_output};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs::{self, OpenOptions},
    io::BufWriter,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::Manager;

fn default_wait_timeout() -> u64 {
    30_000
}

// Duplicate positions are never saved or used to declare completion.
const STABLE_LIMIT: u32 = 5;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureWindow {
    pub handle: String,
    pub process_id: u32,
    pub title: String,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureOptions {
    #[serde(default = "default_wait_timeout")]
    pub wait_timeout_ms: u64,
    pub handle: String,
    pub process_id: u32,
    pub asin: String,
    #[serde(default)]
    pub book_title: String,
    #[serde(default)]
    pub series_name: Option<String>,
    pub output_directory: String,
    pub region: Region,
    pub key: String,
    pub max_pages: u32,
    #[serde(default)]
    pub auto_page: bool,
    #[serde(default)]
    pub portrait: bool,
    #[serde(default)]
    pub from_beginning: bool,
    #[serde(default)]
    pub rewind_only: bool,
    // Internal evidence from this run's rewind, never supplied by the frontend.
    #[serde(skip)]
    confirmed_start: Option<ReadingPosition>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureSetup {
    pub handle: String,
    pub process_id: u32,
    pub portrait: bool,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchCaptureOptions {
    pub wait_timeout_ms: u64,
    pub book_ids: Vec<i64>,
    pub output_directory: String,
    pub key: String,
    pub max_pages: u32,
    pub portrait: bool,
}

pub struct BatchBook {
    pub asin: String,
    pub title: String,
    pub series_name: Option<String>,
}

impl BatchCaptureOptions {
    fn for_book(&self, book: &BatchBook) -> CaptureOptions {
        CaptureOptions {
            wait_timeout_ms: self.wait_timeout_ms,
            handle: String::new(),
            process_id: std::process::id(),
            asin: book.asin.clone(),
            book_title: book.title.clone(),
            series_name: book.series_name.clone(),
            output_directory: self.output_directory.clone(),
            region: Region {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            key: self.key.clone(),
            max_pages: self.max_pages,
            auto_page: true,
            portrait: self.portrait,
            from_beginning: true,
            rewind_only: false,
            confirmed_start: None,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapturePreview {
    pub window: CaptureWindow,
    pub region: Option<Region>,
    pub image: String,
    pub source: String,
    pub warning: Option<String>,
}

impl CaptureOptions {
    fn validate(&self) -> Result<(), String> {
        super::validate_asin(&self.asin)?;
        if !matches!(self.key.as_str(), "left" | "right" | "pagedown" | "space") {
            return Err("ページ送りキーが不正です".into());
        }
        if !(1000..=120_000).contains(&self.wait_timeout_ms)
            || !(1..=10_000).contains(&self.max_pages)
        {
            return Err(
                "描画待機の上限は1000〜120000ms、最大枚数は1〜10000枚で指定してください".into(),
            );
        }
        if self.region.x < 0 || self.region.y < 0 || self.region.width < 0 || self.region.height < 0
        {
            return Err("保存領域には0以上の値を指定してください".into());
        }
        if !self.output_directory.trim().is_empty()
            && !Path::new(self.output_directory.trim()).is_absolute()
        {
            return Err("保存先には絶対パスを指定してください".into());
        }
        if self.rewind_only && !self.from_beginning {
            return Err("先頭への移動が指定されていません".into());
        }
        Ok(())
    }
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureStatus {
    pub running: bool,
    pub saved_pages: u32,
    pub unchanged_count: u32,
    pub output_directory: String,
    pub message: String,
    pub detected_region: Option<Region>,
    pub batch: bool,
    pub total_books: usize,
    pub completed_books: usize,
    pub skipped_books: usize,
    pub current_title: String,
    pub current_position: Option<u32>,
    pub total_positions: Option<u32>,
    pub log_path: Option<String>,
    pub log_error: Option<String>,
}

#[derive(Clone, Default)]
pub struct CaptureManager {
    status: Arc<Mutex<CaptureStatus>>,
    cancel: Arc<AtomicBool>,
    log: Arc<Mutex<capture_log::CaptureLog>>,
}

impl CaptureManager {
    pub fn status(&self) -> Result<CaptureStatus, String> {
        let mut status = self
            .status
            .lock()
            .map(|s| s.clone())
            .map_err(|e| e.to_string())?;
        if let Ok(log) = self.log.lock() {
            status.log_path = log.path();
            status.log_error = log.error();
        }
        Ok(status)
    }

    pub fn configure_logging(&self, directory: PathBuf) {
        if let Ok(mut log) = self.log.lock() {
            log.configure(directory);
        }
    }

    fn begin_trace(&self, fields: serde_json::Value) {
        if let Ok(mut log) = self.log.lock() {
            log.start(fields);
            log.record(
                "runtime",
                json!({"appVersion":env!("CARGO_PKG_VERSION"), "processId":std::process::id()}),
            );
        }
    }

    fn trace(&self, event: &str, fields: serde_json::Value) {
        if let Ok(mut log) = self.log.lock() {
            log.record(event, fields);
        }
    }

    fn trace_failure(&self, stage: &str, error: &str) {
        self.trace(
            "operation_failed",
            json!({"stage":stage, "reason":capture_log::failure_reason(error)}),
        );
    }

    fn trace_step<T>(
        &self,
        stage: &str,
        operation: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let started = Instant::now();
        self.trace("operation_started", json!({"stage":stage}));
        let result = operation();
        match &result {
            Ok(_) => self.trace(
                "operation_finished",
                json!({"stage":stage, "durationMs":started.elapsed().as_millis() as u64}),
            ),
            Err(error) => self.trace_failure(stage, error),
        }
        result
    }

    pub fn stop(&self) {
        self.trace("stop_requested", json!({"source":"main_window"}));
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn start(
        &self,
        options: CaptureOptions,
        default_directory: PathBuf,
        reader_handle: Option<usize>,
        reader: Option<tauri::WebviewWindow>,
    ) -> Result<(), String> {
        options.validate()?;
        if reader.is_none() || !options.from_beginning || !options.auto_page {
            return Err("バックアップには内蔵の正規Kindle Cloud Readerを使用し、先頭から自動領域で保存してください".into());
        }
        let mut status = self.status.lock().map_err(|e| e.to_string())?;
        if status.running {
            return Err("すでにスクリーンショットを保存中です".into());
        }
        self.begin_trace(json!({"mode":"single", "pageLayout":if options.portrait {"single"} else {"spread"}, "key":options.key, "waitTimeoutMs":options.wait_timeout_ms, "maxPages":options.max_pages}));
        let selected = self.trace_step("prepare_window", || {
            platform::prepare_window(
                &CaptureSetup {
                    handle: options.handle.clone(),
                    process_id: options.process_id,
                    portrait: options.portrait,
                },
                reader_handle,
            )
        })?;
        let target = platform::Target::new(&options, reader_handle)?;
        platform::focus_window(&selected)?;
        self.trace(
            "window_ready",
            json!({"width":selected.width, "height":selected.height}),
        );
        let root = if options.output_directory.trim().is_empty() {
            default_directory
        } else {
            PathBuf::from(options.output_directory.trim())
        };
        let destination = capture_output::archive_path(
            &root,
            options.series_name.as_deref(),
            &options.book_title,
            &options.asin,
        )?;
        if destination.try_exists().map_err(|e| e.to_string())? && !options.rewind_only {
            return Err("この書籍のバックアップは既に存在します（上書きはしません）".into());
        }
        let directory = capture_output::work_directory(&root, &options.asin)?;
        self.cancel.store(false, Ordering::SeqCst);
        *status = CaptureStatus {
            running: true,
            output_directory: if options.rewind_only {
                String::new()
            } else {
                directory.to_string_lossy().into_owned()
            },
            message: "対象を前面にしました。ページの読み込み・描画完了を確認して開始します".into(),
            ..Default::default()
        };
        let manager = self.clone();
        let spawned = std::thread::Builder::new()
            .name("kindle-capture".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let _awake = platform::KeepAwake::new()?;
                    let end = run(&manager, &options, &target, &directory, reader.as_ref())?;
                    if end == CaptureEnd::LastPage {
                        publish_backup(&manager, &directory, &destination, &options.asin)?;
                    }
                    Ok::<_, String>(end)
                }));
                match &result {
                    Ok(Ok(end)) => manager.trace("run_finished", json!({"outcome":match end {CaptureEnd::LastPage=>"completed", CaptureEnd::Limit=>"page_limit", CaptureEnd::Rewound=>"rewound"}})),
                    Ok(Err(error)) => manager.trace_failure("single_run", error),
                    Err(_) => manager.trace("run_panicked", json!({"stage":"single_run"})),
                }
                if let Ok(mut status) = manager.status.lock() {
                    status.running = false;
                    status.message = match result {
                        Ok(Ok(end)) => end.message().into(),
                        Ok(Err(error)) => format!("停止: {error}"),
                        Err(_) => "バックアップ処理で予期しないエラーが発生しました".into(),
                    };
                }
            });
        if let Err(error) = spawned {
            self.trace_failure("spawn_worker", &error.to_string());
            status.running = false;
            status.message = format!("バックアップ処理を開始できません: {error}");
            return Err(status.message.clone());
        }
        Ok(())
    }

    pub fn start_batch(
        &self,
        options: BatchCaptureOptions,
        books: Vec<BatchBook>,
        default_directory: PathBuf,
        app: tauri::AppHandle,
    ) -> Result<(), String> {
        if books.is_empty() || books.len() > 10_000 {
            return Err("バックアップ対象は1〜10000冊で指定してください".into());
        }
        for book in &books {
            options.for_book(book).validate()?;
        }
        let root = if options.output_directory.trim().is_empty() {
            default_directory
        } else {
            PathBuf::from(options.output_directory.trim())
        };
        let mut status = self.status.lock().map_err(|e| e.to_string())?;
        if status.running {
            return Err("すでにバックアップ処理を実行中です".into());
        }
        fs::create_dir_all(&root).map_err(|e| format!("保存先を作成できません: {e}"))?;
        self.cancel.store(false, Ordering::SeqCst);
        *status = CaptureStatus {
            running: true,
            batch: true,
            total_books: books.len(),
            output_directory: root.to_string_lossy().into_owned(),
            message: "バックアップ済みデータを確認しています".into(),
            ..Default::default()
        };
        self.begin_trace(json!({"mode":"batch", "books":books.len(), "pageLayout":if options.portrait {"single"} else {"spread"}, "key":options.key, "waitTimeoutMs":options.wait_timeout_ms, "maxPages":options.max_pages}));
        let manager = self.clone();
        let spawned = std::thread::Builder::new()
            .name("kindle-batch-capture".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let _awake = platform::KeepAwake::new()?;
                    let mut previous_handle = None;
                    run_batch(&manager, &books, &root, |book, directory| {
                        // Do not steal focus back if the user interrupted between books.
                        wait(&manager, Duration::ZERO)?;
                        if let Some(handle) = previous_handle {
                            platform::check_foreground(handle)?;
                        }
                        let window = manager.trace_step("open_reader", || {
                            super::open_capture_reader_window(&app, &book.asin, options.portrait)
                        })?;
                        manager.trace(
                            "window_ready",
                            json!({"width":window.width, "height":window.height}),
                        );
                        let handle = super::capture_reader_handle(&app)
                            .ok_or("内蔵リーダーのウィンドウを取得できません")?;
                        let reader = app
                            .get_webview_window(super::CAPTURE_READER_LABEL)
                            .ok_or("内蔵リーダーが閉じられました")?;
                        previous_handle = Some(handle);
                        wait_for_batch_reader(
                            &manager,
                            &reader,
                            &book.asin,
                            handle,
                            options.portrait,
                        )?;
                        let mut single = options.for_book(book);
                        single.handle = handle.to_string();
                        let target = platform::Target::new(&single, Some(handle))?;
                        run(&manager, &single, &target, directory, Some(&reader))
                    })
                }));
                match &result {
                    Ok(Ok(())) => manager.trace("run_finished", json!({"outcome":"completed"})),
                    Ok(Err(error)) => manager.trace_failure("batch_run", error),
                    Err(_) => manager.trace("run_panicked", json!({"stage":"batch_run"})),
                }
                if let Ok(mut status) = manager.status.lock() {
                    status.running = false;
                    status.message = match result {
                        Ok(Ok(())) => format!(
                            "一括バックアップ終了: 完了 {}冊・バックアップ済みスキップ {}冊",
                            status.completed_books, status.skipped_books
                        ),
                        Ok(Err(error)) => format!("一括バックアップ停止: {error}"),
                        Err(_) => "一括バックアップで予期しないエラーが発生しました".into(),
                    };
                }
            });
        if let Err(error) = spawned {
            self.trace_failure("spawn_worker", &error.to_string());
            status.running = false;
            status.message = format!("一括バックアップを開始できません: {error}");
            return Err(status.message.clone());
        }
        Ok(())
    }

    pub fn preview(
        &self,
        setup: CaptureSetup,
        reader_handle: Option<usize>,
        reader: Option<tauri::WebviewWindow>,
    ) -> Result<CapturePreview, String> {
        let previous = {
            let mut status = self.status.lock().map_err(|e| e.to_string())?;
            if status.running {
                return Err("バックアップを停止してから自動調整してください".into());
            }
            let previous = status.clone();
            status.running = true;
            status.message = "ページ領域を自動判定しています".into();
            previous
        };
        self.cancel.store(false, Ordering::SeqCst);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let window = platform::prepare_window(&setup, reader_handle)?;
            platform::focus_window(&window)?;
            wait(self, Duration::from_millis(900))?;
            let options = CaptureOptions {
                wait_timeout_ms: default_wait_timeout(),
                handle: window.handle.clone(),
                process_id: window.process_id,
                asin: "B000000000".into(),
                book_title: String::new(),
                series_name: None,
                output_directory: String::new(),
                region: Region {
                    x: 0,
                    y: 0,
                    width: 0,
                    height: 0,
                },
                key: "left".into(),
                max_pages: 1,
                auto_page: true,
                portrait: false,
                from_beginning: false,
                rewind_only: false,
                confirmed_start: None,
            };
            let target = platform::Target::new(&options, reader_handle)?;
            prepare_reader_capture(self, &target, reader.as_ref())?;
            let frame = capture_without_resume(self, &target, options.region, reader.as_ref())?;
            let (region, source, warning) =
                match detect_region(&frame, reader.as_ref(), setup.portrait) {
                    Ok((region, source)) => (Some(region), source, None),
                    Err(error) => (
                        None,
                        "自動判定できませんでした",
                        Some(format!(
                            "{error}。リーダーの単ページ表示とページ背景色を確認してください"
                        )),
                    ),
                };
            let image = format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(frame.png_bytes()?)
            );
            Ok(CapturePreview {
                window,
                region,
                image,
                source: source.into(),
                warning,
            })
        }))
        .unwrap_or_else(|_| Err("ページ領域の自動判定中にエラーが発生しました".into()));
        if let Ok(mut status) = self.status.lock() {
            *status = previous;
        }
        result
    }
}

fn wait(manager: &CaptureManager, duration: Duration) -> Result<(), String> {
    let deadline = Instant::now() + duration;
    loop {
        if manager.cancel.load(Ordering::SeqCst) {
            return Err("手動で停止しました（保存済み画像は残ります）".into());
        }
        if platform::escape_pressed() {
            return Err("Escキーで停止しました（保存済み画像は残ります）".into());
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(());
        }
        std::thread::sleep(remaining.min(Duration::from_millis(50)));
    }
}

#[derive(Debug, PartialEq, Eq)]
enum CaptureEnd {
    LastPage,
    Limit,
    Rewound,
}

impl CaptureEnd {
    fn message(&self) -> &'static str {
        match self {
            Self::LastPage => "最終位置まで保存し、CBZバックアップを生成しました",
            Self::Limit => "保存枚数の上限に達しました。未完了のためCBZは生成していません",
            Self::Rewound => "先頭への移動処理を終えました。表紙・先頭ページか確認してください",
        }
    }
}

// Inspect only the configured output tree. Do not follow directory symlinks.
#[cfg(test)]
fn has_capture_data(
    directory: &Path,
    mut guard: impl FnMut() -> Result<(), String>,
) -> Result<bool, String> {
    let mut pending = vec![directory.to_path_buf()];
    while let Some(path) = pending.pop() {
        guard()?;
        let entries = match fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && path == directory => {
                return Ok(false)
            }
            Err(error) => {
                return Err(format!(
                    "撮影済みデータを確認できません（{}）: {error}",
                    path.display()
                ))
            }
        };
        for entry in entries {
            guard()?;
            let entry = entry.map_err(|e| e.to_string())?;
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            if kind.is_dir() {
                pending.push(entry.path());
            }
            if kind.is_file()
                && entry
                    .path()
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| {
                        ["png", "jpg", "jpeg", "webp", "bmp"]
                            .iter()
                            .any(|ext| e.eq_ignore_ascii_case(ext))
                    })
                && entry.metadata().map_err(|e| e.to_string())?.len() > 0
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn run_batch(
    manager: &CaptureManager,
    books: &[BatchBook],
    root: &Path,
    mut capture_book: impl FnMut(&BatchBook, &Path) -> Result<CaptureEnd, String>,
) -> Result<(), String> {
    for (index, book) in books.iter().enumerate() {
        wait(manager, Duration::ZERO)?;
        manager.trace("book_started", json!({"index":index + 1, "bookFingerprint":format!("{:x}", md5::compute(book.asin.as_bytes()))}));
        if let Ok(mut status) = manager.status.lock() {
            status.current_title = book.title.clone();
            status.saved_pages = 0;
            status.unchanged_count = 0;
            status.detected_region = None;
            status.current_position = None;
            status.total_positions = None;
            status.message = "バックアップ済みデータを確認しています".into();
        }
        let destination = capture_output::archive_path(
            root,
            book.series_name.as_deref(),
            &book.title,
            &book.asin,
        )?;
        if capture_output::has_backup(&destination, &book.asin)? {
            manager.trace("book_skipped", json!({"reason":"completed_archive_exists"}));
            if let Ok(mut status) = manager.status.lock() {
                status.skipped_books += 1;
                status.output_directory = destination.to_string_lossy().into_owned();
            }
            continue;
        }
        wait(manager, Duration::ZERO)?;
        let directory = capture_output::work_directory(root, &book.asin)?;
        if let Ok(mut status) = manager.status.lock() {
            status.saved_pages = 0;
            status.unchanged_count = 0;
            status.detected_region = None;
            status.output_directory = directory.to_string_lossy().into_owned();
            status.message = "内蔵リーダーで本を開いています".into();
        }
        let end = capture_book(book, &directory).map_err(|error| {
            manager.trace_failure("capture_book", &error);
            format!("{}: {error}", book.title)
        })?;
        wait(manager, Duration::ZERO)?;
        if end != CaptureEnd::LastPage {
            manager.trace(
                "book_stopped",
                json!({"reason":if end == CaptureEnd::Limit {"page_limit"} else {"not_completed"}}),
            );
            return Err(format!(
                "{}: {}。次の本には進みません",
                book.title,
                end.message()
            ));
        }
        publish_backup(manager, &directory, &destination, &book.asin)?;
        manager.trace("book_finished", json!({"index":index + 1}));
        if let Ok(mut status) = manager.status.lock() {
            status.completed_books += 1;
        }
    }
    Ok(())
}

fn publish_backup(
    manager: &CaptureManager,
    directory: &Path,
    destination: &Path,
    asin: &str,
) -> Result<(), String> {
    let pages = manager.status()?.saved_pages;
    if let Ok(mut status) = manager.status.lock() {
        status.message = "CBZバックアップを生成しています".into();
    }
    manager.trace_step("publish_archive", || {
        capture_output::finish_archive(directory, destination, asin, pages, || {
            wait(manager, Duration::ZERO)
        })
    })?;
    manager.trace("archive_completed", json!({"savedPages":pages}));
    if let Ok(mut status) = manager.status.lock() {
        status.output_directory = destination.to_string_lossy().into_owned();
    }
    Ok(())
}

fn wait_for_batch_reader(
    manager: &CaptureManager,
    reader: &tauri::WebviewWindow,
    asin: &str,
    handle: usize,
    single_page: bool,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(90);
    let started = Instant::now();
    manager.trace("reader_wait_started", json!({"timeoutMs":90_000}));
    let mut next_log = Instant::now();
    let mut ready_count = 0;
    while Instant::now() < deadline {
        wait(manager, Duration::from_millis(500))?;
        platform::check_foreground(handle)?;
        if capture_layout::decline_resume_prompt(reader)? {
            ready_count = 0;
            continue;
        }
        // Check the loaded document, not only the navigation URL (which may update first).
        let ready = capture_layout::reader_ready(reader, asin, single_page)?;
        if Instant::now() >= next_log {
            manager.trace(
                "reader_wait_state",
                json!({"ready":ready, "elapsedMs":started.elapsed().as_millis() as u64}),
            );
            next_log = Instant::now() + Duration::from_secs(2);
        }
        if ready {
            ready_count += 1;
            if ready_count >= 2 {
                manager.trace(
                    "reader_wait_finished",
                    json!({"durationMs":started.elapsed().as_millis() as u64}),
                );
                return Ok(());
            }
        } else {
            ready_count = 0;
        }
    }
    manager.trace("reader_wait_timeout", json!({"timeoutMs":90_000}));
    Err(
        "本文の読み込みが90秒以内に完了しませんでした。ログイン・通信・読書可否を確認してください"
            .into(),
    )
}

struct Frame {
    width: u32,
    height: u32,
    rgb: Vec<u8>,
}

impl Frame {
    fn png_bytes(&self) -> Result<Vec<u8>, String> {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, self.width, self.height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer
            .write_image_data(&self.rgb)
            .map_err(|e| e.to_string())?;
        writer.finish().map_err(|e| e.to_string())?;
        Ok(bytes)
    }

    fn crop(&self, region: Region) -> Result<Self, String> {
        let region = checked_region(region, self.width as i32, self.height as i32)?;
        let mut rgb = Vec::with_capacity(region.width as usize * region.height as usize * 3);
        for y in region.y..region.y + region.height {
            let offset = (y as usize * self.width as usize + region.x as usize) * 3;
            rgb.extend_from_slice(&self.rgb[offset..offset + region.width as usize * 3]);
        }
        Ok(Self {
            width: region.width as u32,
            height: region.height as u32,
            rgb,
        })
    }
    fn save(&self, path: &Path) -> Result<(), String> {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| e.to_string())?;
        let mut encoder = png::Encoder::new(BufWriter::new(file), self.width, self.height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer
            .write_image_data(&self.rgb)
            .map_err(|e| e.to_string())?;
        writer.finish().map_err(|e| e.to_string())
    }
}

fn run(
    manager: &CaptureManager,
    options: &CaptureOptions,
    target: &platform::Target,
    directory: &Path,
    reader: Option<&tauri::WebviewWindow>,
) -> Result<CaptureEnd, String> {
    let mut options = options.clone();
    manager.trace(
        "capture_started",
        json!({"pageLayout":if options.portrait {"single"} else {"spread"}, "key":options.key}),
    );
    if options.from_beginning {
        options.confirmed_start =
            Some(manager.trace_step("rewind", || rewind(manager, &options, target, reader))?);
        if options.rewind_only {
            return Ok(CaptureEnd::Rewound);
        }
    }
    prepare_reader_capture(manager, target, reader)?;
    let mut auto_state = AutoPageState {
        confirmed_start: options.confirmed_start,
        ..Default::default()
    };
    run_pages(
        manager,
        &options,
        directory,
        || {
            let reader = reader.ok_or("現在位置の確認には内蔵リーダーが必要です")?;
            let read_position = || {
                check_reader_book(Some(reader), &options.asin)?;
                reading_position(manager, target, reader)
            };
            let capture = || {
                manager.trace_step("capture_frame", || {
                if !options.auto_page {
                    return capture_without_resume(manager, target, options.region, Some(reader));
                }
                let frame = capture_without_resume(
                    manager,
                    target,
                    Region {
                        x: 0,
                        y: 0,
                        width: 0,
                        height: 0,
                    },
                    Some(reader),
                )?;
                let (region, _) = detect_region(&frame, Some(reader), options.portrait)?;
                manager.trace("capture_region", json!({"x":region.x, "y":region.y, "width":region.width, "height":region.height}));
                if let Ok(mut status) = manager.status.lock() {
                    status.detected_region = Some(region);
                }
                frame.crop(region)
            })
            };
            let observe = || {
                check_reader_book(Some(reader), &options.asin)?;
                capture_layout::reader_navigation(reader)
            };
            capture_when_ready(
                manager,
                &mut auto_state,
                Duration::from_millis(options.wait_timeout_ms),
                || {
                    check_reader_book(Some(reader), &options.asin)?;
                    platform::check_foreground(
                        options.handle.parse().map_err(|_| "ウィンドウが不正です")?,
                    )?;
                    capture_layout::reader_page_render(reader, &options.asin, options.portrait)
                },
                read_position,
                capture,
                observe,
            )
        },
        || {
            check_reader_book(reader, &options.asin)?;
            trace_key_input(manager, &options.key, "forward", || {
                target.advance(&options.key)
            })
        },
    )
}

fn prepare_reader_capture(
    manager: &CaptureManager,
    target: &platform::Target,
    reader: Option<&tauri::WebviewWindow>,
) -> Result<(), String> {
    let Some(reader) = reader else {
        return Ok(());
    };
    let ui = capture_layout::reader_navigation(reader)?;
    trace_navigation(manager, "capture_preparation", &ui);
    if ui.toc_open {
        return Err("目次が開いたままです。目次を閉じてから再試行してください".into());
    }
    if ui.known && ui.chrome_visible {
        manager.trace_step("hide_toolbar_click", || target.tap_center())?;
        let mut last_ui = ui;
        for _ in 0..15 {
            wait(manager, Duration::from_millis(100))?;
            last_ui = capture_layout::reader_navigation(reader)?;
            if !last_ui.chrome_visible {
                trace_navigation(manager, "toolbar_hidden", &last_ui);
                return Ok(());
            }
        }
        trace_navigation(manager, "toolbar_hide_timeout", &last_ui);
        return Err(
            "操作バーを閉じられません。本文中央をクリックしてから再試行してください".into(),
        );
    }
    Ok(())
}

fn trace_navigation(manager: &CaptureManager, stage: &str, ui: &capture_layout::ReaderNavigation) {
    manager.trace("reader_ui", json!({"stage":stage, "known":ui.known, "toolbarVisible":ui.chrome_visible, "toolbar":ui.toolbar, "tocOpen":ui.toc_open, "tocCloseVisible":ui.toc_close_visible, "start":ui.position, "end":ui.position_end, "total":ui.total}));
}

fn trace_key_input(
    manager: &CaptureManager,
    key: &str,
    purpose: &str,
    send: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    manager.trace("key_send_requested", json!({"key":key, "purpose":purpose}));
    let started = Instant::now();
    let result = send();
    match &result {
        // Accepted by SendInput is not evidence that the reader changed pages.
        Ok(()) => manager.trace("key_send_accepted", json!({"key":key, "purpose":purpose, "durationMs":started.elapsed().as_millis() as u64})),
        Err(error) => manager.trace_failure("key_send", error),
    }
    result
}

fn check_reader_book(reader: Option<&tauri::WebviewWindow>, asin: &str) -> Result<(), String> {
    if let Some(reader) = reader {
        if !super::reader_is_book(&reader.url().map_err(|e| e.to_string())?, asin) {
            return Err(
                "選択した本以外の画面へ変わったため終了しました。次の巻は保存しません".into(),
            );
        }
    }
    Ok(())
}

fn capture_without_resume(
    manager: &CaptureManager,
    target: &platform::Target,
    region: Region,
    reader: Option<&tauri::WebviewWindow>,
) -> Result<Frame, String> {
    for _ in 0..5 {
        wait(manager, Duration::ZERO)?;
        if let Some(reader) = reader {
            if capture_layout::decline_resume_prompt(reader)? {
                wait(manager, Duration::from_millis(350))?;
                continue;
            }
        }
        let frame = target.capture(region)?;
        if let Some(reader) = reader {
            // A late resume prompt must never become a saved book page.
            if capture_layout::decline_resume_prompt(reader)? {
                wait(manager, Duration::from_millis(350))?;
                continue;
            }
        }
        return Ok(frame);
    }
    Err("読書位置の確認画面が閉じるまで待ちましたが、完了しませんでした".into())
}

fn rewind(
    manager: &CaptureManager,
    options: &CaptureOptions,
    target: &platform::Target,
    reader: Option<&tauri::WebviewWindow>,
) -> Result<ReadingPosition, String> {
    check_reader_book(reader, &options.asin)?;
    let reader = reader.ok_or("先頭位置の確認には内蔵リーダーが必要です")?;
    if capture_layout::decline_resume_prompt(reader)? {
        wait(manager, Duration::from_millis(350))?;
    }
    if let Ok(mut status) = manager.status.lock() {
        status.message = "先頭へ戻しています（この間は保存しません）".into();
    }
    let ui = capture_layout::reader_navigation(reader)?;
    if ui.known && !ui.chrome_visible && !ui.toc_open {
        target.tap_center()?;
        wait(manager, Duration::from_millis(400))?;
    }
    let requested_cover = manager.trace_step("select_toc_start", || {
        capture_layout::try_go_to_cover(reader, &options.book_title)
    })?;
    manager.trace("toc_start_result", json!({"selected":requested_cover}));
    let start = confirm_reader_start(
        manager,
        requested_cover,
        || {
            let deadline = Instant::now() + Duration::from_millis(options.wait_timeout_ms);
            wait_for_render(manager, None, deadline, &mut || {
                check_reader_book(Some(reader), &options.asin)?;
                platform::check_foreground(
                    options.handle.parse().map_err(|_| "ウィンドウが不正です")?,
                )?;
                capture_layout::reader_page_render(reader, &options.asin, options.portrait)
            })?;
            Ok(())
        },
        || {
            check_reader_book(Some(reader), &options.asin)?;
            platform::check_foreground(
                options.handle.parse().map_err(|_| "ウィンドウが不正です")?,
            )?;
            // TOC navigation can still be loading when its side panel closes.
            // Do not click the toolbar until the new page has finished painting.
            prepare_reader_capture(manager, target, Some(reader))?;
            reading_position(manager, target, reader)
        },
    )?;
    manager.trace(
        "start_confirmed",
        json!({"start":start.start, "end":start.end, "total":start.total}),
    );
    if let Ok(mut status) = manager.status.lock() {
        status.message = format!("確認した先頭位置 {} から開始します", start.start);
    }
    Ok(start)
}

// The selected first TOC entry defines the beginning, even when numbered 2.
// Wait for rendering and read it once, without issuing page-turn probe inputs.
fn confirm_reader_start(
    manager: &CaptureManager,
    selected_toc_start: bool,
    ready: impl FnOnce() -> Result<(), String>,
    read: impl FnOnce() -> Result<ReadingPosition, String>,
) -> Result<ReadingPosition, String> {
    wait(manager, Duration::ZERO)?;
    if !selected_toc_start {
        return Err(
            "目次の最上位項目への移動を確認できないため、バックアップを開始しません".into(),
        );
    }
    ready()?;
    wait(manager, Duration::ZERO)?;
    let start = read()?;
    validate_reading_position(start)?;
    wait(manager, Duration::ZERO)?;
    Ok(start)
}

fn detect_region(
    frame: &Frame,
    reader: Option<&tauri::WebviewWindow>,
    single_page: bool,
) -> Result<(Region, &'static str), String> {
    if let Some(reader) = reader {
        if let Some(region) =
            capture_layout::reader_region(reader, frame.width, frame.height, single_page)?
        {
            let refined = capture_layout::refine_reader_region(
                frame.width,
                frame.height,
                &frame.rgb,
                region,
                single_page,
            )?;
            if refined != region {
                return Ok((refined, "表示要素と背景色のページ境界"));
            }
            return Ok((region, "表示要素のページ境界"));
        }
    }
    capture_layout::pixel_region(frame.width, frame.height, &frame.rgb, single_page)
        .map(|region| (region, "画面と背景の境界"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ReadingPosition {
    start: u32,
    end: u32,
    total: u32,
}

fn validate_reading_position(position: ReadingPosition) -> Result<(), String> {
    if position.start == 0 || position.end < position.start || position.end > position.total {
        return Err("読書位置が不正です".into());
    }
    Ok(())
}

fn validate_start(
    position: ReadingPosition,
    confirmed: Option<ReadingPosition>,
) -> Result<(), String> {
    validate_reading_position(position)?;
    if confirmed.map_or(position.start != 1, |start| start != position) {
        return Err(
            "先頭移動で確認した開始位置と一致しないため、バックアップを開始しません".into(),
        );
    }
    Ok(())
}

#[derive(Default)]
struct AutoPageState {
    token: Option<u64>,
    position: Option<ReadingPosition>,
    confirmed_start: Option<ReadingPosition>,
}

fn wait_for_render(
    manager: &CaptureManager,
    after: Option<u64>,
    deadline: Instant,
    observe: &mut impl FnMut() -> Result<capture_layout::PageRender, String>,
) -> Result<capture_layout::PageRender, String> {
    let started = Instant::now();
    manager.trace("render_wait_started", json!({"afterToken":after}));
    let mut previous: Option<capture_layout::PageRender> = None;
    let mut next_heartbeat = Instant::now();
    let mut polls = 0u64;
    loop {
        wait(manager, Duration::ZERO)?;
        let render = observe().map_err(|error| {
            manager.trace_failure("render_observation", &error);
            error
        })?;
        polls += 1;
        let changed = previous.map_or(true, |before| {
            before.ready != render.ready
                || before.error != render.error
                || before.waiting_for != render.waiting_for
                || before.image_count != render.image_count
        });
        if changed || Instant::now() >= next_heartbeat {
            manager.trace("render_wait_state", json!({"ready":render.ready, "token":render.token, "afterToken":after, "waitingForUpdate":after == Some(render.token), "waitingFor":render.waiting_for, "imageCount":render.image_count, "error":render.error, "polls":polls, "durationMs":started.elapsed().as_millis() as u64}));
            next_heartbeat = Instant::now() + Duration::from_secs(2);
            previous = Some(render);
        }
        match render.error {
            Some(capture_layout::PageRenderError::WrongBook) => {
                return Err("読み込み待機中に別の書籍・画面へ移動しました".into())
            }
            // TOC navigation can briefly expose only a canvas before the image
            // appears. Observe until the existing deadline; never capture canvas.
            Some(capture_layout::PageRenderError::CanvasNotSupported) => {}
            None => {}
        }
        if render.ready && render.error.is_none() && after != Some(render.token) {
            manager.trace("render_wait_finished", json!({"token":render.token, "polls":polls, "durationMs":started.elapsed().as_millis() as u64}));
            return Ok(render);
        }
        if Instant::now() >= deadline {
            manager.trace("render_wait_timeout", json!({"ready":render.ready, "token":render.token, "afterToken":after, "waitingForUpdate":after == Some(render.token), "waitingFor":render.waiting_for, "polls":polls, "durationMs":started.elapsed().as_millis() as u64}));
            if render.error == Some(capture_layout::PageRenderError::CanvasNotSupported) {
                return Err(
                    "この描画形式では画像の読み込み完了を確認できないため、バックアップできません"
                        .into(),
                );
            }
            return Err("ページの更新・画像読み込み・描画完了を制限時間内に確認できませんでした。未完了のためCBZは生成しません".into());
        }
        // This is a readiness poll, not a mandatory page delay. Return as soon
        // as the decoded image has been painted in two animation frames.
        wait(manager, Duration::from_millis(16))?;
    }
}

fn capture_when_ready(
    manager: &CaptureManager,
    state: &mut AutoPageState,
    timeout: Duration,
    mut render: impl FnMut() -> Result<capture_layout::PageRender, String>,
    mut position: impl FnMut() -> Result<ReadingPosition, String>,
    mut capture: impl FnMut() -> Result<Frame, String>,
    mut navigation: impl FnMut() -> Result<capture_layout::ReaderNavigation, String>,
) -> Result<(Frame, ReadingPosition), String> {
    let deadline = Instant::now() + timeout;
    loop {
        wait_for_render(manager, state.token, deadline, &mut render)?;
        let current = position()?;
        manager.trace("position_confirmed", json!({"start":current.start, "end":current.end, "total":current.total, "previousStart":state.position.map(|p| p.start), "previousEnd":state.position.map(|p| p.end)}));
        validate_reading_position(current)?;
        if let Some(previous) = state.position {
            if previous == current {
                if Instant::now() >= deadline {
                    return Err(
                        "ページ送り後も現在位置が変わりません。未完了のためCBZは生成しません"
                            .into(),
                    );
                }
                wait(manager, Duration::from_millis(16))?;
                continue;
            }
            // Screens must be contiguous: a spread starts right after the
            // previous screen's last page. Any gap or reversal stops the run.
            if previous.total != current.total || current.start != previous.end + 1 {
                return Err(
                    "総数の変更・位置の逆行・ページの飛び越しを検出したため停止しました".into(),
                );
            }
        } else {
            validate_start(current, state.confirmed_start)?;
        }
        // Opening/closing the normal toolbar may itself trigger an image
        // update. Wait for that paint without reopening it or rereading position.
        let before = wait_for_render(manager, None, deadline, &mut render)?;
        let (frame, current) =
            capture_with_position(|| Ok(current), &mut capture, &mut navigation)?;
        let after = render()?;
        wait(manager, Duration::ZERO)?;
        if !after.ready || after.token != before.token || after.error.is_some() {
            manager.trace("capture_discarded", json!({"reason":"render_changed_during_capture", "beforeToken":before.token, "afterToken":after.token, "ready":after.ready}));
            if Instant::now() >= deadline {
                return Err(
                    "スクリーンショット取得中の描画更新が完了しませんでした。画像は保存しません"
                        .into(),
                );
            }
            continue;
        }
        state.token = Some(after.token);
        state.position = Some(current);
        manager.trace("frame_confirmed", json!({"start":current.start, "end":current.end, "total":current.total, "token":after.token}));
        return Ok((frame, current));
    }
}

fn capture_with_position(
    read_position: impl FnOnce() -> Result<ReadingPosition, String>,
    capture: impl FnOnce() -> Result<Frame, String>,
    observe: impl FnOnce() -> Result<capture_layout::ReaderNavigation, String>,
) -> Result<(Frame, ReadingPosition), String> {
    // Confirm each page once, before capturing it. No page-turn input is sent
    // until run_pages has saved the image. The post-capture check is read-only:
    // missing hidden indicators must not open the toolbar a second time.
    let position = read_position()?;
    let frame = capture()?;
    let navigation = observe()?;
    if !navigation.known || navigation.toc_open || navigation.chrome_visible {
        return Err(
            "スクリーンショット取得中に本文以外のUIが表示されました。画像は保存しません".into(),
        );
    }
    if navigation
        .position
        .is_some_and(|current| current != position.start)
        || navigation
            .total
            .is_some_and(|total| total != position.total)
    {
        return Err("スクリーンショット取得中に読書位置が変わりました".into());
    }
    Ok((frame, position))
}

fn reading_position(
    manager: &CaptureManager,
    target: &platform::Target,
    reader: &tauri::WebviewWindow,
) -> Result<ReadingPosition, String> {
    let started = Instant::now();
    let result = read_position_with_toolbar(
        manager,
        || capture_layout::reader_navigation(reader),
        || target.tap_center(),
        Duration::from_millis(16),
    );
    match &result {
        Ok(position) => manager.trace("position_read", json!({"start":position.start, "end":position.end, "total":position.total, "durationMs":started.elapsed().as_millis() as u64})),
        Err(error) => manager.trace_failure("position_read", error),
    }
    result
}

fn navigation_position(
    navigation: &capture_layout::ReaderNavigation,
) -> Result<ReadingPosition, String> {
    if !navigation.known || navigation.toc_open {
        return Err("本文の表示状態を確認できません。目次を閉じて再試行してください".into());
    }
    let (Some(start), Some(total)) = (navigation.position, navigation.total) else {
        return Err(format!(
            "現在位置と総数を取得できません（現在位置: {}、総数: {}、位置表示: {}）。画像比較で完了とは判定せず停止します",
            navigation.position.map_or_else(|| "取得不可".into(), |n| n.to_string()),
            navigation.total.map_or_else(|| "取得不可".into(), |n| n.to_string()),
            if navigation.position_texts.is_empty() { "表示なし".into() } else { navigation.position_texts.join(" / ") },
        ));
    };
    // A spread reports a start-end range; single pages report no end.
    let end = navigation.position_end.unwrap_or(start);
    if start == 0 || end < start || end > total {
        return Err(format!(
            "現在位置と総数を取得できません（現在位置: {start}、終端: {end}、総数: {total}、位置表示: {}）。画像比較で完了とは判定せず停止します",
            if navigation.position_texts.is_empty() { "表示なし".into() } else { navigation.position_texts.join(" / ") },
        ));
    }
    Ok(ReadingPosition { start, end, total })
}

fn read_position_with_toolbar(
    manager: &CaptureManager,
    mut observe: impl FnMut() -> Result<capture_layout::ReaderNavigation, String>,
    mut toggle: impl FnMut() -> Result<(), String>,
    poll_interval: Duration,
) -> Result<ReadingPosition, String> {
    wait(manager, Duration::ZERO)?;
    let mut navigation = observe()?;
    trace_navigation(manager, "position_read_initial", &navigation);
    if !navigation.known || navigation.toc_open {
        return navigation_position(&navigation);
    }
    if let Ok(position) = navigation_position(&navigation) {
        return Ok(position);
    }
    // Kindle can remove the position indicator when hiding its chrome. Reveal
    // ordinary UI temporarily, and always restore it before taking a screenshot.
    let opened = !navigation.chrome_visible;
    if opened {
        wait(manager, Duration::ZERO)?;
        manager.trace_step("show_position_toolbar", &mut toggle)?;
    }
    let result = (|| {
        for _ in 0..125 {
            wait(manager, poll_interval)?;
            navigation = observe()?;
            if !navigation.known || navigation.toc_open {
                return navigation_position(&navigation);
            }
            if navigation.chrome_visible {
                if let Ok(position) = navigation_position(&navigation) {
                    return Ok(position);
                }
            }
        }
        Err(navigation_position(&navigation)
            .err()
            .unwrap_or_else(|| "位置確認のための操作バーを表示できませんでした".into()))
    })();
    if opened {
        wait(manager, Duration::ZERO)?;
        let mut navigation = observe()?;
        if navigation.known && !navigation.toc_open && navigation.chrome_visible {
            manager.trace_step("hide_position_toolbar", &mut toggle)?;
            let mut closed = false;
            for _ in 0..125 {
                wait(manager, poll_interval)?;
                navigation = observe()?;
                if !navigation.chrome_visible {
                    trace_navigation(manager, "position_toolbar_hidden", &navigation);
                    closed = true;
                    break;
                }
            }
            if !closed {
                trace_navigation(manager, "position_toolbar_hide_timeout", &navigation);
                return Err(
                    "位置確認後に操作バーを閉じられません。スクリーンショットは保存しません".into(),
                );
            }
        }
    }
    result
}

fn run_pages(
    manager: &CaptureManager,
    options: &CaptureOptions,
    directory: &Path,
    mut capture: impl FnMut() -> Result<(Frame, ReadingPosition), String>,
    mut advance: impl FnMut() -> Result<(), String>,
) -> Result<CaptureEnd, String> {
    let mut previous: Option<ReadingPosition> = None;
    let mut saved = 0;
    let mut unchanged = 0;
    loop {
        wait(manager, Duration::ZERO)?;
        let (current, position) = capture()?;
        wait(manager, Duration::ZERO)?;
        validate_reading_position(position)?;
        if let Some(before) = &previous {
            if before == &position {
                unchanged += 1;
            } else if before.total != position.total || position.start != before.end + 1 {
                // Screens must be contiguous; a gap or reversal stops the whole run.
                return Err(
                    "総数の変更・位置の逆行・ページの飛び越しを検出したため停止しました".into(),
                );
            } else {
                unchanged = 0;
            }
        } else {
            validate_start(position, options.confirmed_start)?;
        }
        // Distinct positions are saved even when the artwork is identical.
        if unchanged == 0 {
            let number = saved + 1;
            current.save(&directory.join(format!("screenshot_{number:04}.png")))?;
            saved += 1;
            manager.trace("page_saved", json!({"savedPages":saved, "start":position.start, "end":position.end, "total":position.total}));
        }
        previous = Some(position);
        if let Ok(mut status) = manager.status.lock() {
            status.saved_pages = saved;
            status.unchanged_count = unchanged;
            status.current_position = Some(position.start);
            status.total_positions = Some(position.total);
            status.message = format!(
                "{saved}枚保存済み・現在位置 {}-{}/{}",
                position.start, position.end, position.total
            );
        }
        if position.end == position.total {
            manager.trace(
                "last_position_saved",
                json!({"savedPages":saved, "total":position.total}),
            );
            return Ok(CaptureEnd::LastPage);
        }
        if unchanged >= STABLE_LIMIT {
            return Err(
                "ページ送り後も現在位置が変わりません。未完了のためCBZは生成しません".into(),
            );
        }
        if saved >= options.max_pages {
            manager.trace(
                "page_limit_reached",
                json!({"savedPages":saved, "maxPages":options.max_pages}),
            );
            return Ok(CaptureEnd::Limit);
        }
        // Recheck focus immediately before input and after the rendering wait.
        wait(manager, Duration::ZERO)?;
        // A delayed turn must not receive another key and skip a page.
        if unchanged == 0 {
            advance()?;
        }
    }
}

fn checked_region(region: Region, width: i32, height: i32) -> Result<Region, String> {
    let region = if region.width == 0 || region.height == 0 {
        Region {
            x: 0,
            y: 0,
            width,
            height,
        }
    } else {
        region
    };
    if region.x < 0
        || region.y < 0
        || region.width <= 0
        || region.height <= 0
        || region
            .x
            .checked_add(region.width)
            .map_or(true, |right| right > width)
        || region
            .y
            .checked_add(region.height)
            .map_or(true, |bottom| bottom > height)
        || i64::from(region.width) * i64::from(region.height) > 40_000_000
    {
        return Err("保存領域がウィンドウの範囲外、または大きすぎます".into());
    }
    Ok(region)
}

pub use platform::{list_windows, prepare_window};

#[cfg(windows)]
mod platform {
    use super::*;
    use std::{
        mem::{size_of, zeroed},
        ptr::null_mut,
    };
    use windows_sys::Win32::{
        Foundation::{HWND, LPARAM, POINT, RECT},
        Graphics::Gdi::*,
        UI::{HiDpi::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    };

    // Thread-local DPI awareness keeps coordinates and pixels consistent across monitors.
    struct DpiGuard(DPI_AWARENESS_CONTEXT);
    impl DpiGuard {
        unsafe fn new() -> Self {
            Self(SetThreadDpiAwarenessContext(
                DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
            ))
        }
    }
    impl Drop for DpiGuard {
        fn drop(&mut self) {
            unsafe {
                if !self.0.is_null() {
                    SetThreadDpiAwarenessContext(self.0);
                }
            }
        }
    }

    struct WindowListing {
        windows: Vec<CaptureWindow>,
        reader_handle: Option<usize>,
    }

    pub fn list_windows(reader_handle: Option<usize>) -> Result<Vec<CaptureWindow>, String> {
        let mut listing = WindowListing {
            windows: Vec::new(),
            reader_handle,
        };
        unsafe {
            let _dpi = DpiGuard::new();
            if EnumWindows(Some(enumerate), &mut listing as *mut _ as LPARAM) == 0 {
                return Err("ウィンドウ一覧を取得できませんでした".into());
            }
        }
        listing.windows.sort_by(|a, b| a.title.cmp(&b.title));
        Ok(listing.windows)
    }

    unsafe extern "system" fn enumerate(hwnd: HWND, data: LPARAM) -> i32 {
        if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 {
            return 1;
        }
        let mut process_id = 0;
        GetWindowThreadProcessId(hwnd, &mut process_id);
        let listing = &mut *(data as *mut WindowListing);
        // Only the dedicated reader is allowed from this process, never MMK or the Amazon checker.
        if process_id == std::process::id() && listing.reader_handle != Some(hwnd as usize) {
            return 1;
        }
        let mut text = [0u16; 1024];
        let length = GetWindowTextW(hwnd, text.as_mut_ptr(), text.len() as i32);
        let mut rect: RECT = zeroed();
        if length > 0 && GetClientRect(hwnd, &mut rect) != 0 && rect.right > 0 && rect.bottom > 0 {
            listing.windows.push(CaptureWindow {
                handle: (hwnd as usize).to_string(),
                process_id,
                title: String::from_utf16_lossy(&text[..length as usize]),
                width: rect.right,
                height: rect.bottom,
            });
        }
        1
    }

    pub fn escape_pressed() -> bool {
        unsafe { GetAsyncKeyState(VK_ESCAPE as i32) < 0 }
    }

    // Execution-state requests are thread-local and are released on every exit path.
    pub struct KeepAwake;
    impl KeepAwake {
        pub fn new() -> Result<Self, String> {
            use windows_sys::Win32::System::Power::*;
            if unsafe {
                SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED)
            } == 0
            {
                return Err("バックアップ中のスリープ・画面消灯抑止を設定できませんでした".into());
            }
            Ok(Self)
        }
    }
    impl Drop for KeepAwake {
        fn drop(&mut self) {
            use windows_sys::Win32::System::Power::*;
            unsafe {
                SetThreadExecutionState(ES_CONTINUOUS);
            }
        }
    }

    pub fn check_foreground(handle: usize) -> Result<(), String> {
        unsafe {
            let hwnd = handle as HWND;
            if IsWindow(hwnd) == 0 || IsIconic(hwnd) != 0 || GetForegroundWindow() != hwnd {
                return Err("内蔵リーダーが閉じられたか、最小化・フォーカス変更されました".into());
            }
        }
        Ok(())
    }

    pub fn prepare_window(
        setup: &CaptureSetup,
        reader_handle: Option<usize>,
    ) -> Result<CaptureWindow, String> {
        let selected = list_windows(reader_handle)?
            .into_iter()
            .find(|w| w.handle == setup.handle && w.process_id == setup.process_id)
            .ok_or("対象ウィンドウが見つかりません。一覧を更新してください")?;
        if setup.portrait {
            unsafe {
                let _dpi = DpiGuard::new();
                let hwnd = setup
                    .handle
                    .parse::<usize>()
                    .map_err(|_| "ウィンドウが不正です")? as HWND;
                if IsZoomed(hwnd) != 0 {
                    ShowWindow(hwnd, SW_RESTORE);
                }
                let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                let mut info: MONITORINFO = zeroed();
                info.cbSize = size_of::<MONITORINFO>() as u32;
                let mut outer: RECT = zeroed();
                let mut client: RECT = zeroed();
                if GetMonitorInfoW(monitor, &mut info) == 0
                    || GetWindowRect(hwnd, &mut outer) == 0
                    || GetClientRect(hwnd, &mut client) == 0
                {
                    return Err("ディスプレイの作業領域を取得できませんでした".into());
                }
                let work = info.rcWork;
                let border_width = (outer.right - outer.left - client.right).max(0);
                let border_height = (outer.bottom - outer.top - client.bottom).max(0);
                let (width, height) = crate::capture_layout::portrait_size(
                    work.right - work.left,
                    work.bottom - work.top,
                    border_width,
                    border_height,
                )?;
                let x = work.left + (work.right - work.left - width) / 2;
                if SetWindowPos(
                    hwnd,
                    null_mut(),
                    x,
                    work.top,
                    width,
                    height,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                ) == 0
                {
                    return Err("縦長ウィンドウへ調整できませんでした".into());
                }
            }
        } else {
            // A spread needs the widest client area the screen allows.
            unsafe {
                let _dpi = DpiGuard::new();
                let hwnd = setup
                    .handle
                    .parse::<usize>()
                    .map_err(|_| "ウィンドウが不正です")? as HWND;
                if IsIconic(hwnd) != 0 {
                    ShowWindow(hwnd, SW_RESTORE);
                }
                if IsZoomed(hwnd) == 0 && ShowWindow(hwnd, SW_MAXIMIZE) == 0 {
                    return Err("ウィンドウを最大化できませんでした".into());
                }
            }
        }
        list_windows(reader_handle)?
            .into_iter()
            .find(|w| w.handle == selected.handle && w.process_id == selected.process_id)
            .ok_or_else(|| "調整後のウィンドウが見つかりません".into())
    }

    pub fn focus_window(window: &CaptureWindow) -> Result<(), String> {
        unsafe {
            let hwnd = window
                .handle
                .parse::<usize>()
                .map_err(|_| "ウィンドウが不正です")? as HWND;
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid != window.process_id || SetForegroundWindow(hwnd) == 0 {
                return Err("対象を前面にできませんでした。対象ウィンドウをクリックしてから再試行してください".into());
            }
        }
        Ok(())
    }

    pub struct Target {
        handle: usize,
        process_id: u32,
        title: String,
        width: i32,
        height: i32,
    }
    impl Target {
        pub fn new(options: &CaptureOptions, reader_handle: Option<usize>) -> Result<Self, String> {
            let selected = list_windows(reader_handle)?
                .into_iter()
                .find(|w| w.handle == options.handle && w.process_id == options.process_id)
                .ok_or("対象ウィンドウが見つかりません。一覧を更新してください")?;
            if !options.auto_page {
                checked_region(options.region, selected.width, selected.height)?;
            }
            Ok(Self {
                handle: options
                    .handle
                    .parse()
                    .map_err(|_| "対象ウィンドウが不正です")?,
                process_id: options.process_id,
                title: selected.title,
                width: selected.width,
                height: selected.height,
            })
        }

        unsafe fn check(&self) -> Result<HWND, String> {
            let hwnd = self.handle as HWND;
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if IsWindow(hwnd) == 0
                || pid != self.process_id
                || IsIconic(hwnd) != 0
                || IsWindowVisible(hwnd) == 0
            {
                return Err("対象ウィンドウが閉じられたか、最小化されました".into());
            }
            if GetForegroundWindow() != hwnd {
                return Err("対象ウィンドウからフォーカスが外れました".into());
            }
            let mut text = [0u16; 1024];
            let length = GetWindowTextW(hwnd, text.as_mut_ptr(), text.len() as i32);
            if String::from_utf16_lossy(&text[..length.max(0) as usize]) != self.title {
                return Err(
                    "対象のタイトルが変わりました。本やタブが切り替わっていないか確認してください"
                        .into(),
                );
            }
            let mut rect: RECT = zeroed();
            if GetClientRect(hwnd, &mut rect) == 0
                || rect.right != self.width
                || rect.bottom != self.height
            {
                return Err("対象ウィンドウのサイズが変わりました".into());
            }
            // Escape also serves as an emergency stop while the reader has focus.
            if GetAsyncKeyState(VK_ESCAPE as i32) < 0 {
                return Err("Escキーで停止しました".into());
            }
            Ok(hwnd)
        }

        pub fn advance(&self, key: &str) -> Result<(), String> {
            unsafe {
                let _dpi = DpiGuard::new();
                self.check()?;
                if [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN]
                    .iter()
                    .any(|key| GetAsyncKeyState(*key as i32) < 0)
                {
                    return Err("修飾キーが押されているため、誤操作防止のため停止しました".into());
                }
                let vk = match key {
                    "left" => VK_LEFT,
                    "right" => VK_RIGHT,
                    "pagedown" => VK_NEXT,
                    "pageup" => VK_PRIOR,
                    _ => VK_SPACE,
                };
                let sequence = if key == "shift-space" {
                    vec![
                        (VK_SHIFT, 0),
                        (vk, 0),
                        (vk, KEYEVENTF_KEYUP),
                        (VK_SHIFT, KEYEVENTF_KEYUP),
                    ]
                } else {
                    vec![(vk, 0), (vk, KEYEVENTF_KEYUP)]
                };
                let inputs: Vec<INPUT> = sequence
                    .into_iter()
                    .map(|(vk, flags)| {
                        let mut input: INPUT = zeroed();
                        input.r#type = INPUT_KEYBOARD;
                        let extended = if [VK_LEFT, VK_RIGHT, VK_PRIOR, VK_NEXT].contains(&vk) {
                            KEYEVENTF_EXTENDEDKEY
                        } else {
                            0
                        };
                        input.Anonymous.ki = KEYBDINPUT {
                            wVk: vk,
                            wScan: MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) as u16,
                            dwFlags: flags | extended,
                            time: 0,
                            dwExtraInfo: 0,
                        };
                        input
                    })
                    .collect();
                if SendInput(
                    inputs.len() as u32,
                    inputs.as_ptr(),
                    size_of::<INPUT>() as i32,
                ) != inputs.len() as u32
                {
                    // Best effort release if only keydown was accepted.
                    for input in &inputs {
                        if input.Anonymous.ki.dwFlags & KEYEVENTF_KEYUP != 0 {
                            SendInput(1, input, size_of::<INPUT>() as i32);
                        }
                    }
                    return Err(
                        "ページ送りキーを送信できませんでした（対象の権限も確認してください）"
                            .into(),
                    );
                }
                Ok(())
            }
        }

        pub fn tap_center(&self) -> Result<(), String> {
            unsafe {
                let _dpi = DpiGuard::new();
                let hwnd = self.check()?;
                let mut point = POINT {
                    x: self.width / 2,
                    y: self.height / 2,
                };
                if ClientToScreen(hwnd, &mut point) == 0 {
                    return Err("本文中央の位置を取得できません".into());
                }
                let left = GetSystemMetrics(SM_XVIRTUALSCREEN);
                let top = GetSystemMetrics(SM_YVIRTUALSCREEN);
                let width = GetSystemMetrics(SM_CXVIRTUALSCREEN);
                let height = GetSystemMetrics(SM_CYVIRTUALSCREEN);
                if width <= 1
                    || height <= 1
                    || point.x < left
                    || point.y < top
                    || point.x >= left + width
                    || point.y >= top + height
                {
                    return Err("本文中央が画面外です".into());
                }
                let mut inputs: [INPUT; 3] = zeroed();
                for input in &mut inputs {
                    input.r#type = INPUT_MOUSE;
                }
                inputs[0].Anonymous.mi = MOUSEINPUT {
                    dx: ((point.x - left) as i64 * 65535 / (width - 1) as i64) as i32,
                    dy: ((point.y - top) as i64 * 65535 / (height - 1) as i64) as i32,
                    mouseData: 0,
                    dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                    time: 0,
                    dwExtraInfo: 0,
                };
                inputs[1].Anonymous.mi.dwFlags = MOUSEEVENTF_LEFTDOWN;
                inputs[2].Anonymous.mi.dwFlags = MOUSEEVENTF_LEFTUP;
                if SendInput(3, inputs.as_ptr(), size_of::<INPUT>() as i32) != 3 {
                    SendInput(1, &inputs[2], size_of::<INPUT>() as i32);
                    return Err("本文中央をクリックできませんでした".into());
                }
                Ok(())
            }
        }

        pub fn capture(&self, region: Region) -> Result<Frame, String> {
            unsafe {
                let _dpi = DpiGuard::new();
                let hwnd = self.check()?;
                let region = checked_region(region, self.width, self.height)?;
                let mut origin = POINT {
                    x: region.x,
                    y: region.y,
                };
                if ClientToScreen(hwnd, &mut origin) == 0 {
                    return Err("スクリーンショットの取得位置を確認できません".into());
                }
                let right = origin.x + region.width;
                let bottom = origin.y + region.height;
                if origin.x < GetSystemMetrics(SM_XVIRTUALSCREEN)
                    || origin.y < GetSystemMetrics(SM_YVIRTUALSCREEN)
                    || right
                        > GetSystemMetrics(SM_XVIRTUALSCREEN) + GetSystemMetrics(SM_CXVIRTUALSCREEN)
                    || bottom
                        > GetSystemMetrics(SM_YVIRTUALSCREEN) + GetSystemMetrics(SM_CYVIRTUALSCREEN)
                {
                    return Err("保存領域が画面の外にはみ出しています".into());
                }
                let screen = GetDC(null_mut());
                if screen.is_null() {
                    return Err("画面を取得できません".into());
                }
                let dc = CreateCompatibleDC(screen);
                let bitmap = CreateCompatibleBitmap(screen, region.width, region.height);
                if dc.is_null() || bitmap.is_null() {
                    if !bitmap.is_null() {
                        DeleteObject(bitmap);
                    }
                    if !dc.is_null() {
                        DeleteDC(dc);
                    }
                    ReleaseDC(null_mut(), screen);
                    return Err("スクリーンショット用メモリーを確保できません".into());
                }
                let old = SelectObject(dc, bitmap);
                let copied = BitBlt(
                    dc,
                    0,
                    0,
                    region.width,
                    region.height,
                    screen,
                    origin.x,
                    origin.y,
                    SRCCOPY | CAPTUREBLT,
                );
                SelectObject(dc, old);
                let mut info: BITMAPINFO = zeroed();
                info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
                info.bmiHeader.biWidth = region.width;
                info.bmiHeader.biHeight = -region.height;
                info.bmiHeader.biPlanes = 1;
                info.bmiHeader.biBitCount = 32;
                info.bmiHeader.biCompression = BI_RGB;
                let mut pixels = vec![0u8; region.width as usize * region.height as usize * 4];
                let lines = if copied != 0 {
                    GetDIBits(
                        dc,
                        bitmap,
                        0,
                        region.height as u32,
                        pixels.as_mut_ptr().cast(),
                        &mut info,
                        DIB_RGB_COLORS,
                    )
                } else {
                    0
                };
                DeleteObject(bitmap);
                DeleteDC(dc);
                ReleaseDC(null_mut(), screen);
                if lines != region.height {
                    return Err("スクリーンショットを取得できません".into());
                }
                self.check()?;
                let rgb = pixels
                    .chunks_exact(4)
                    .flat_map(|p| [p[2], p[1], p[0]])
                    .collect();
                Ok(Frame {
                    width: region.width as u32,
                    height: region.height as u32,
                    rgb,
                })
            }
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use super::*;
    pub struct KeepAwake;
    impl KeepAwake {
        pub fn new() -> Result<Self, String> {
            Err("バックアップ機能はWindows専用です".into())
        }
    }
    pub fn list_windows(_: Option<usize>) -> Result<Vec<CaptureWindow>, String> {
        Err("バックアップ機能はWindows専用です".into())
    }
    pub fn escape_pressed() -> bool {
        false
    }
    pub fn check_foreground(_: usize) -> Result<(), String> {
        Err("バックアップ機能はWindows専用です".into())
    }
    pub fn prepare_window(_: &CaptureSetup, _: Option<usize>) -> Result<CaptureWindow, String> {
        Err("バックアップ機能はWindows専用です".into())
    }
    pub fn focus_window(_: &CaptureWindow) -> Result<(), String> {
        Err("バックアップ機能はWindows専用です".into())
    }
    pub struct Target;
    impl Target {
        pub fn new(_: &CaptureOptions, _: Option<usize>) -> Result<Self, String> {
            Err("バックアップ機能はWindows専用です".into())
        }
        pub fn capture(&self, _: Region) -> Result<Frame, String> {
            unreachable!()
        }
        pub fn advance(&self, _: &str) -> Result<(), String> {
            unreachable!()
        }
        pub fn tap_center(&self) -> Result<(), String> {
            unreachable!()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn options() -> CaptureOptions {
        CaptureOptions {
            wait_timeout_ms: default_wait_timeout(),
            handle: "1".into(),
            process_id: 1,
            asin: "B012345678".into(),
            book_title: "Test book".into(),
            series_name: None,
            output_directory: String::new(),
            region: Region {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            key: "left".into(),
            max_pages: 1000,
            auto_page: false,
            portrait: false,
            from_beginning: false,
            rewind_only: false,
            confirmed_start: None,
        }
    }
    #[test]
    fn validates_capture_options() {
        let mut o = options();
        assert!(o.validate().is_ok());
        o.max_pages = 0;
        assert!(o.validate().is_err());
        o = options();
        o.wait_timeout_ms = 1;
        assert!(o.validate().is_err());
        o = options();
        o.key = "enter".into();
        assert!(o.validate().is_err());
        o = options();
        o.output_directory = "relative".into();
        assert!(o.validate().is_err());
        o = options();
        o.asin = "../invalid".into();
        assert!(o.validate().is_err());
    }
    #[test]
    fn validates_region_without_overflow() {
        assert!(checked_region(options().region, 100, 200).is_ok());
        assert!(checked_region(
            Region {
                x: 90,
                y: 0,
                width: 20,
                height: 10
            },
            100,
            200
        )
        .is_err());
        assert!(checked_region(
            Region {
                x: i32::MAX,
                y: 0,
                width: 20,
                height: 10
            },
            100,
            200
        )
        .is_err());
    }
    #[test]
    fn crops_only_requested_rectangle() {
        let frame = Frame {
            width: 4,
            height: 3,
            rgb: (0..36).collect(),
        };
        let cropped = frame
            .crop(Region {
                x: 1,
                y: 1,
                width: 2,
                height: 2,
            })
            .unwrap();
        assert_eq!((cropped.width, cropped.height), (2, 2));
        assert_eq!(
            cropped.rgb,
            [15, 16, 17, 18, 19, 20, 27, 28, 29, 30, 31, 32]
        );
        assert!(frame
            .crop(Region {
                x: 3,
                y: 2,
                width: 2,
                height: 2
            })
            .is_err());
    }
    #[test]
    fn saves_png_without_overwriting() {
        let path = std::env::temp_dir().join(format!("mmk-capture-{}.png", uuid::Uuid::new_v4()));
        let frame = Frame {
            width: 1,
            height: 1,
            rgb: vec![12, 34, 56],
        };
        frame.save(&path).unwrap();
        assert!(frame.save(&path).is_err());
        let decoder = png::Decoder::new(std::fs::File::open(&path).unwrap());
        let mut reader = decoder.read_info().unwrap();
        let mut bytes = vec![0; reader.output_buffer_size()];
        reader.next_frame(&mut bytes).unwrap();
        assert_eq!(bytes, frame.rgb);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn cancellation_interrupts_wait() {
        let manager = CaptureManager::default();
        manager.stop();
        assert!(wait(&manager, Duration::from_secs(30)).is_err());
    }
    fn test_directory() -> PathBuf {
        let path = std::env::temp_dir().join(format!("mmk-capture-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn accepts_toc_start_after_rendering_without_probe_inputs_or_repeated_reads() {
        use std::cell::Cell;
        for (first, end) in [(1, 1), (2, 2), (2, 3), (7, 7)] {
            let ready = Cell::new(false);
            let reads = Cell::new(0);
            let start = confirm_reader_start(
                &CaptureManager::default(),
                true,
                || {
                    ready.set(true);
                    Ok(())
                },
                || {
                    assert!(
                        ready.get(),
                        "must wait for rendering before reading the start"
                    );
                    reads.set(reads.get() + 1);
                    Ok(ReadingPosition {
                        start: first,
                        end,
                        total: 20,
                    })
                },
            )
            .unwrap();
            assert_eq!(start.start, first);
            assert_eq!(start.end, end);
            assert_eq!(reads.get(), 1);
        }
    }

    #[test]
    fn startup_waits_for_rendering_before_hiding_toolbar_and_reading_position() {
        use std::cell::{Cell, RefCell};
        let rendering = Cell::new(true);
        let steps = RefCell::new(Vec::new());
        let expected = ReadingPosition {
            start: 2,
            end: 2,
            total: 170,
        };
        let start = confirm_reader_start(
            &CaptureManager::default(),
            true,
            || {
                steps.borrow_mut().push("render_ready");
                rendering.set(false);
                Ok(())
            },
            || {
                assert!(
                    !rendering.get(),
                    "toolbar clicks during TOC navigation may be ignored"
                );
                steps.borrow_mut().push("hide_toolbar");
                steps.borrow_mut().push("read_position");
                Ok(expected)
            },
        )
        .unwrap();
        assert_eq!(start, expected);
        assert_eq!(
            *steps.borrow(),
            ["render_ready", "hide_toolbar", "read_position"]
        );
    }

    #[test]
    fn rejects_start_when_toc_selection_or_rendering_fails() {
        let manager = CaptureManager::default();
        assert!(confirm_reader_start(
            &manager,
            false,
            || panic!("TOC selection failed"),
            || panic!("must not accept an arbitrary position"),
        )
        .is_err());
        assert!(confirm_reader_start(
            &manager,
            true,
            || Err("render timeout".into()),
            || panic!("must not read before rendering"),
        )
        .is_err());
    }

    #[test]
    fn start_confirmation_rejects_missing_or_invalid_positions_and_cancellation() {
        let manager = CaptureManager::default();
        assert!(
            confirm_reader_start(&manager, true, || Ok(()), || Err("missing position".into()),)
                .is_err()
        );
        for invalid in [
            ReadingPosition {
                start: 0,
                end: 0,
                total: 8,
            },
            ReadingPosition {
                start: 2,
                end: 1,
                total: 8,
            },
            ReadingPosition {
                start: 2,
                end: 9,
                total: 8,
            },
        ] {
            assert!(confirm_reader_start(&manager, true, || Ok(()), || Ok(invalid),).is_err());
        }
        manager.stop();
        assert!(confirm_reader_start(
            &manager,
            true,
            || panic!("cancelled before rendering"),
            || panic!("cancelled before reading"),
        )
        .is_err());
    }

    #[test]
    fn cancellation_during_start_rendering_prevents_position_read() {
        let manager = CaptureManager::default();
        assert!(confirm_reader_start(
            &manager,
            true,
            || {
                manager.stop();
                Ok(())
            },
            || panic!("cancelled during rendering"),
        )
        .is_err());
    }

    #[test]
    fn saves_from_confirmed_non_one_start_in_single_and_spread_modes() {
        use std::cell::Cell;
        for spread in [false, true] {
            let directory = test_directory();
            let manager = CaptureManager::default();
            let first = ReadingPosition {
                start: 2,
                end: if spread { 3 } else { 2 },
                total: 5,
            };
            let first = confirm_reader_start(&manager, true, || Ok(()), || Ok(first)).unwrap();
            let mut options = options();
            options.confirmed_start = Some(first);
            let mut state = AutoPageState {
                confirmed_start: Some(first),
                ..Default::default()
            };
            let position = Cell::new(first);
            let token = Cell::new(1);
            let result = run_pages(
                &manager,
                &options,
                &directory,
                || {
                    capture_when_ready(
                        &manager,
                        &mut state,
                        Duration::ZERO,
                        || Ok(rendered(token.get(), true)),
                        || Ok(position.get()),
                        || {
                            Ok(Frame {
                                width: 1,
                                height: 1,
                                rgb: vec![0; 3],
                            })
                        },
                        || Ok(navigation(false, None, None)),
                    )
                },
                || {
                    let previous = position.get();
                    let start = previous.end + 1;
                    position.set(ReadingPosition {
                        start,
                        end: if spread { (start + 1).min(5) } else { start },
                        total: 5,
                    });
                    token.set(token.get() + 1);
                    Ok(())
                },
            )
            .unwrap();
            assert_eq!(result, CaptureEnd::LastPage);
            assert_eq!(
                manager.status().unwrap().saved_pages,
                if spread { 2 } else { 4 }
            );
            assert!(directory.join("screenshot_0001.png").exists());
            clean_test_directory(&directory);
        }
    }

    #[test]
    fn first_capture_must_match_confirmed_start_including_range_and_total() {
        let first = ReadingPosition {
            start: 2,
            end: 2,
            total: 5,
        };
        for actual in [
            ReadingPosition {
                start: 3,
                end: 3,
                total: 5,
            },
            ReadingPosition {
                start: 2,
                end: 3,
                total: 5,
            },
            ReadingPosition {
                start: 2,
                end: 2,
                total: 6,
            },
        ] {
            let mut state = AutoPageState {
                confirmed_start: Some(first),
                ..Default::default()
            };
            assert!(capture_when_ready(
                &CaptureManager::default(),
                &mut state,
                Duration::ZERO,
                || Ok(rendered(1, true)),
                || Ok(actual),
                || panic!("mismatch must not capture"),
                || panic!("must not observe"),
            )
            .is_err());
            let directory = test_directory();
            let mut options = options();
            options.confirmed_start = Some(first);
            assert!(run_pages(
                &CaptureManager::default(),
                &options,
                &directory,
                || Ok((
                    Frame {
                        width: 1,
                        height: 1,
                        rgb: vec![0; 3]
                    },
                    actual
                )),
                || panic!("must not turn"),
            )
            .is_err());
            assert_eq!(fs::read_dir(&directory).unwrap().count(), 0);
            clean_test_directory(&directory);
        }
    }

    fn clean_test_directory(path: &Path) {
        for file in fs::read_dir(path).unwrap() {
            let file = file.unwrap();
            if file.file_type().unwrap().is_dir() {
                clean_test_directory(&file.path());
            } else {
                fs::remove_file(file.path()).unwrap();
            }
        }
        fs::remove_dir(path).unwrap();
    }

    fn batch_books() -> Vec<BatchBook> {
        ["B000000001", "B000000002", "B000000003"]
            .into_iter()
            .map(|asin| BatchBook {
                asin: asin.into(),
                title: asin.into(),
                series_name: None,
            })
            .collect()
    }

    #[test]
    fn capture_data_requires_nonempty_image_under_the_selected_directory() {
        let root = test_directory();
        let book = root.join("B000000001");
        assert!(!has_capture_data(&book, || Ok(())).unwrap());
        let run = book.join("prior-run");
        fs::create_dir_all(&run).unwrap();
        fs::write(run.join("notes.txt"), b"not a screenshot").unwrap();
        fs::write(run.join("screenshot_0001.png"), b"").unwrap();
        assert!(!has_capture_data(&book, || Ok(())).unwrap());
        fs::write(run.join("screenshot_0001.PNG"), b"image data").unwrap();
        assert!(has_capture_data(&book, || Ok(())).unwrap());
        assert!(!has_capture_data(&root.join("B000000002"), || Ok(())).unwrap());
        assert_eq!(
            has_capture_data(&book, || Err("cancelled".into())).unwrap_err(),
            "cancelled"
        );
        clean_test_directory(&root);
    }

    #[test]
    fn capture_data_errors_are_not_treated_as_uncaptured() {
        let root = test_directory();
        let path = root.join("not-a-directory");
        fs::write(&path, b"file").unwrap();
        assert!(has_capture_data(&path, || Ok(())).is_err());
        clean_test_directory(&root);
    }

    #[test]
    fn batch_keeps_order_ignores_legacy_images_and_skips_only_completed_cbz() {
        let root = test_directory();
        let mut books = batch_books();
        books.reverse();
        let existing = root.join(&books[1].asin).join("previous-run");
        fs::create_dir_all(&existing).unwrap();
        fs::write(existing.join("screenshot_0001.png"), b"existing").unwrap();
        let manager = CaptureManager::default();
        let mut opened = Vec::new();
        run_batch(&manager, &books, &root, |book, directory| {
            opened.push(book.asin.clone());
            assert!(directory.starts_with(root.join(".mmk-backup").join(&book.asin)));
            fs::write(directory.join("screenshot_0001.png"), b"new image").unwrap();
            manager.status.lock().unwrap().saved_pages = 1;
            Ok(CaptureEnd::LastPage)
        })
        .unwrap();
        assert_eq!(opened, ["B000000003", "B000000002", "B000000001"]);
        assert_eq!(manager.status().unwrap().completed_books, 3);
        assert_eq!(manager.status().unwrap().skipped_books, 0);
        assert_eq!(
            fs::read(existing.join("screenshot_0001.png")).unwrap(),
            b"existing"
        );
        // All three books are now skipped; no reader needs to be opened.
        run_batch(&CaptureManager::default(), &books, &root, |_, _| {
            panic!("must skip")
        })
        .unwrap();
        clean_test_directory(&root);
    }

    #[test]
    fn batch_stops_on_limit_or_error_without_opening_next_book() {
        for end in [Ok(CaptureEnd::Limit), Err("capture failed".into())] {
            let root = test_directory();
            let manager = CaptureManager::default();
            let mut calls = 0;
            let mut end = Some(end);
            let result = run_batch(&manager, &batch_books(), &root, |_, directory| {
                calls += 1;
                fs::write(directory.join("screenshot_0001.png"), b"saved page").unwrap();
                end.take().unwrap()
            });
            assert!(result.is_err());
            assert_eq!(calls, 1);
            assert_eq!(manager.status().unwrap().completed_books, 0);
            assert!(
                has_capture_data(&root.join(".mmk-backup").join("B000000001"), || Ok(())).unwrap()
            );
            assert!(!root.join("シリーズなし").join("B000000002").exists());
            clean_test_directory(&root);
        }
    }

    #[test]
    fn batch_groups_series_and_does_not_skip_a_different_book_with_the_same_title() {
        let root = test_directory();
        let mut books = batch_books();
        for book in &mut books[..2] {
            book.title = "同名の書籍".into();
            book.series_name = Some("シリーズA".into());
        }
        books[2].title = "単巻の書籍".into();
        let manager = CaptureManager::default();
        let mut directories = Vec::new();
        run_batch(&manager, &books, &root, |_, directory| {
            directories.push(directory.to_path_buf());
            fs::write(directory.join("screenshot_0001.png"), b"captured page").unwrap();
            manager.status.lock().unwrap().saved_pages = 1;
            Ok(CaptureEnd::LastPage)
        })
        .unwrap();
        assert_eq!(directories.len(), 3);
        assert!(root.join("シリーズA").join("同名の書籍.cbz").exists());
        assert!(root
            .join("シリーズA")
            .join("同名の書籍 [B000000002].cbz")
            .exists());
        assert!(root.join("シリーズなし").join("単巻の書籍.cbz").exists());
        assert_eq!(manager.status().unwrap().completed_books, 3);
        let manager = CaptureManager::default();
        run_batch(&manager, &books, &root, |_, _| panic!("all captured")).unwrap();
        assert_eq!(manager.status().unwrap().skipped_books, 3);
        clean_test_directory(&root);
    }

    #[test]
    fn batch_cancellation_prevents_opening_any_more_books() {
        let root = test_directory();
        let manager = CaptureManager::default();
        manager.stop();
        assert!(run_batch(&manager, &batch_books(), &root, |_, _| panic!("cancelled")).is_err());
        let manager = CaptureManager::default();
        let mut calls = 0;
        assert!(run_batch(&manager, &batch_books(), &root, |_, _| {
            calls += 1;
            manager.stop();
            Ok(CaptureEnd::LastPage)
        })
        .is_err());
        assert_eq!(calls, 1);
        assert!(!root.join("シリーズなし").join("B000000002").exists());
        clean_test_directory(&root);
    }

    #[test]
    fn capture_loop_saves_identical_art_at_distinct_positions_and_stops_at_total() {
        let directory = test_directory();
        let manager = CaptureManager::default();
        let options = options();
        let mut positions = [1, 2, 3].into_iter();
        let mut advances = 0;
        let result = run_pages(
            &manager,
            &options,
            &directory,
            || {
                let start = positions.next().unwrap();
                Ok((
                    Frame {
                        width: 2,
                        height: 2,
                        rgb: vec![255; 12],
                    },
                    ReadingPosition {
                        start,
                        end: start,
                        total: 3,
                    },
                ))
            },
            || {
                advances += 1;
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(result, CaptureEnd::LastPage);
        assert_eq!(manager.status().unwrap().saved_pages, 3);
        assert_eq!(advances, 2);
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 3);
        assert!(directory.join("screenshot_0001.png").exists());
        assert!(directory.join("screenshot_0002.png").exists());
        clean_test_directory(&directory);
    }

    #[test]
    fn capture_loop_page_limit_does_not_advance_past_last_capture() {
        let directory = test_directory();
        let manager = CaptureManager::default();
        let mut options = options();
        options.max_pages = 1;
        let result = run_pages(
            &manager,
            &options,
            &directory,
            || {
                Ok((
                    Frame {
                        width: 1,
                        height: 1,
                        rgb: vec![0; 3],
                    },
                    ReadingPosition {
                        start: 1,
                        end: 1,
                        total: 2,
                    },
                ))
            },
            || panic!("must not send input after reaching limit"),
        )
        .unwrap();
        assert_eq!(result, CaptureEnd::Limit);
        assert_eq!(manager.status().unwrap().saved_pages, 1);
        clean_test_directory(&directory);
    }

    #[test]
    fn backup_uses_fresh_numbering_and_does_not_touch_legacy_images() {
        let directory = test_directory();
        fs::write(directory.join("screenshot_0007.png"), b"original image").unwrap();
        let manager = CaptureManager::default();
        let mut options = options();
        options.max_pages = 1;
        assert_eq!(
            run_pages(
                &manager,
                &options,
                &directory,
                || Ok((
                    Frame {
                        width: 1,
                        height: 1,
                        rgb: vec![0; 3]
                    },
                    ReadingPosition {
                        start: 1,
                        end: 1,
                        total: 2
                    }
                )),
                || panic!("must not advance after limit"),
            )
            .unwrap(),
            CaptureEnd::Limit
        );
        assert_eq!(
            fs::read(directory.join("screenshot_0007.png")).unwrap(),
            b"original image"
        );
        assert!(directory.join("screenshot_0001.png").exists());
        assert_eq!(manager.status().unwrap().saved_pages, 1);
        clean_test_directory(&directory);
    }

    #[test]
    fn capture_loop_stops_before_input_when_cancelled() {
        let directory = test_directory();
        let manager = CaptureManager::default();
        let result = run_pages(
            &manager,
            &options(),
            &directory,
            || {
                manager.stop();
                Ok((
                    Frame {
                        width: 1,
                        height: 1,
                        rgb: vec![0; 3],
                    },
                    ReadingPosition {
                        start: 1,
                        end: 1,
                        total: 2,
                    },
                ))
            },
            || panic!("must not send input after cancellation"),
        );
        assert!(result.unwrap_err().contains("手動"));
        clean_test_directory(&directory);
    }

    #[test]
    fn stuck_position_is_an_error_and_does_not_send_extra_turns() {
        let directory = test_directory();
        let manager = CaptureManager::default();
        let options = options();
        let mut advances = 0;
        let result = run_pages(
            &manager,
            &options,
            &directory,
            || {
                Ok((
                    Frame {
                        width: 1,
                        height: 1,
                        rgb: vec![0; 3],
                    },
                    ReadingPosition {
                        start: 1,
                        end: 1,
                        total: 2,
                    },
                ))
            },
            || {
                advances += 1;
                Ok(())
            },
        );
        assert!(result.is_err());
        assert_eq!(advances, 1);
        assert_eq!(manager.status().unwrap().saved_pages, 1);
        clean_test_directory(&directory);
    }

    #[test]
    fn rejects_missing_positions_non_first_start_jumps_regression_and_total_changes() {
        for sequence in [
            vec![(2, 3)],
            vec![(0, 3)],
            vec![(1, 3), (3, 3)],
            vec![(1, 3), (2, 3), (1, 3)],
            vec![(1, 3), (2, 4)],
        ] {
            let directory = test_directory();
            let options = options();
            let mut sequence = sequence.into_iter();
            let result = run_pages(
                &CaptureManager::default(),
                &options,
                &directory,
                || {
                    let (position, total) = sequence.next().unwrap();
                    Ok((
                        Frame {
                            width: 1,
                            height: 1,
                            rgb: vec![0; 3],
                        },
                        ReadingPosition {
                            start: position,
                            end: position,
                            total,
                        },
                    ))
                },
                || Ok(()),
            );
            assert!(result.is_err());
            clean_test_directory(&directory);
        }
        let directory = test_directory();
        assert!(run_pages(
            &CaptureManager::default(),
            &options(),
            &directory,
            || Err("missing position".into()),
            || panic!("no input")
        )
        .is_err());
        clean_test_directory(&directory);
    }

    #[test]
    fn delayed_turn_does_not_duplicate_pages_or_skip_by_sending_another_key() {
        let directory = test_directory();
        let manager = CaptureManager::default();
        let options = options();
        let mut positions = [1, 1, 1, 2].into_iter();
        let mut advances = 0;
        let result = run_pages(
            &manager,
            &options,
            &directory,
            || {
                let start = positions.next().unwrap();
                Ok((
                    Frame {
                        width: 1,
                        height: 1,
                        rgb: vec![0; 3],
                    },
                    ReadingPosition {
                        start,
                        end: start,
                        total: 2,
                    },
                ))
            },
            || {
                advances += 1;
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(result, CaptureEnd::LastPage);
        assert_eq!(advances, 1);
        assert_eq!(manager.status().unwrap().saved_pages, 2);
        clean_test_directory(&directory);
    }

    #[test]
    fn spread_screens_step_contiguously_and_finish_at_the_last_end() {
        let directory = test_directory();
        let manager = CaptureManager::default();
        let options = options();
        let mut screens = [(1, 2), (3, 4), (5, 5)].into_iter();
        let result = run_pages(
            &manager,
            &options,
            &directory,
            || {
                let (start, end) = screens.next().unwrap();
                Ok((
                    Frame {
                        width: 1,
                        height: 1,
                        rgb: vec![0; 3],
                    },
                    ReadingPosition {
                        start,
                        end,
                        total: 5,
                    },
                ))
            },
            || Ok(()),
        )
        .unwrap();
        assert_eq!(result, CaptureEnd::LastPage);
        assert_eq!(manager.status().unwrap().saved_pages, 3);
        clean_test_directory(&directory);
    }

    #[test]
    fn a_gap_between_spread_screens_stops_the_run() {
        for screens in [vec![(1, 2), (4, 4)], vec![(3, 4), (3, 4), (2, 2)]] {
            let directory = test_directory();
            let mut screens = screens.into_iter();
            let result = run_pages(
                &CaptureManager::default(),
                &options(),
                &directory,
                || {
                    let (start, end) = screens.next().unwrap();
                    Ok((
                        Frame {
                            width: 1,
                            height: 1,
                            rgb: vec![0; 3],
                        },
                        ReadingPosition {
                            start,
                            end,
                            total: 4,
                        },
                    ))
                },
                || Ok(()),
            );
            assert!(result.is_err());
            clean_test_directory(&directory);
        }
    }

    fn navigation(
        chrome_visible: bool,
        position: Option<u32>,
        total: Option<u32>,
    ) -> capture_layout::ReaderNavigation {
        capture_layout::ReaderNavigation {
            known: true,
            chrome_visible,
            toolbar: None,
            toc_open: false,
            toc_close_visible: false,
            position,
            position_end: None,
            total,
            position_texts: Vec::new(),
        }
    }

    fn rendered(token: u64, ready: bool) -> capture_layout::PageRender {
        capture_layout::PageRender {
            ready,
            token,
            error: None,
            ..Default::default()
        }
    }

    #[test]
    fn operation_log_distinguishes_accepted_input_from_actual_page_movement() {
        use std::cell::Cell;
        for moves in [true, false] {
            let directory = test_directory();
            let logs = directory.join("logs");
            let manager = CaptureManager::default();
            manager.configure_logging(logs);
            manager.begin_trace(json!({"mode":"test"}));
            let current = Cell::new(1);
            let mut state = AutoPageState::default();
            let result = run_pages(
                &manager,
                &options(),
                &directory,
                || {
                    capture_when_ready(
                        &manager,
                        &mut state,
                        Duration::ZERO,
                        || Ok(rendered(current.get() as u64, true)),
                        || {
                            Ok(ReadingPosition {
                                start: current.get(),
                                end: current.get(),
                                total: 2,
                            })
                        },
                        || {
                            Ok(Frame {
                                width: 1,
                                height: 1,
                                rgb: vec![0; 3],
                            })
                        },
                        || Ok(navigation(false, None, None)),
                    )
                },
                || {
                    trace_key_input(&manager, "left", "forward", || {
                        if moves {
                            current.set(2);
                        }
                        Ok(())
                    })
                },
            );
            if let Err(error) = &result {
                manager.trace_failure("test_run", error);
            }
            assert_eq!(result.is_ok(), moves);
            let status = manager.status().unwrap();
            assert!(status.log_error.is_none());
            let text = fs::read_to_string(status.log_path.unwrap()).unwrap();
            let records: Vec<serde_json::Value> = text
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            let event_index = |name| records.iter().position(|r| r["event"] == name).unwrap();
            assert!(event_index("page_saved") < event_index("key_send_requested"));
            assert!(event_index("key_send_requested") < event_index("key_send_accepted"));
            if moves {
                assert!(event_index("last_position_saved") > event_index("key_send_accepted"));
            } else {
                let timeout = records
                    .iter()
                    .find(|r| r["event"] == "render_wait_timeout")
                    .unwrap();
                assert_eq!(timeout["fields"]["waitingForUpdate"], true);
                assert!(!records.iter().any(|r| r["event"] == "last_position_saved"));
            }
            assert!(!text.contains("Test book"));
            assert!(!text.contains("B012345678"));
            drop(manager);
            clean_test_directory(&directory);
        }
    }

    #[test]
    fn failed_input_and_failed_log_storage_do_not_masquerade_as_accepted_input() {
        let directory = test_directory();
        let manager = CaptureManager::default();
        manager.configure_logging(directory.clone());
        manager.begin_trace(json!({}));
        assert!(trace_key_input(&manager, "left", "forward", || Err(
            "ページ送りキーを送信できません".into()
        ))
        .is_err());
        let text = fs::read_to_string(manager.status().unwrap().log_path.unwrap()).unwrap();
        assert!(text.contains("key_send_requested"));
        assert!(!text.contains("key_send_accepted"));
        assert!(text.contains("input_failed"));
        drop(manager);
        clean_test_directory(&directory);

        let directory = test_directory();
        let blocked = directory.join("blocked");
        fs::write(&blocked, "occupied").unwrap();
        let manager = CaptureManager::default();
        manager.configure_logging(blocked);
        manager.begin_trace(json!({}));
        let mut calls = 0;
        trace_key_input(&manager, "left", "forward", || {
            calls += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(calls, 1);
        assert!(manager.status().unwrap().log_error.is_some());
        drop(manager);
        clean_test_directory(&directory);
    }

    #[test]
    fn automatic_capture_saves_before_turning() {
        use std::cell::Cell;
        let directory = test_directory();
        let manager = CaptureManager::default();
        let options = options();
        let position = Cell::new(1);
        let mut state = AutoPageState::default();
        let end = run_pages(
            &manager,
            &options,
            &directory,
            || {
                capture_when_ready(
                    &manager,
                    &mut state,
                    Duration::from_secs(1),
                    || Ok(rendered(u64::from(position.get()), true)),
                    || {
                        Ok(ReadingPosition {
                            start: position.get(),
                            end: position.get(),
                            total: 3,
                        })
                    },
                    || {
                        Ok(Frame {
                            width: 1,
                            height: 1,
                            rgb: vec![0; 3],
                        })
                    },
                    || Ok(navigation(false, None, None)),
                )
            },
            || {
                assert!(directory
                    .join(format!("screenshot_{:04}.png", position.get()))
                    .exists());
                position.set(position.get() + 1);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(end, CaptureEnd::LastPage);
        assert_eq!(manager.status().unwrap().saved_pages, 3);
        clean_test_directory(&directory);
    }

    #[test]
    fn automatic_capture_does_not_save_the_previous_or_undecoded_page() {
        for render in [rendered(1, true), rendered(2, false)] {
            let mut state = AutoPageState {
                token: Some(1),
                position: Some(ReadingPosition {
                    start: 1,
                    end: 1,
                    total: 2,
                }),
                ..Default::default()
            };
            let result = capture_when_ready(
                &CaptureManager::default(),
                &mut state,
                Duration::ZERO,
                || Ok(render),
                || panic!("must not read the old page position"),
                || panic!("must not capture the old or undecoded page"),
                || panic!("must not inspect after capture"),
            );
            assert!(result.is_err());
            assert_eq!(state.token, Some(1));
        }
    }

    #[test]
    fn temporary_canvas_during_navigation_waits_for_the_image_renderer() {
        for ready in [false, true] {
            let mut polls = 0;
            let render = wait_for_render(
                &CaptureManager::default(),
                None,
                Instant::now() + Duration::from_secs(1),
                &mut || {
                    polls += 1;
                    if polls == 1 {
                        Ok(capture_layout::PageRender {
                            ready,
                            token: 1,
                            error: Some(capture_layout::PageRenderError::CanvasNotSupported),
                            ..Default::default()
                        })
                    } else {
                        Ok(rendered(2, true))
                    }
                },
            )
            .unwrap();
            assert_eq!(polls, 2);
            assert_eq!(render.token, 2);
            assert!(render.error.is_none());
        }
    }

    #[test]
    fn persistent_canvas_still_stops_at_the_existing_deadline() {
        let result = wait_for_render(
            &CaptureManager::default(),
            None,
            Instant::now(),
            &mut || {
                Ok(capture_layout::PageRender {
                    ready: false,
                    token: 1,
                    error: Some(capture_layout::PageRenderError::CanvasNotSupported),
                    ..Default::default()
                })
            },
        );
        assert!(result.unwrap_err().contains("この描画形式"));
    }

    #[test]
    fn automatic_capture_waits_for_the_update_and_does_not_turn_again() {
        let manager = CaptureManager::default();
        let mut calls = 0;
        let mut state = AutoPageState {
            token: Some(1),
            position: Some(ReadingPosition {
                start: 1,
                end: 1,
                total: 2,
            }),
            ..Default::default()
        };
        let result = capture_when_ready(
            &manager,
            &mut state,
            Duration::from_secs(1),
            || {
                calls += 1;
                Ok(if calls == 1 {
                    rendered(1, true)
                } else {
                    rendered(2, true)
                })
            },
            || {
                Ok(ReadingPosition {
                    start: 2,
                    end: 2,
                    total: 2,
                })
            },
            || {
                Ok(Frame {
                    width: 1,
                    height: 1,
                    rgb: vec![0; 3],
                })
            },
            || Ok(navigation(false, None, None)),
        )
        .unwrap();
        assert_eq!(result.1.start, 2);
        assert_eq!(state.token, Some(2));
    }

    #[test]
    fn automatic_capture_discards_a_frame_if_the_renderer_updates_during_capture() {
        use std::cell::Cell;
        let token = Cell::new(1);
        let captures = Cell::new(0);
        let mut state = AutoPageState::default();
        let result = capture_when_ready(
            &CaptureManager::default(),
            &mut state,
            Duration::from_secs(1),
            || Ok(rendered(token.get(), true)),
            || {
                Ok(ReadingPosition {
                    start: 1,
                    end: 1,
                    total: 2,
                })
            },
            || {
                captures.set(captures.get() + 1);
                token.set(2);
                Ok(Frame {
                    width: 1,
                    height: 1,
                    rgb: vec![captures.get(); 3],
                })
            },
            || Ok(navigation(false, None, None)),
        )
        .unwrap();
        assert_eq!(captures.get(), 2);
        assert_eq!(result.0.rgb, vec![2; 3]);
        assert_eq!(state.token, Some(2));
    }

    #[test]
    fn automatic_capture_rejects_position_gaps_and_unsupported_renderers() {
        let mut state = AutoPageState {
            token: Some(1),
            position: Some(ReadingPosition {
                start: 1,
                end: 1,
                total: 3,
            }),
            ..Default::default()
        };
        assert!(capture_when_ready(
            &CaptureManager::default(),
            &mut state,
            Duration::ZERO,
            || Ok(rendered(2, true)),
            || Ok(ReadingPosition {
                start: 3,
                end: 3,
                total: 3
            }),
            || panic!("must not capture a skipped page"),
            || panic!("no capture")
        )
        .is_err());
        let manager = CaptureManager::default();
        let error = wait_for_render(&manager, None, Instant::now(), &mut || {
            Ok(capture_layout::PageRender {
                ready: false,
                token: 1,
                error: Some(capture_layout::PageRenderError::CanvasNotSupported),
                ..Default::default()
            })
        })
        .err()
        .unwrap();
        assert!(error.contains("この描画形式"));
        manager.stop();
        assert!(
            wait_for_render(&manager, None, Instant::now(), &mut || panic!("cancelled")).is_err()
        );
    }

    #[test]
    fn positioned_capture_opens_toolbar_once_per_page_and_detects_the_last_page() {
        use std::cell::Cell;
        let directory = test_directory();
        let manager = CaptureManager::default();
        let options = options();
        let current = Cell::new(1);
        let expanded = Cell::new(false);
        let toggles = Cell::new(0);
        let position_reads = Cell::new(0);
        let result = run_pages(
            &manager,
            &options,
            &directory,
            || {
                capture_with_position(
                    || {
                        position_reads.set(position_reads.get() + 1);
                        read_position_with_toolbar(
                            &manager,
                            || {
                                Ok(if expanded.get() {
                                    navigation(true, Some(current.get()), Some(2))
                                } else {
                                    navigation(false, None, None)
                                })
                            },
                            || {
                                toggles.set(toggles.get() + 1);
                                expanded.set(!expanded.get());
                                Ok(())
                            },
                            Duration::ZERO,
                        )
                    },
                    || {
                        assert!(!expanded.get(), "the toolbar must be hidden during capture");
                        Ok(Frame {
                            width: 1,
                            height: 1,
                            rgb: vec![0; 3],
                        })
                    },
                    || Ok(navigation(false, None, None)),
                )
            },
            || {
                assert!(
                    directory
                        .join(format!("screenshot_{:04}.png", current.get()))
                        .exists(),
                    "save before sending the next turn"
                );
                current.set(current.get() + 1);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(result, CaptureEnd::LastPage);
        assert_eq!(manager.status().unwrap().saved_pages, 2);
        assert_eq!(position_reads.get(), 2);
        assert_eq!(toggles.get(), 4, "one show/hide cycle per page, not two");
        assert!(!expanded.get());
        clean_test_directory(&directory);
    }

    #[test]
    fn positioned_capture_checks_any_passively_available_position() {
        for (position, total) in [
            (None, None),
            (Some(1), None),
            (None, Some(2)),
            (Some(1), Some(2)),
        ] {
            let result = capture_with_position(
                || {
                    Ok(ReadingPosition {
                        start: 1,
                        end: 1,
                        total: 2,
                    })
                },
                || {
                    Ok(Frame {
                        width: 1,
                        height: 1,
                        rgb: vec![0; 3],
                    })
                },
                || Ok(navigation(false, position, total)),
            )
            .unwrap();
            assert_eq!(
                result.1,
                ReadingPosition {
                    start: 1,
                    end: 1,
                    total: 2
                }
            );
        }
    }

    #[test]
    fn positioned_capture_rejects_changed_position_and_unexpected_ui() {
        for mode in 0..5 {
            let result = capture_with_position(
                || {
                    Ok(ReadingPosition {
                        start: 1,
                        end: 1,
                        total: 2,
                    })
                },
                || {
                    Ok(Frame {
                        width: 1,
                        height: 1,
                        rgb: vec![0; 3],
                    })
                },
                || {
                    let mut ui = navigation(false, None, None);
                    match mode {
                        0 => ui.position = Some(2),
                        1 => ui.total = Some(3),
                        2 => ui.chrome_visible = true,
                        3 => ui.toc_open = true,
                        _ => ui.known = false,
                    }
                    Ok(ui)
                },
            );
            assert!(result.is_err());
        }
    }

    #[test]
    fn positioned_capture_never_captures_without_confirming_the_position() {
        let result = capture_with_position(
            || Err("missing position".into()),
            || panic!("must confirm the position before taking the screenshot"),
            || panic!("must not inspect after a failed position read"),
        );
        assert!(matches!(result, Err(error) if error == "missing position"));
    }

    #[test]
    fn reads_available_position_without_toggling_the_toolbar() {
        let result = read_position_with_toolbar(
            &CaptureManager::default(),
            || Ok(navigation(false, Some(1), Some(196))),
            || panic!("must not toggle a readable position"),
            Duration::ZERO,
        )
        .unwrap();
        assert_eq!(
            result,
            ReadingPosition {
                start: 1,
                end: 1,
                total: 196
            }
        );
    }

    #[test]
    fn reveals_missing_position_waits_for_ui_and_hides_toolbar_before_returning() {
        use std::cell::Cell;
        let expanded = Cell::new(false);
        let toggles = Cell::new(0);
        let visible_reads = Cell::new(0);
        let result = read_position_with_toolbar(
            &CaptureManager::default(),
            || {
                if expanded.get() {
                    visible_reads.set(visible_reads.get() + 1);
                    if visible_reads.get() >= 3 {
                        return Ok(navigation(true, Some(1), Some(196)));
                    }
                }
                Ok(navigation(expanded.get(), None, None))
            },
            || {
                toggles.set(toggles.get() + 1);
                expanded.set(!expanded.get());
                Ok(())
            },
            Duration::ZERO,
        )
        .unwrap();
        assert_eq!(
            result,
            ReadingPosition {
                start: 1,
                end: 1,
                total: 196
            }
        );
        assert_eq!(toggles.get(), 2);
        assert!(!expanded.get(), "screenshots must not include the toolbar");
    }

    #[test]
    fn missing_total_stays_an_error_with_diagnostics_and_toolbar_is_restored() {
        use std::cell::Cell;
        let expanded = Cell::new(false);
        let result = read_position_with_toolbar(
            &CaptureManager::default(),
            || {
                let mut ui = navigation(expanded.get(), Some(1), None);
                if expanded.get() {
                    ui.position_texts.push("位置1".into());
                }
                Ok(ui)
            },
            || {
                expanded.set(!expanded.get());
                Ok(())
            },
            Duration::ZERO,
        );
        let error = result.unwrap_err();
        assert!(error.contains("総数: 取得不可"));
        assert!(error.contains("位置表示: 位置1"));
        assert!(!expanded.get());
    }

    #[test]
    fn failure_to_hide_toolbar_prevents_returning_a_capture_position() {
        use std::cell::Cell;
        let expanded = Cell::new(false);
        let result = read_position_with_toolbar(
            &CaptureManager::default(),
            || {
                Ok(if expanded.get() {
                    navigation(true, Some(1), Some(196))
                } else {
                    navigation(false, None, None)
                })
            },
            || {
                expanded.set(true);
                Ok(())
            },
            Duration::ZERO,
        );
        assert!(result.unwrap_err().contains("操作バーを閉じられません"));
    }

    #[test]
    fn does_not_toggle_controls_on_a_non_reader_or_open_toc() {
        for toc_open in [false, true] {
            let result = read_position_with_toolbar(
                &CaptureManager::default(),
                || {
                    let mut ui = navigation(false, None, None);
                    ui.known = toc_open;
                    ui.toc_open = toc_open;
                    Ok(ui)
                },
                || panic!("must not click an unknown screen or TOC"),
                Duration::ZERO,
            );
            assert!(result.is_err());
        }
    }

    #[test]
    fn cancellation_during_position_read_does_not_send_any_more_clicks() {
        use std::cell::Cell;
        let manager = CaptureManager::default();
        let expanded = Cell::new(false);
        let toggles = Cell::new(0);
        let result = read_position_with_toolbar(
            &manager,
            || {
                if expanded.get() {
                    manager.stop();
                }
                Ok(if expanded.get() {
                    navigation(true, Some(1), Some(196))
                } else {
                    navigation(false, None, None)
                })
            },
            || {
                toggles.set(toggles.get() + 1);
                expanded.set(true);
                Ok(())
            },
            Duration::ZERO,
        );
        assert!(result.unwrap_err().contains("手動で停止"));
        assert_eq!(toggles.get(), 1);
    }
}
