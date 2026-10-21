//! Local, bounded JSONL diagnostics. Callers supply operational fields only,
//! never reader content, URLs, titles, credentials, or full capture options.
use serde_json::{json, Value};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{Instant, SystemTime},
};

const MAX_BYTES: u64 = 2 * 1024 * 1024;
const KEEP_RUNS: usize = 10;

#[derive(Default)]
pub struct CaptureLog {
    directory: Option<PathBuf>,
    active: Option<RunLog>,
    error: Option<String>,
}

struct RunLog {
    path: PathBuf,
    previous: PathBuf,
    file: Option<File>,
    run_id: String,
    started: Instant,
    sequence: u64,
    bytes: u64,
    max_bytes: u64,
}

impl CaptureLog {
    pub fn configure(&mut self, directory: PathBuf) {
        self.directory = Some(directory);
    }

    pub fn start(&mut self, fields: Value) {
        self.active = None;
        self.error = None;
        let Some(directory) = &self.directory else {
            return;
        };
        let result = (|| {
            fs::create_dir_all(directory)?;
            prune(directory, KEEP_RUNS - 1)?;
            RunLog::new(directory, MAX_BYTES)
        })();
        match result {
            Ok(log) => {
                self.active = Some(log);
                self.record("run_started", fields);
            }
            Err(_) => self.error = Some("操作ログの作成に失敗しました".into()),
        }
    }

    pub fn record(&mut self, event: &str, fields: Value) {
        if self.error.is_some() {
            return;
        }
        if let Some(log) = &mut self.active {
            if log.write(event, fields).is_err() {
                // Logging must never interrupt a backup or recursively log errors.
                self.error = Some("操作ログの書き込みに失敗しました".into());
                log.file = None;
            }
        }
    }

    pub fn path(&self) -> Option<String> {
        self.active
            .as_ref()
            .map(|log| log.path.to_string_lossy().into_owned())
    }

    pub fn error(&self) -> Option<String> {
        self.error.clone()
    }
}

impl RunLog {
    fn new(directory: &Path, max_bytes: u64) -> std::io::Result<Self> {
        let run_id = uuid::Uuid::new_v4().to_string();
        let path = directory.join(format!("backup-{run_id}.jsonl"));
        let previous = directory.join(format!("backup-{run_id}.previous.jsonl"));
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        Ok(Self {
            path,
            previous,
            file: Some(file),
            run_id,
            started: Instant::now(),
            sequence: 0,
            bytes: 0,
            max_bytes,
        })
    }

    fn write(&mut self, event: &str, fields: Value) -> std::io::Result<()> {
        self.sequence += 1;
        let record = json!({
            "version": 1,
            "time": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            "runId": self.run_id,
            "sequence": self.sequence,
            "elapsedMs": self.started.elapsed().as_millis() as u64,
            "event": event,
            "fields": fields,
        });
        let mut line = serde_json::to_vec(&record)?;
        line.push(b'\n');
        if line.len() as u64 > self.max_bytes {
            return Err(std::io::Error::other("log entry too large"));
        }
        if self.bytes + line.len() as u64 > self.max_bytes {
            self.file = None; // Close the handle before renaming on Windows.
            if self.previous.try_exists()? {
                fs::remove_file(&self.previous)?;
            }
            fs::rename(&self.path, &self.previous)?;
            self.file = Some(
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&self.path)?,
            );
            self.bytes = 0;
        }
        self.file
            .as_mut()
            .ok_or_else(|| std::io::Error::other("log closed"))?
            .write_all(&line)?;
        self.bytes += line.len() as u64;
        Ok(())
    }
}

fn run_name(name: &str) -> Option<&str> {
    let id = name.strip_prefix("backup-")?.strip_suffix(".jsonl")?;
    let id = id.strip_suffix(".previous").unwrap_or(id);
    uuid::Uuid::parse_str(id).ok()?;
    Some(id)
}

