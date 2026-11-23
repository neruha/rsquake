//! 電文のリプレイ (開発用)。
//!
//! 受け付けるもの:
//! - JSONL ファイル (1 行 1 メッセージ。受信ログをそのまま使える)
//! - JSON ファイル 1 つ
//! - 連番 JSON のフォルダ (テストサーバーの `assets/eew-seq/*`)
//!
//! DMDATA の電文と Wolfx の `jma_eew` のどちらも読める (1 件ずつ形で見分ける)。
//! 間隔は電文の報の時刻の差を再現する。長い空白は `max_gap` で詰める。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::time::Duration;

use crate::eew::report::{Incoming, parse_message};
use crate::eew::wolfx::{self, RegionCodes};
use crate::net::{LinkState, NetEvent};

pub struct ReplayOptions {
    pub paths: Vec<PathBuf>,
    /// 固定間隔。`None` なら電文の時刻差を使う
    pub interval: Option<Duration>,
    pub max_gap: Duration,
    pub start_delay: Duration,
    /// Wolfx の区域名 → code
    pub region_codes: Arc<RegionCodes>,
}

/// Wolfx の形 (`type: "jma_eew"`、または `type` が無く `EventID` がある) か。
fn is_wolfx(text: &str) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return false;
    };
    match v.get("type").and_then(serde_json::Value::as_str) {
        Some(t) => t == "jma_eew",
        None => v.get("EventID").is_some(),
    }
}

/// 1 メッセージを、形に合わせて DMDATA か Wolfx として読む。
pub fn parse_any(text: &str, codes: &RegionCodes) -> Incoming {
    if is_wolfx(text) {
        wolfx::parse_message(text, true, codes)
    } else {
        parse_message(text, true)
    }
}

pub fn load_messages(path: &Path) -> anyhow::Result<Vec<String>> {
    if path.is_dir() {
        let mut files: Vec<PathBuf> = std::fs::read_dir(path)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        files.sort_by_key(|p| {
            let stem = p
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            (stem.parse::<u64>().unwrap_or(u64::MAX), stem)
        });
        return files
            .iter()
            .map(|p| Ok(std::fs::read_to_string(p)?))
            .collect();
    }
    let text = std::fs::read_to_string(path)?;
    if path.extension().is_some_and(|x| x == "jsonl") {
        Ok(text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(str::to_string)
            .collect())
    } else {
        Ok(vec![text])
    }
}

pub fn spawn(opts: ReplayOptions, tx: Sender<NetEvent>, ctx: egui::Context) {
    std::thread::Builder::new()
        .name("replay".into())
        .spawn(move || {
            let send = |e: NetEvent| {
                let ok = tx.send(e).is_ok();
                ctx.request_repaint();
                ok
            };
            send(NetEvent::Link(LinkState::Connected));
            std::thread::sleep(opts.start_delay);
            for path in &opts.paths {
                let messages = match load_messages(path) {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::error!("{} を読めない: {e}", path.display());
                        continue;
                    }
                };
                tracing::info!("リプレイ: {} ({} 件)", path.display(), messages.len());
                let mut prev: Option<chrono::DateTime<chrono::Utc>> = None;
                for text in messages {
                    let incoming = parse_any(&text, &opts.region_codes);
                    let at = match &incoming {
                        Incoming::Report(r) => Some(r.report_time),
                        _ => None,
                    };
                    let wait = match (opts.interval, prev, at) {
                        (Some(i), _, _) => i,
                        (None, Some(p), Some(a)) => {
                            (a - p).to_std().unwrap_or_default().min(opts.max_gap)
                        }
                        (None, None, _) => Duration::ZERO,
                        (None, _, None) => Duration::from_secs(1),
                    };
                    if at.is_some() {
                        prev = at;
                    }
                    std::thread::sleep(wait);
                    match incoming {
                        Incoming::Ignored(why) => tracing::debug!("無視: {why}"),
                        Incoming::Status(u) => {
                            send(NetEvent::Upstream(u));
                        }
                        m => {
                            if !send(NetEvent::Message(m)) {
                                return;
                            }
                        }
                    }
                }
            }
            tracing::info!("リプレイ終了");
        })
        .expect("replay thread");
}
