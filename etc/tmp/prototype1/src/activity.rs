use active_win_pos_rs::get_active_window;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use tracing::{error, debug};

#[derive(Clone, Debug, Serialize)]
pub struct ActivityLog {
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub app_name: String,
    pub window_title: String,
    pub is_captured: bool,
}

pub struct ActivityMonitor {
    current_log: Option<ActivityLog>,
    log_file_path: PathBuf,
    blocked_apps: Vec<String>,
    blocked_titles: Vec<String>,
}

impl ActivityMonitor {
    pub fn new(log_file_path: PathBuf) -> Self {
        Self {
            current_log: None,
            log_file_path,
            // ブラックリスト（小文字で比較）
            blocked_apps: vec![
                "spotify".to_string(),
                "slack".to_string(),
                "line".to_string(),
                "discord".to_string(),
            ],
            blocked_titles: vec![
                "private".to_string(),
                "incognito".to_string(),
                "secret".to_string(),
            ],
        }
    }

    /// 現在のアクティブウィンドウをチェックし、ログを更新する
    /// 戻り値: キャプチャを許可するかどうか (true: 許可, false: 禁止)
    pub fn check_activity(&mut self) -> bool {
        let now = Utc::now();
        let active_window = match get_active_window() {
            Ok(window) => window,
            Err(_) => {
                // ウィンドウ情報が取れない場合はデフォルト許可、または前回の状態を維持
                // ここでは安全側に倒して許可し、ログは "Unknown" とする
                return true;
            }
        };

        let app_name = active_window.app_name;
        let window_title = active_window.title;
        let is_blocked = self.is_blocked(&app_name, &window_title);

        // 状態が変わったかチェック
        let changed = if let Some(current) = &self.current_log {
            current.app_name != app_name || 
            current.window_title != window_title ||
            current.is_captured != !is_blocked // is_blocked == true なら is_captured == false
        } else {
            true
        };

        if changed {
            // 前回のログを確定して書き出し
            if let Some(mut log) = self.current_log.take() {
                log.end_time = now;
                self.write_log(&log);
            }

            // 新しいログを開始
            self.current_log = Some(ActivityLog {
                start_time: now,
                end_time: now, // 一旦現在時刻
                app_name,
                window_title,
                is_captured: !is_blocked,
            });
        } else {
            // 継続中：end_timeのみ更新（メモリ上）
            if let Some(log) = &mut self.current_log {
                log.end_time = now;
            }
        }

        !is_blocked
    }

    fn is_blocked(&self, app_name: &str, title: &str) -> bool {
        let app_lower = app_name.to_lowercase();
        let title_lower = title.to_lowercase();

        for blocked in &self.blocked_apps {
            if app_lower.contains(blocked) {
                debug!("Blocked app detected: {}", app_name);
                return true;
            }
        }

        for blocked in &self.blocked_titles {
            if title_lower.contains(blocked) {
                debug!("Blocked title detected: {}", title);
                return true;
            }
        }

        false
    }

    fn write_log(&self, log: &ActivityLog) {
        let json = match serde_json::to_string(log) {
            Ok(j) => j,
            Err(e) => {
                error!("Failed to serialize activity log: {}", e);
                return;
            }
        };

        let mut file = match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_file_path)
        {
            Ok(f) => f,
            Err(e) => {
                error!("Failed to open log file: {}", e);
                return;
            }
        };

        if let Err(e) = writeln!(file, "{}", json) {
            error!("Failed to write to log file: {}", e);
        }
    }
    
    // アプリケーション終了時に呼び出して最後のログを書き込む
    pub fn flush(&mut self) {
        if let Some(log) = self.current_log.take() {
            self.write_log(&log);
        }
    }
}


