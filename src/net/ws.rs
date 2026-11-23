//! EEW の受信元 (DMDATA の proxy / Wolfx) への WebSocket 接続。切れたらつなぎ直す。
//!
//! 接続・再接続・無通信判定・ログは共通で、メッセージの解釈だけを受信元ごとに替える。

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::Notify;
use tokio::time::{Instant, interval, interval_at, sleep};
use tokio_tungstenite::tungstenite::Message;

use super::{LinkState, NetEvent};
use crate::config::{Config, EewSource};
use crate::eew::report::{Incoming, Upstream, parse_message};
use crate::eew::wolfx;

const PING_EVERY: Duration = Duration::from_secs(10);
/// この間なにも届かなければ切れたとみなす
const SILENCE_LIMIT: Duration = Duration::from_secs(35);
const BACKOFF_MIN: Duration = Duration::from_millis(500);
const BACKOFF_MAX: Duration = Duration::from_secs(5);
/// `GET /health` を見る間隔。proxy の上流が切れたことに、接続後も気づくため
const HEALTH_EVERY: Duration = Duration::from_secs(30);
const HEALTH_TIMEOUT: Duration = Duration::from_secs(5);

pub struct WsOptions {
    pub url: String,
    pub source: EewSource,
    pub accept_non_normal: bool,
    /// `status` を待たない (テストモード)
    pub test_mode: bool,
    pub log_dir: Option<PathBuf>,
    /// Wolfx 用。区域名 → code
    pub region_codes: Arc<HashMap<String, String>>,
    /// proxy の `/health` を見る間隔。`None` なら見ない
    pub health_interval: Option<Duration>,
}

impl WsOptions {
    pub fn from_config(config: &Config, region_codes: Arc<HashMap<String, String>>) -> Self {
        Self {
            url: config.ws_url(),
            source: config.source,
            accept_non_normal: config.accept_non_normal(),
            test_mode: config.test_mode,
            log_dir: config
                .log_telegrams
                .then(|| Config::dir().map(|d| d.join("logs")))
                .flatten(),
            region_codes,
            // /health は proxy だけ。テストモード (テストサーバー) と Wolfx には無い
            health_interval: (config.source == EewSource::Dmdata && !config.test_mode)
                .then_some(HEALTH_EVERY),
        }
    }