fn prune(directory: &Path, keep: usize) -> std::io::Result<()> {
    let mut groups = std::collections::BTreeMap::<String, (SystemTime, Vec<PathBuf>)>::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(id) = run_name(name) else {
            continue;
        };
        let modified = entry.metadata()?.modified()?;
        let group = groups
            .entry(id.to_owned())
            .or_insert((modified, Vec::new()));
        group.0 = group.0.max(modified);
        group.1.push(entry.path());
    }
    let mut groups: Vec<_> = groups.into_values().collect();
    groups.sort_by_key(|(time, _)| *time);
    let remove = groups.len().saturating_sub(keep);
    for (_, files) in groups.into_iter().take(remove) {
        for path in files {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

// Error strings may contain book titles or local paths. Emit only a bounded
// classification; the original error remains available in the existing UI.
pub fn failure_reason(error: &str) -> &'static str {
    if error.contains("Esc") {
        "escape"
    } else if error.contains("手動") {
        "cancelled"
    } else if error.contains("フォーカス")
        || error.contains("最小化")
        || error.contains("ウィンドウが閉じ")
        || error.contains("リーダーが閉じ")
        || error.contains("サイズが変")
        || error.contains("タイトルが変")
    {
        "window_guard"
    } else if error.contains("別の書籍") || error.contains("本以外") {
        "wrong_book"
    } else if error.contains("変わりません") || error.contains("正常に進まない") {
        "position_unchanged"
    } else if error.contains("読書位置が変")
        || error.contains("飛び越し")
        || error.contains("逆行")
        || error.contains("開始位置")
    {
        "position_mismatch"
    } else if error.contains("現在位置と総数") {
        "position_unavailable"
    } else if error.contains("時間切れ") || error.contains("制限時間") || error.contains("90秒")
    {
        "timeout"
    } else if error.contains("キーを送信") || error.contains("修飾キー") {
        "input_failed"
    } else if error.contains("操作バー") || error.contains("目次") {
        "reader_controls"
    } else if error.contains("描画形式") {
        "unsupported_renderer"
    } else {
        "operation_failed"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn directory() -> PathBuf {
        let path = std::env::temp_dir().join(format!("mmk-log-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        path
    }
    fn clean(path: &Path) {
        for entry in fs::read_dir(path).unwrap() {
            fs::remove_file(entry.unwrap().path()).unwrap();
        }
        fs::remove_dir(path).unwrap();
    }

    #[test]
    fn records_ordered_json_lines_and_distinguishes_runs() {
        let dir = directory();
        let mut log = CaptureLog::default();
        log.configure(dir.clone());
        log.start(json!({"mode":"batch"}));
        log.record("key_send_accepted", json!({"key":"left"}));
        let path = log.path().unwrap();
        let records: Vec<Value> = fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["sequence"], 1);
        assert_eq!(records[1]["sequence"], 2);
        assert_eq!(records[1]["fields"]["key"], "left");
        log.start(json!({}));
        assert_ne!(log.path().unwrap(), path);
        drop(log);
        clean(&dir);
    }

    #[test]
    fn bounds_size_and_rotates_to_a_single_previous_file() {
        let dir = directory();
        let mut log = RunLog::new(&dir, 600).unwrap();
        for _ in 0..20 {
            log.write("heartbeat", json!({"polls":100})).unwrap();
        }
        assert!(fs::metadata(&log.path).unwrap().len() <= 600);
        assert!(fs::metadata(&log.previous).unwrap().len() <= 600);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
        let last: Value = serde_json::from_str(
            fs::read_to_string(&log.path)
                .unwrap()
                .lines()
                .last()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(last["sequence"], 20);
        drop(log);
        clean(&dir);
    }

    #[test]
    fn prunes_only_owned_logs_and_preserves_unrelated_files() {
        let dir = directory();
        fs::write(dir.join("notes.txt"), "user data").unwrap();
        fs::write(dir.join("backup-personal.jsonl"), "user data").unwrap();
        for _ in 0..4 {
            let mut log = RunLog::new(&dir, 600).unwrap();
            log.write("start", json!({})).unwrap();
        }
        prune(&dir, 2).unwrap();
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 4);
        assert_eq!(
            fs::read_to_string(dir.join("notes.txt")).unwrap(),
            "user data"
        );
        prune(&dir, 0).unwrap();
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
        clean(&dir);
    }

    #[test]
    fn keeps_only_the_latest_ten_runs() {
        let dir = directory();
        let mut log = CaptureLog::default();
        log.configure(dir.clone());
        for _ in 0..KEEP_RUNS + 3 {
            log.start(json!({}));
        }
        assert_eq!(fs::read_dir(&dir).unwrap().count(), KEEP_RUNS);
        assert!(log.error().is_none());
        drop(log);
        clean(&dir);
    }

    #[test]
    fn write_failures_do_not_escape_and_error_messages_do_not_leak_details() {
        let dir = directory();
        let blocked = dir.join("not-a-directory");
        fs::write(&blocked, "occupied").unwrap();
        let mut log = CaptureLog::default();
        log.configure(blocked);
        log.start(json!({}));
        log.record("ignored", json!({}));
        assert!(log.error().is_some());
        assert_eq!(
            failure_reason("Private book: ページ送り後も現在位置が変わりません C:\\Users\\secret"),
            "position_unchanged"
        );
        assert_eq!(
            failure_reason("password=secret https://example.com/token"),
            "operation_failed"
        );
        clean(&dir);
    }

    #[test]
    fn active_write_error_disables_logging_without_panicking() {
        let dir = directory();
        let mut log = CaptureLog::default();
        log.configure(dir.clone());
        log.start(json!({}));
        log.active.as_mut().unwrap().file = None;
        log.record("failed_write", json!({}));
        assert!(log.error().is_some());
        log.record("ignored_after_error", json!({}));
        log.start(json!({"retry":true}));
        assert!(log.error().is_none());
        drop(log);
        clean(&dir);
    }

    #[test]
    fn toolbar_close_failures_are_not_window_guard_failures() {
        for message in [
            "操作バーを閉じられません。本文中央をクリックしてから再試行してください",
            "位置確認後に操作バーを閉じられません。スクリーンショットは保存しません",
            "目次パネルが閉じませんでした",
        ] {
            assert_eq!(failure_reason(message), "reader_controls", "{message}");
        }
        for message in [
            "内蔵リーダーが閉じられました",
            "対象ウィンドウが閉じられたか、最小化されました",
            "対象ウィンドウからフォーカスが外れました",
        ] {
            assert_eq!(failure_reason(message), "window_guard", "{message}");
        }
    }
}
