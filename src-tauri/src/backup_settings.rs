use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PageLayout {
    #[default]
    Single,
    Spread,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BackupSettings {
    pub wait_timeout_ms: u64,
    pub max_pages: u32,
    pub page_layout: PageLayout,
    pub key: String,
}

impl Default for BackupSettings {
    fn default() -> Self {
        Self {
            wait_timeout_ms: 30_000,
            max_pages: 1000,
            page_layout: PageLayout::Single,
            key: "left".into(),
        }
    }
}

impl BackupSettings {
    pub fn validate(&self) -> Result<(), String> {
        if !(1000..=120_000).contains(&self.wait_timeout_ms)
            || !(1..=10_000).contains(&self.max_pages)
            || !matches!(self.key.as_str(), "left" | "right" | "pagedown" | "space")
        {
            return Err("バックアップ設定が不正です。waitTimeoutMs 1000〜120000ms、上限1〜10000枚、key left/right/pagedown/spaceを指定してください".into());
        }
        Ok(())
    }
}

pub fn load(path: &Path) -> Result<BackupSettings, String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let defaults = BackupSettings::default();
            save(path, &defaults)?;
            return Ok(defaults);
        }
        Err(e) => return Err(format!("バックアップ設定を読めません: {e}")),
    };
    let mut value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("バックアップ設定を読めません: {e}"))?;
    let mut migrated = false;
    if let Some(fields) = value.as_object_mut() {
        if !fields.contains_key("pageLayout") {
            let layout = if fields.get("portrait").and_then(|value| value.as_bool()) == Some(false)
            {
                "spread"
            } else {
                "single"
            };
            fields.insert("pageLayout".into(), layout.into());
            migrated = true;
        }
        for key in ["autoWait", "intervalMs", "stableCount", "portrait"] {
            migrated |= fields.remove(key).is_some();
        }
    }
    let settings: BackupSettings =
        serde_json::from_value(value).map_err(|e| format!("バックアップ設定を読めません: {e}"))?;
    settings.validate()?;
    if migrated {
        save(path, &settings)?;
    }
    Ok(settings)
}

pub fn save(path: &Path, settings: &BackupSettings) -> Result<(), String> {
    settings.validate()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        fs::write(
            &temporary,
            serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        fs::rename(&temporary, path).map_err(|e| format!("バックアップ設定を保存できません: {e}"))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub fn update_visible_settings(path: &Path, page_layout: PageLayout) -> Result<(), String> {
    let mut current = load(path)?;
    current.page_layout = page_layout;
    save(path, &current)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ui_updates_preserve_file_only_settings() {
        let path = std::env::temp_dir().join(format!("mmk-backup-{}.json", uuid::Uuid::new_v4()));
        let mut settings = load(&path).unwrap();
        settings.max_pages = 2345;
        settings.key = "right".into();
        settings.wait_timeout_ms = 45_000;
        save(&path, &settings).unwrap();
        update_visible_settings(&path, PageLayout::Spread).unwrap();
        let updated = load(&path).unwrap();
        assert_eq!(updated.page_layout, PageLayout::Spread);
        assert_eq!(updated.max_pages, 2345);
        assert_eq!(updated.key, "right");
        assert_eq!(updated.wait_timeout_ms, 45_000);
        settings.wait_timeout_ms = 0;
        assert!(save(&path, &settings).is_err());
        assert_eq!(load(&path).unwrap().wait_timeout_ms, 45_000);
        fs::write(&path, b"broken").unwrap();
        assert!(load(&path).is_err());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn migrates_old_layout_and_removes_fixed_wait_settings() {
        for (portrait, expected) in [(true, PageLayout::Single), (false, PageLayout::Spread)] {
            let path =
                std::env::temp_dir().join(format!("mmk-backup-{}.json", uuid::Uuid::new_v4()));
            fs::write(&path, format!(r#"{{"autoWait":false,"intervalMs":0,"stableCount":0,"portrait":{portrait},"maxPages":2345}}"#)).unwrap();
            let settings = load(&path).unwrap();
            assert_eq!(settings.page_layout, expected);
            assert_eq!(settings.max_pages, 2345);
            let json: serde_json::Value =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            for key in ["autoWait", "intervalMs", "stableCount", "portrait"] {
                assert!(json.get(key).is_none());
            }
            assert_eq!(load(&path).unwrap().page_layout, expected);
            fs::remove_file(path).unwrap();
        }
    }
}