    /// これが変わったら、つなぎ直す必要がある。
    fn key(&self) -> WsKey {
        WsKey {
            url: self.url.clone(),
            source: self.source,
            accept_non_normal: self.accept_non_normal,
            test_mode: self.test_mode,
            logging: self.log_dir.is_some(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WsKey {
    url: String,
    source: EewSource,
    accept_non_normal: bool,
    test_mode: bool,
    logging: bool,
}

/// 受信スレッドを止めるためのハンドル。落としても止まらない (`stop` を呼んだときだけ止まる)。
pub struct WsHandle {
    alive: Arc<Mutex<bool>>,
    wake: Arc<Notify>,
}

impl WsHandle {
    /// 止める。戻った時点で、このスレッドはもう `NetEvent` を送らない。
    /// 進行中の送信は完了を待つので、あとから起こす新しいスレッドのイベントより必ず先に届く。
    pub fn stop(&self) {
        *self.alive.lock().unwrap_or_else(|p| p.into_inner()) = false;
        self.wake.notify_one();
    }
}

/// 受信元の切り替えに合わせて、受信スレッドを入れ替える。
pub struct WsSupervisor {
    current: WsHandle,
    key: WsKey,
    tx: Sender<NetEvent>,
    ctx: egui::Context,
    region_codes: Arc<HashMap<String, String>>,
}

impl WsSupervisor {
    pub fn start(
        config: &Config,
        tx: Sender<NetEvent>,
        ctx: egui::Context,
        region_codes: Arc<HashMap<String, String>>,
    ) -> Self {
        let opts = WsOptions::from_config(config, region_codes.clone());
        let key = opts.key();
        let current = spawn(opts, tx.clone(), ctx.clone());
        Self {
            current,
            key,
            tx,
            ctx,
            region_codes,
        }
    }

    /// 受信元・接続先・テストモード・訓練報の扱い・ログ保存のどれかが変わっていたら、
    /// 古いスレッドを止めて新しく起こす。起こしたら `true`。
    pub fn reconnect_if_changed(&mut self, config: &Config) -> bool {
        let opts = WsOptions::from_config(config, self.region_codes.clone());
        let key = opts.key();
        if key == self.key {
            return false;
        }
        tracing::info!("つなぎ直す: {:?} -> {:?}", self.key, key);
        self.current.stop();
        self.current = spawn(opts, self.tx.clone(), self.ctx.clone());
        self.key = key;
        true
    }
}

pub fn spawn(opts: WsOptions, tx: Sender<NetEvent>, ctx: egui::Context) -> WsHandle {
    let alive = Arc::new(Mutex::new(true));
    let wake = Arc::new(Notify::new());
    let handle = WsHandle {
        alive: alive.clone(),
        wake: wake.clone(),
    };
    std::thread::Builder::new()
        .name("ws".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            rt.block_on(run(opts, tx, ctx, alive, wake));
        })
        .expect("ws thread");
    handle
}

async fn run(
    opts: WsOptions,
    tx: Sender<NetEvent>,
    ctx: egui::Context,
    alive: Arc<Mutex<bool>>,
    wake: Arc<Notify>,
) {
    // wss 用
    super::install_tls_provider();

    // 生きているかの確認と送信を同じロックの中で行う。止められたあとは何も送らない
    let send = |e: NetEvent| {
        let guard = alive.lock().unwrap_or_else(|p| p.into_inner());
        if !*guard {
            return false;
        }
        let ok = tx.send(e).is_ok();
        drop(guard);
        ctx.request_repaint();
        ok
    };
    let mut backoff = BACKOFF_MIN;
    loop {
        if !send(NetEvent::Link(LinkState::Connecting)) {
            return;
        }
        tracing::info!("接続: {}", opts.url);
        let connect = tokio_tungstenite::connect_async(opts.url.as_str());
        let connected = tokio::select! {
            r = connect => r,
            _ = wake.notified() => return,
        };
        match connected {
            Ok((stream, _)) => {
                backoff = BACKOFF_MIN;
                send(NetEvent::Link(LinkState::Connected));
                // Wolfx は上流の状態を送らない。テストモードの DMDATA と同じ扱い
                if opts.test_mode || opts.source == EewSource::Wolfx {
                    send(NetEvent::Upstream(Upstream::Unknown));
                }
                let reason = match opts
                    .health_interval
                    .and_then(|every| health_url(&opts.url).map(|url| (url, every)))
                {
                    // 接続中だけ /health を見る。session が終わればこちらも打ち切られる
                    Some((url, every)) => tokio::select! {
                        r = session(stream, &opts, &send, &wake) => r,
                        _ = health_loop(url, every, &send) => "health 監視が終了".into(),
                    },
                    None => session(stream, &opts, &send, &wake).await,
                };
                tracing::warn!("切断: {reason}");
            }
            Err(e) => tracing::warn!("接続できない: {e}"),
        }
        if !send(NetEvent::Link(LinkState::Disconnected)) {
            return;
        }
        tokio::select! {
            _ = sleep(backoff) => {}
            _ = wake.notified() => return,
        }
        backoff = (backoff * 2).min(BACKOFF_MAX);
    }
}

/// proxy の `/health` の URL。平文の `ws://` だけ (HTTP クライアントは TLS を持たない)。
pub fn health_url(ws_url: &str) -> Option<String> {
    let rest = ws_url.strip_prefix("ws://")?;
    let host = rest.split(['/', '?']).next().filter(|h| !h.is_empty())?;
    Some(format!("http://{host}/health"))
}

/// `/health` の本文から、上流の状態を読む。
pub fn parse_health(body: &str) -> Option<Upstream> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    match v.get("upstream")?.as_str()? {
        "connected" => Some(Upstream::Connected),
        "disconnected" => Some(Upstream::Disconnected),
        _ => None,
    }
}

/// `every` ごとに `/health` を見て、上流の状態を送る。止まらない (呼び出し側が打ち切る)。
/// 取れなかったときは何も送らない (WebSocket 側の切断は別に検知する)。
async fn health_loop(url: String, every: Duration, send: &impl Fn(NetEvent) -> bool) {
    super::install_tls_provider();
    let client = match reqwest::Client::builder()
        .timeout(HEALTH_TIMEOUT)
        .connect_timeout(HEALTH_TIMEOUT)
        .user_agent("rsquake")
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("/health のクライアントを作れない: {e}");
            return std::future::pending().await;
        }
    };
    // 接続直後の status で状態は分かっているので、最初は every だけ待つ
    let mut tick = interval_at(Instant::now() + every, every);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        let fetched = async {
            let res = client.get(&url).send().await.map_err(|e| e.to_string())?;
            let body = res.text().await.map_err(|e| e.to_string())?;
            parse_health(&body).ok_or_else(|| format!("読めない応答: {body}"))
        }
        .await;
        match fetched {
            Ok(u) => {
                send(NetEvent::Upstream(u));
            }
            Err(e) => tracing::debug!("/health を取れない: {e}"),
        }
    }
}

