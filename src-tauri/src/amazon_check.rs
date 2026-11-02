use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{Emitter, Manager};

use crate::database::Database;
use crate::models::{
    AmazonCheckResult, AmazonContinueSeriesItemInput, AmazonContinueSeriesItemView,
};

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    Idle,
    Checking,
    LoginRequired,
    Success,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub version: u64,
    pub phase: Phase,
    pub message: String,
    pub last_success: Option<AmazonCheckResult>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub status: Status,
    pub items: Vec<AmazonContinueSeriesItemView>,
}

struct Check {
    id: u64,
    status: Status,
    progress_at: Instant,
}

impl Check {
    fn active(&self, id: u64) -> bool {
        self.id == id && matches!(self.status.phase, Phase::Checking | Phase::LoginRequired)
    }

    fn update(&mut self, phase: Phase, message: &str) -> Status {
        self.status.version += 1;
        self.status.phase = phase;
        self.status.message = message.to_owned();
        self.progress_at = Instant::now();
        self.status.clone()
    }
}

pub struct ManagerState(Mutex<Check>);

impl ManagerState {
    pub fn new(last_success: Option<AmazonCheckResult>) -> Self {
        Self(Mutex::new(Check {
            id: 0,
            status: Status {
                version: 0,
                phase: Phase::Idle,
                message: if last_success.is_some() {
                    "前回の取得結果です。現在のログイン状態は再チェックで確認します"
                } else {
                    "まだ取得を確認していません。「再チェック」を押してください"
                }
                .to_owned(),
                last_success,
            },
            progress_at: Instant::now(),
        }))
    }

    pub fn snapshot(&self) -> Status {
        self.0.lock().unwrap().status.clone()
    }

    pub fn snapshot_with_items(&self, database: &Database) -> Result<Snapshot, String> {
        let check = self.0.lock().unwrap();
        Ok(Snapshot {
            status: check.status.clone(),
            items: database.list_amazon_continue_series()?,
        })
    }

    pub fn start(&self) -> Result<(u64, Status), String> {
        let mut check = self.0.lock().unwrap();
        if check.active(check.id) {
            return Err("新刊チェックは既に実行中です".to_owned());
        }
        check.id += 1;
        let status = check.update(Phase::Checking, "Amazonの候補を取得しています");
        Ok((check.id, status))
    }

    pub fn update(&self, id: u64, phase: Phase, message: &str) -> Option<Status> {
        let mut check = self.0.lock().unwrap();
        check.active(id).then(|| check.update(phase, message))
    }

    pub fn complete(
        &self,
        id: u64,
        database: &Database,
        items: Vec<AmazonContinueSeriesItemInput>,
    ) -> Result<Status, String> {
        let mut check = self.0.lock().unwrap();
        if !check.active(id) {
            return Err("終了済みの新刊チェックからの取得結果を拒否しました".to_owned());
        }
        // Keep the watchdog and late page callbacks from racing with the database commit.
        let result = database.replace_amazon_continue_series(items)?;
        check.status.last_success = Some(result);
        Ok(check.update(Phase::Success, "Amazonの候補を取得しました"))
    }

    pub fn clear(&self) -> Status {
        let mut check = self.0.lock().unwrap();
        check.id += 1;
        check.status.last_success = None;
        check.update(
            Phase::Idle,
            "Amazon連携を解除しました。再チェックにはログインが必要です",
        )
    }

    fn expired(&self, id: u64) -> Option<Status> {
        let mut check = self.0.lock().unwrap();
        if !check.active(id)
            || check.status.phase != Phase::Checking
            || check.progress_at.elapsed() < Duration::from_secs(90)
        {
            return None;
        }
        Some(check.update(
            Phase::Error,
            "Amazonから応答を取得できませんでした。通信状態やログイン状態を再チェックしてください",
        ))
    }
}

pub fn publish(app: &tauri::AppHandle, status: Status) {
    let _ = app.emit_to("main", "amazon-check-status", status);
}

pub fn fail(app: &tauri::AppHandle, id: u64, message: &str) {
    if let Some(status) = app
        .state::<ManagerState>()
        .update(id, Phase::Error, message)
    {
        publish(app, status);
    }
}

pub fn watch(app: tauri::AppHandle, id: u64) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(1));
        let state = app.state::<ManagerState>();
        if let Some(status) = state.expired(id) {
            if let Some(window) =
                app.get_webview_window(crate::amazon::AMAZON_CONTINUE_SERIES_LABEL)
            {
                let _ = window.close();
            }
            publish(&app, status);
            break;
        }
        if !state.0.lock().unwrap().active(id) {
            break;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_idle_login_error_and_ignores_late_callbacks() {
        let state = ManagerState::new(None);
        assert_eq!(state.snapshot().phase, Phase::Idle);
        let (id, _) = state.start().unwrap();
        assert!(state.start().is_err());
        state.update(id, Phase::LoginRequired, "login").unwrap();
        assert_eq!(state.snapshot().phase, Phase::LoginRequired);
        state.update(id, Phase::Error, "failed").unwrap();
        assert!(state.update(id, Phase::Checking, "late").is_none());
        let (next, _) = state.start().unwrap();
        assert_ne!(id, next);
        assert!(state.update(id, Phase::Error, "stale").is_none());
        state.clear();
        assert!(state.update(next, Phase::Error, "stale").is_none());
    }

    #[test]
    fn watchdog_times_out_checking_but_not_login_and_preserves_previous_success() {
        let result = AmazonCheckResult {
            completed_at: "2026-11-03T00:00:00Z".into(),
            received_count: 0,
            stored_count: 0,
        };
        let state = ManagerState::new(Some(result));
        let (id, _) = state.start().unwrap();
        state.0.lock().unwrap().progress_at = Instant::now() - Duration::from_secs(91);
        assert_eq!(state.expired(id).unwrap().phase, Phase::Error);
        assert!(state.snapshot().last_success.is_some());
        let (id, _) = state.start().unwrap();
        state.update(id, Phase::LoginRequired, "login");
        state.0.lock().unwrap().progress_at = Instant::now() - Duration::from_secs(91);
        assert!(state.expired(id).is_none());
    }
}