async fn session<S>(
    stream: tokio_tungstenite::WebSocketStream<S>,
    opts: &WsOptions,
    send: &impl Fn(NetEvent) -> bool,
    wake: &Notify,
) -> String
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (mut sink, mut source) = stream.split();
    let mut ping = interval(PING_EVERY);
    ping.tick().await;
    let mut last_seen = Instant::now();
    let mut log = opts.log_dir.clone().map(TelegramLog::new);
    let wolfx = opts.source == EewSource::Wolfx;
    // 接続直後の最新 1 件。進行中でない古い報は出さない
    let mut first_report = wolfx;

    if wolfx && let Err(e) = sink.send(Message::text("query_jmaeew")).await {
        return e.to_string();
    }

    loop {
        tokio::select! {
            _ = wake.notified() => return "停止".into(),
            msg = source.next() => {
                last_seen = Instant::now();
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        let text = text.as_str();
                        if text.eq_ignore_ascii_case("pong") || text.eq_ignore_ascii_case("ping") {
                            continue;
                        }
                        let incoming = if wolfx {
                            wolfx::parse_message(text, opts.accept_non_normal, &opts.region_codes)
                        } else {
                            if let Some(f) = log.as_mut() {
                                f.write(text);
                            }
                            parse_message(text, opts.accept_non_normal)
                        };
                        let event = match incoming {
                            Incoming::Status(u) if opts.test_mode => {
                                tracing::info!("status: {u:?} (テストモードでは使わない)");
                                continue;
                            }
                            Incoming::Status(u) => NetEvent::Upstream(u),
                            Incoming::Ignored(why) => {
                                tracing::debug!("無視: {why}");
                                continue;
                            }
                            Incoming::Report(r) => {
                                // heartbeat などは記録せず、報だけを記録する
                                if wolfx && let Some(f) = log.as_mut() {
                                    f.write(text);
                                }
                                let first = std::mem::take(&mut first_report);
                                if first
                                    && !opts.test_mode
                                    && wolfx::is_stale(&r, chrono::Utc::now())
                                {
                                    tracing::info!(
                                        "{} 第{}報は古いので捨てる ({})",
                                        r.event_id, r.serial, r.report_time
                                    );
                                    continue;
                                }
                                NetEvent::Message(Incoming::Report(r))
                            }
                        };
                        if !send(event) {
                            return "UI が閉じた、または停止".into();
                        }
                    }
                    Some(Ok(Message::Close(f))) => return format!("close {f:?}"),
                    Some(Ok(_)) => {}
                    Some(Err(e)) => return e.to_string(),
                    None => return "ストリーム終了".into(),
                }
            }
            _ = ping.tick() => {
                if last_seen.elapsed() > SILENCE_LIMIT {
                    return "無通信".into();
                }
                if let Err(e) = sink.send(Message::Ping(Vec::new().into())).await {
                    return e.to_string();
                }
                // Wolfx の推奨。pong が返る
                if wolfx && let Err(e) = sink.send(Message::text("ping")).await {
                    return e.to_string();
                }
            }
        }
    }
}
/// 受信した電文の JSONL。ファイルは日付 (ローカル時刻) ごとで、日付が変わったら開き直す。
struct TelegramLog {
    dir: PathBuf,
    /// いま開いているファイルの名前と中身
    file: Option<(String, std::fs::File)>,
}

impl TelegramLog {
    fn new(dir: PathBuf) -> Self {
        Self { dir, file: None }
    }

    fn file_name(now: chrono::DateTime<chrono::Local>) -> String {
        format!("telegrams-{}.jsonl", now.format("%Y%m%d"))
    }

    fn write(&mut self, text: &str) {
        let name = Self::file_name(chrono::Local::now());
        if self.file.as_ref().is_none_or(|(n, _)| *n != name) {
            self.file = std::fs::create_dir_all(&self.dir)
                .and_then(|()| {
                    std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(self.dir.join(&name))
                })
                .map_err(|e| tracing::warn!("ログを開けない: {e}"))
                .ok()
                .map(|f| (name, f));
        }
        let Some((_, f)) = self.file.as_mut() else {
            return;
        };
        // 1 行 1 メッセージにそろえる
        let line = if text.contains('\n') {
            serde_json::from_str::<serde_json::Value>(text)
                .map(|v| v.to_string())
                .unwrap_or_else(|_| text.replace('\n', " "))
        } else {
            text.to_string()
        };
        let _ = writeln!(f, "{line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_url_from_ws_url() {
        assert_eq!(
            health_url("ws://localhost:3000/ws?name=rsquake&v=2&topics=eew").as_deref(),
            Some("http://localhost:3000/health")
        );
        assert_eq!(
            health_url("ws://127.0.0.1:3000").as_deref(),
            Some("http://127.0.0.1:3000/health")
        );
        assert_eq!(health_url("wss://ws-api.wolfx.jp/jma_eew"), None);
        assert_eq!(health_url("ws://"), None);
    }

    #[test]
    fn health_upstream_is_read() {
        let ok = r#"{"status":"ok","database":"connected","upstream":"connected"}"#;
        assert_eq!(parse_health(ok), Some(Upstream::Connected));
        let degraded = r#"{"status":"degraded","database":"connected","upstream":"disconnected"}"#;
        assert_eq!(parse_health(degraded), Some(Upstream::Disconnected));
        assert_eq!(parse_health(r#"{"status":"ok"}"#), None);
        assert_eq!(parse_health(r#"{"upstream":"??"}"#), None);
        assert_eq!(parse_health("not json"), None);
    }
}
