//! 強震モニタ (NIED) のリアルタイム画像の取得。
//!
//! - 要求は 1 秒に 1 回まで。前の要求が終わるまで次を出さない (逐次処理なので重ならない)
//! - タイムアウトは 2 秒。失敗しても 1 秒ごとに試し続ける
//! - 画像の時刻 `T` は、パソコンの時計で `T + 遅れ` になった瞬間に取りに行く
//!   (秒の境目に合わせるので、切り捨てで余計に遅れない)
//! - 404 は「その時刻の画像がまだ無い」ので、遅れを 100 ms 増やす。
//!   取れ続けたら遅れを 100 ms ずつ減らし、なるべく小さい遅れに戻す
//!
//! 描画する色は画像の画素の色をそのまま使う。

use std::io::Cursor;
use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::time::Duration;

use chrono::{DateTime, FixedOffset, NaiveDateTime, TimeZone, Utc};
use egui::Color32;
use tokio::time::{Instant, sleep_until, timeout};

use super::NetEvent;
use crate::geo::stations::StationList;

pub const BASE_URL: &str = "http://www.kmoni.bosai.go.jp";

const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);
const REQUEST_INTERVAL: Duration = Duration::from_secs(1);

const DELAY_MIN_MS: i64 = 200;
/// 起動時に latest.json から求める遅れの上限
const DELAY_MAX_MS: i64 = 5_000;
/// 404 で増やしたあとの遅れの上限
const LAG_MAX_MS: i64 = 10_000;
const DELAY_DEFAULT_MS: i64 = 2_000;
const LAG_STEP_MS: i64 = 100;
/// 最後に 404 になった遅れより十分長いとき、これだけ続けて取れたら遅れを減らす
const FAST_STREAK: u32 = 5;
/// 404 の境目の近くでは、これだけ続けて取れたら遅れを減らしてみる
const SLOW_STREAK: u32 = 60;

/// 強震モニタの画像の種類。今は `jma_s` (リアルタイム震度) だけ。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageKind {
    #[default]
    RealtimeShindo,
}

impl ImageKind {
    fn slug(self) -> &'static str {
        match self {
            Self::RealtimeShindo => "jma_s",
        }
    }

    /// 画像の URL。時刻は JST で組み立てる。
    pub fn url(self, base: &str, at: DateTime<Utc>) -> String {
        let jst = at.with_timezone(&jst());
        let slug = self.slug();
        format!(
            "{}/data/map_img/RealTimeImg/{slug}/{}/{}.{slug}.gif",
            base.trim_end_matches('/'),
            jst.format("%Y%m%d"),
            jst.format("%Y%m%d%H%M%S"),
        )
    }
}

fn jst() -> FixedOffset {
    FixedOffset::east_opt(9 * 3600).expect("JST")
}

pub fn latest_url(base: &str) -> String {
    format!(
        "{}/webservice/server/pros/latest.json",
        base.trim_end_matches('/')
    )
}

/// `latest.json` の `latest_time` (JST の文字列) を読む。
pub fn parse_latest(body: &str) -> Result<DateTime<Utc>, String> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let s = v
        .get("latest_time")
        .and_then(|s| s.as_str())
        .ok_or("latest_time が無い")?;
    let naive = NaiveDateTime::parse_from_str(s, "%Y/%m/%d %H:%M:%S").map_err(|e| e.to_string())?;
    jst()
        .from_local_datetime(&naive)
        .single()
        .map(|t| t.with_timezone(&Utc))
        .ok_or_else(|| "時刻が決まらない".to_string())
}

/// どの時刻の画像をいつ取りに行くかを決める。時計も通信も持たない。
///
/// `lag_ms` は「画像の時刻」から「パソコンの時計でその画像が取れるようになるまで」の遅れ。
/// 時計のずれもここに含まれる。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timing {
    lag_ms: i64,
    /// 最後に 404 になった遅れ。これに近い間は、ゆっくり減らす
    last_404_ms: Option<i64>,
    /// 続けて取れた回数
    streak: u32,
}

impl Timing {
    pub fn new(delay_ms: i64) -> Self {
        Self {
            lag_ms: delay_ms.clamp(DELAY_MIN_MS, DELAY_MAX_MS),
            last_404_ms: None,
            streak: 0,
        }
    }

    pub fn lag_ms(&self) -> i64 {
        self.lag_ms
    }

    fn lag(&self) -> chrono::Duration {
        chrono::Duration::milliseconds(self.lag_ms)
    }

    /// 今取れるはずの最新の画像の時刻 (秒に切り捨て)。
    pub fn target(&self, now: DateTime<Utc>) -> DateTime<Utc> {
        let t = now - self.lag();
        DateTime::from_timestamp(t.timestamp(), 0).expect("範囲内の時刻")
    }

    /// 時刻 `target` の画像が取れるようになる (パソコンの時計での) 時刻。
    pub fn due(&self, target: DateTime<Utc>) -> DateTime<Utc> {
        target + self.lag()
    }

    /// 画像がまだ公開されていなかった。次からは遅れを増やす。
    pub fn on_not_found(&mut self) {
        self.last_404_ms = Some(self.lag_ms);
        self.lag_ms = (self.lag_ms + LAG_STEP_MS).min(LAG_MAX_MS);
        self.streak = 0;
    }

    /// 画像が取れた。取れ続けたら遅れを減らす。
    ///
    /// 最後に 404 になった遅れより十分長ければ早く減らし (一時的な遅れからすぐ戻る)、
    /// 境目の近くでは間をあけて試す (404 で 1 枚抜ける回数を少なくする)。
    pub fn on_success(&mut self) {
        self.streak += 1;
        let far = self
            .last_404_ms
            .is_none_or(|b| self.lag_ms - LAG_STEP_MS > b);
        if (far && self.streak >= FAST_STREAK) || self.streak >= SLOW_STREAK {
            if !far {
                // 境目が変わったかもしれないので、一度忘れて探し直す
                self.last_404_ms = None;
            }
            self.lag_ms = (self.lag_ms - LAG_STEP_MS).max(DELAY_MIN_MS);
            self.streak = 0;
        }
    }
}

#[derive(Debug)]
pub enum FetchError {
    /// 404。その時刻の画像がまだ公開されていない
    NotFound,
    Failed(String),
}

/// 通信の差し替え口 (テスト用)。
#[allow(async_fn_in_trait)]
pub trait Source {
    async fn latest(&self) -> Result<DateTime<Utc>, FetchError>;
    async fn image(&self, kind: ImageKind, at: DateTime<Utc>) -> Result<Vec<u8>, FetchError>;
}

/// 時計の差し替え口 (テスト用)。
pub trait Clock {
    fn now(&self) -> DateTime<Utc>;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

pub struct HttpSource {
    client: reqwest::Client,
    base: String,
}

impl HttpSource {
    pub fn new(base: &str) -> anyhow::Result<Self> {
        super::install_tls_provider();
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .connect_timeout(REQUEST_TIMEOUT)
            .pool_max_idle_per_host(1)
            .user_agent("rsquake")
            .build()?;
        Ok(Self {
            client,
            base: base.to_string(),
        })
    }

    async fn get(&self, url: &str) -> Result<reqwest::Response, FetchError> {
        let res = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| FetchError::Failed(e.to_string()))?;
        match res.status() {
            s if s.is_success() => Ok(res),
            reqwest::StatusCode::NOT_FOUND => Err(FetchError::NotFound),
            s => Err(FetchError::Failed(format!("HTTP {s}"))),
        }
    }
}

impl Source for HttpSource {
    async fn latest(&self) -> Result<DateTime<Utc>, FetchError> {
        let body = self
            .get(&latest_url(&self.base))
            .await?
            .text()
            .await
            .map_err(|e| FetchError::Failed(e.to_string()))?;
        parse_latest(&body).map_err(FetchError::Failed)
    }

    async fn image(&self, kind: ImageKind, at: DateTime<Utc>) -> Result<Vec<u8>, FetchError> {
        let bytes = self
            .get(&kind.url(&self.base, at))
            .await?
            .bytes()
            .await
            .map_err(|e| FetchError::Failed(e.to_string()))?;
        Ok(bytes.to_vec())
    }
}

/// 1 枚の画像から読み取った観測点の色。
/// `colors` は `StationList::stations` と同じ並び。
#[derive(Debug)]
pub struct KmoniFrame {
    /// 画像の時刻
    pub time: DateTime<Utc>,
    pub colors: Vec<Option<Color32>>,
}

#[derive(Debug)]
pub enum KmoniEvent {
    Frame(KmoniFrame),
    /// 通信か画像の読み取りに失敗した。前の観測点はそのまま残す
    Failed(String),
}

/// GIF を読み、各観測点の `center_point` の画素の色を取る。
/// 画像に載らない観測点、休止中の観測点、透明な画素は `None`。
pub fn decode_frame(bytes: &[u8], stations: &StationList) -> anyhow::Result<Vec<Option<Color32>>> {
    let mut opts = gif::DecodeOptions::new();
    opts.set_color_output(gif::ColorOutput::Indexed);
    let mut dec = opts.read_info(Cursor::new(bytes))?;
    let global: Option<Vec<u8>> = dec.global_palette().map(<[u8]>::to_vec);
    let frame = dec
        .read_next_frame()?
        .ok_or_else(|| anyhow::anyhow!("フレームが無い"))?;
    let palette = frame
        .palette
        .as_deref()
        .or(global.as_deref())
        .ok_or_else(|| anyhow::anyhow!("パレットが無い"))?;
    let (left, top) = (frame.left as i32, frame.top as i32);
    let (fw, fh) = (frame.width as i32, frame.height as i32);

    let mut colors = Vec::with_capacity(stations.stations.len());
    for s in &stations.stations {
        let color = (|| {
            if s.suspended {
                return None;
            }
            let (x, y) = s.pixel?;
            let (fx, fy) = (x - left, y - top);
            if fx < 0 || fy < 0 || fx >= fw || fy >= fh {
                return None;
            }
            let index = *frame.buffer.get((fy * fw + fx) as usize)?;
            if frame.transparent == Some(index) {
                return None;
            }
            let rgb = palette.get(index as usize * 3..index as usize * 3 + 3)?;
            Some(Color32::from_rgb(rgb[0], rgb[1], rgb[2]))
        })();
        colors.push(color);
    }
    Ok(colors)
}

/// 取得ループ。`emit` が `false` を返したら (受け手がいなくなったら) 終わる。
pub async fn run<S: Source, C: Clock>(
    source: S,
    clock: C,
    kind: ImageKind,
    stations: Arc<StationList>,
    mut emit: impl FnMut(KmoniEvent) -> bool,
) {
    // 起動時の 1 回だけ latest.json を読み、公開遅延の初期値にする。
    // この要求も 1 秒 1 回の枠に入れるため、画像の要求は 1 秒あけて始める。
    let started = Instant::now();
    let delay_ms = match timeout(REQUEST_TIMEOUT, source.latest()).await {
        Ok(Ok(latest)) => (clock.now() - latest).num_milliseconds(),
        Ok(Err(e)) => {
            tracing::warn!("強震モニタの最新時刻を取れない: {e:?}");
            DELAY_DEFAULT_MS
        }
        Err(_) => {
            tracing::warn!("強震モニタの最新時刻を取れない: タイムアウト");
            DELAY_DEFAULT_MS
        }
    };
    let mut timing = Timing::new(delay_ms);
    tracing::info!("強震モニタ: 公開遅延 {} ms", timing.lag_ms());

    let mut not_before = started + REQUEST_INTERVAL;
    let mut last: Option<DateTime<Utc>> = None;
    loop {
        // 次の画像が公開される瞬間まで待つ。ただし要求の間は 1 秒以上あける
        let now = clock.now();
        let avail = timing.target(now);
        let second = chrono::Duration::seconds(1);
        let target = last.map_or(avail, |l| (l + second).max(avail));
        // 時計が戻ったときなどに、遠い先を待たない
        let target = target.min(avail + second);
        let wait = (timing.due(target) - now).to_std().unwrap_or_default();
        sleep_until((Instant::now() + wait).max(not_before)).await;
        let begun = Instant::now();
        not_before = begun + REQUEST_INTERVAL;

        // 間隔をあけるために待ったなら、より新しい画像が取れるかもしれない
        let target = target.max(timing.target(clock.now()));
        last = Some(target);
        let event = match timeout(REQUEST_TIMEOUT, source.image(kind, target)).await {
            Ok(Ok(bytes)) => match decode_frame(&bytes, &stations) {
                Ok(colors) => {
                    timing.on_success();
                    Some(KmoniEvent::Frame(KmoniFrame {
                        time: target,
                        colors,
                    }))
                }
                Err(e) => Some(KmoniEvent::Failed(format!("画像を読めない: {e}"))),
            },
            Ok(Err(FetchError::NotFound)) => {
                timing.on_not_found();
                // まだ無いだけなので、次も同じ時刻 (か、それより新しいもの) を取りに行く
                last = Some(target - chrono::Duration::seconds(1));
                tracing::debug!("強震モニタ 404: 遅れ {} ms", timing.lag_ms());
                None
            }
            Ok(Err(FetchError::Failed(e))) => Some(KmoniEvent::Failed(e)),
            Err(_) => Some(KmoniEvent::Failed("タイムアウト".into())),
        };
        if let Some(event) = event
            && !emit(event)
        {
            return;
        }
    }
}

pub struct KmoniOptions {
    pub base_url: String,
    pub kind: ImageKind,
    pub stations: Arc<StationList>,
    /// 取得した GIF を `yyyyMMddHHmmss.jma_s.gif` で保存するフォルダ (開発用)
    pub record_dir: Option<std::path::PathBuf>,
}

impl KmoniOptions {
    pub fn new(stations: Arc<StationList>) -> Self {
        Self {
            base_url: BASE_URL.into(),
            kind: ImageKind::default(),
            stations,
            record_dir: None,
        }
    }
}

/// 取得した GIF を JST の時刻の名前で保存する。失敗しても取得は止めない。
struct Recorder<S> {
    inner: S,
    dir: std::path::PathBuf,
}

impl<S: Source> Source for Recorder<S> {
    async fn latest(&self) -> Result<DateTime<Utc>, FetchError> {
        self.inner.latest().await
    }

    async fn image(&self, kind: ImageKind, at: DateTime<Utc>) -> Result<Vec<u8>, FetchError> {
        let bytes = self.inner.image(kind, at).await?;
        let name = format!(
            "{}.{}.gif",
            at.with_timezone(&jst()).format("%Y%m%d%H%M%S"),
            kind.slug()
        );
        if let Err(e) = std::fs::write(self.dir.join(name), &bytes) {
            tracing::warn!("画像を保存できない: {e}");
        }
        Ok(bytes)
    }
}

/// 保存した GIF を時刻順に 1 秒ごとに流す (開発用)。`--kmoni-replay <dir>`。
/// ファイル名の時刻を画像の時刻にする。取得ループの `Source` と `Clock` を差し替えて動かす。
pub struct ReplaySource {
    /// (画像の時刻, ファイルの中身) を時刻順に
    frames: Vec<(DateTime<Utc>, Vec<u8>)>,
}

impl ReplaySource {
    pub fn load(dir: &std::path::Path) -> anyhow::Result<Self> {
        let mut frames = Vec::new();
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            let Some(stem) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Some(time) = stem.split('.').next().and_then(parse_file_time) else {
                continue;
            };
            if !stem.ends_with(".gif") {
                continue;
            }
            frames.push((time, std::fs::read(&path)?));
        }
        frames.sort_by_key(|f| f.0);
        anyhow::ensure!(!frames.is_empty(), "{} に画像が無い", dir.display());
        Ok(Self { frames })
    }

    pub fn first_time(&self) -> DateTime<Utc> {
        self.frames[0].0
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }
}

fn parse_file_time(s: &str) -> Option<DateTime<Utc>> {
    let naive = NaiveDateTime::parse_from_str(s, "%Y%m%d%H%M%S").ok()?;
    jst()
        .from_local_datetime(&naive)
        .single()
        .map(|t| t.with_timezone(&Utc))
}

impl Source for ReplaySource {
    async fn latest(&self) -> Result<DateTime<Utc>, FetchError> {
        // 時計は最初の画像の 2 秒後から始まるので、最新の画像は最初の画像 (公開遅延 2 秒)
        Ok(self.first_time())
    }

    async fn image(&self, _: ImageKind, at: DateTime<Utc>) -> Result<Vec<u8>, FetchError> {
        // 無い時刻は 404 と同じ扱い (取得ループが補正量を増やして待つ)
        match self.frames.binary_search_by_key(&at, |f| f.0) {
            Ok(i) => Ok(self.frames[i].1.clone()),
            Err(_) => Err(FetchError::NotFound),
        }
    }
}

/// 起動からの経過で進む時計。`start` を最初の画像の 2 秒後に合わせる。
pub struct ReplayClock {
    start: DateTime<Utc>,
    began: std::time::Instant,
}

impl ReplayClock {
    pub fn new(start: DateTime<Utc>) -> Self {
        Self {
            start,
            began: std::time::Instant::now(),
        }
    }
}

impl Clock for ReplayClock {
    fn now(&self) -> DateTime<Utc> {
        self.start + chrono::Duration::from_std(self.began.elapsed()).unwrap_or_default()
    }
}

/// 保存した画像の再生。`dir` の画像を時刻順に 1 秒ごとに流す。
pub fn spawn_replay(
    dir: std::path::PathBuf,
    stations: Arc<StationList>,
    tx: Sender<NetEvent>,
    ctx: egui::Context,
) {
    std::thread::Builder::new()
        .name("kmoni-replay".into())
        .spawn(move || {
            let source = match ReplaySource::load(&dir) {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!("画像のリプレイを始められない: {e}");
                    return;
                }
            };
            tracing::info!(
                "強震モニタのリプレイ: {} 枚 ({})",
                source.len(),
                dir.display()
            );
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            let clock = ReplayClock::new(
                source.first_time() + chrono::Duration::milliseconds(DELAY_DEFAULT_MS),
            );
            rt.block_on(run(
                source,
                clock,
                ImageKind::default(),
                stations,
                move |e| {
                    let ok = tx.send(NetEvent::Kmoni(e)).is_ok();
                    ctx.request_repaint();
                    ok
                },
            ));
        })
        .expect("kmoni replay thread");
}

pub fn spawn(opts: KmoniOptions, tx: Sender<NetEvent>, ctx: egui::Context) {
    std::thread::Builder::new()
        .name("kmoni".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            let source = match HttpSource::new(&opts.base_url) {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!("強震モニタの接続を作れない: {e}");
                    return;
                }
            };
            let emit = move |e| {
                let ok = tx.send(NetEvent::Kmoni(e)).is_ok();
                ctx.request_repaint();
                ok
            };
            match opts.record_dir {
                Some(dir) => {
                    if let Err(e) = std::fs::create_dir_all(&dir) {
                        tracing::error!("{} を作れない: {e}", dir.display());
                        return;
                    }
                    tracing::info!("強震モニタの画像を {} に保存する", dir.display());
                    rt.block_on(run(
                        Recorder { inner: source, dir },
                        SystemClock,
                        opts.kind,
                        opts.stations,
                        emit,
                    ));
                }
                None => rt.block_on(run(source, SystemClock, opts.kind, opts.stations, emit)),
            }
        })
        .expect("kmoni thread");
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use chrono::TimeZone;

    use super::*;

    /// 実際の強震モニタの画像。暫定措置: `tests/fixtures` は公開リポジトリに無いので、
    /// 無ければ `None` にして、これを要るテストを飛ばす。
    fn real_gif() -> Option<Vec<u8>> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/kmoni/20261114200133.jma_s.gif");
        let gif = std::fs::read(&path).ok();
        if gif.is_none() {
            eprintln!("skip: {} が無い", path.display());
        }
        gif
    }

    /// 取得ループのテスト用の 1x1 の GIF。どの観測点も画像の外なので、色は全部 `None` になる。
    const DUMMY_IMAGE: &[u8] = &[
        0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00,
        0x00, 0xFF, 0xFF, 0xFF, 0x21, 0xF9, 0x04, 0x01, 0x00, 0x00, 0x00, 0x00, 0x2C, 0x00, 0x00,
        0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x02, 0x02, 0x44, 0x01, 0x00, 0x3B,
    ];

    fn utc(h: u32, m: u32, s: u32, ms: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 14, h, m, s).unwrap()
            + chrono::Duration::milliseconds(ms as i64)
    }

    #[test]
    fn url_is_built_in_jst() {
        // 2026-11-14 11:01:34 UTC は JST の 20:01:34
        assert_eq!(
            ImageKind::RealtimeShindo.url("http://example.test/", utc(11, 1, 34, 0)),
            "http://example.test/data/map_img/RealTimeImg/jma_s/20261114/20261114200134.jma_s.gif"
        );
        // 日付をまたぐ (UTC 15:00 は JST の翌日 0 時)
        assert!(
            ImageKind::RealtimeShindo
                .url("http://x", utc(15, 0, 0, 0))
                .ends_with("/20261115/20261115000000.jma_s.gif")
        );
    }

    #[test]
    fn latest_json_is_read_as_jst() {
        let body =
            r#"{"latest_time": "2026/11/14 20:01:36", "request_time": "2026/11/14 20:01:37"}"#;
        assert_eq!(parse_latest(body).unwrap(), utc(11, 1, 36, 0));
        assert!(parse_latest("{}").is_err());
        assert!(parse_latest("x").is_err());
    }

    #[test]
    fn delay_is_clamped() {
        assert_eq!(Timing::new(0).lag_ms(), 200);
        assert_eq!(Timing::new(-3_000).lag_ms(), 200);
        assert_eq!(Timing::new(1_500).lag_ms(), 1_500);
        assert_eq!(Timing::new(60_000).lag_ms(), 5_000);
    }

    #[test]
    fn lag_grows_by_100ms_on_404_and_stops_at_10s() {
        let mut t = Timing::new(1_000);
        t.on_not_found();
        assert_eq!(t.lag_ms(), 1_100);
        for _ in 0..200 {
            t.on_not_found();
        }
        assert_eq!(t.lag_ms(), 10_000);
    }

    #[test]
    fn lag_shrinks_after_successes() {
        // 404 から十分離れていれば 5 回ごとに減る
        let mut t = Timing::new(3_000);
        for _ in 0..5 {
            t.on_success();
        }
        assert_eq!(t.lag_ms(), 2_900);
        // 404 の境目のすぐ上では 60 回取れるまで減らさない
        t.on_not_found();
        assert_eq!(t.lag_ms(), 3_000);
        for _ in 0..59 {
            t.on_success();
        }
        assert_eq!(t.lag_ms(), 3_000);
        t.on_success();
        assert_eq!(t.lag_ms(), 2_900);
        // 下限は 200 ms
        let mut t = Timing::new(300);
        for _ in 0..100 {
            t.on_success();
        }
        assert_eq!(t.lag_ms(), 200);
    }

    #[test]
    fn target_and_due_line_up_on_the_second() {
        let t = Timing::new(1_000);
        let now = utc(11, 0, 10, 300);
        assert_eq!(t.target(now), utc(11, 0, 9, 0));
        // 09 秒の画像は 10.000 秒に取れるようになる
        assert_eq!(t.due(utc(11, 0, 9, 0)), utc(11, 0, 10, 0));
    }

    #[test]
    fn decodes_station_colors_from_a_real_image() {
        let Some(gif) = real_gif() else {
            return;
        };
        let stations = StationList::bundled();
        let colors = decode_frame(&gif, &stations).unwrap();
        assert_eq!(colors.len(), stations.stations.len());
        let n = colors.iter().flatten().count();
        // 画素位置を持つのは 1,634 地点。透明な画素と休止中の観測点は値なし
        assert!((1_500..=1_634).contains(&n), "{n}");
        for (s, c) in stations.stations.iter().zip(&colors) {
            if s.pixel.is_none() || s.suspended {
                assert!(c.is_none(), "{}", s.code);
            }
        }
    }

    #[test]
    fn broken_image_is_an_error() {
        assert!(decode_frame(b"not a gif", &StationList::bundled()).is_err());
    }

    #[test]
    fn file_names_are_read_as_jst() {
        assert_eq!(parse_file_time("20261114200134"), Some(utc(11, 1, 34, 0)));
        assert_eq!(parse_file_time("x"), None);
    }

    #[test]
    fn replay_source_loads_gifs_in_time_order() {
        let dir = std::env::temp_dir().join(format!("rsquake-replay-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in [
            "20261114200102.jma_s.gif",
            "20261114200100.jma_s.gif",
            "notes.txt",
            "bad.jma_s.gif",
        ] {
            std::fs::write(dir.join(name), DUMMY_IMAGE).unwrap();
        }
        let src = ReplaySource::load(&dir).unwrap();
        assert_eq!(src.len(), 2, "時刻の読めないファイルと GIF 以外は飛ばす");
        assert_eq!(src.first_time(), utc(11, 1, 0, 0));
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        rt.block_on(async {
            assert!(
                src.image(ImageKind::RealtimeShindo, utc(11, 1, 2, 0))
                    .await
                    .is_ok()
            );
            assert!(matches!(
                src.image(ImageKind::RealtimeShindo, utc(11, 1, 1, 0)).await,
                Err(FetchError::NotFound)
            ));
        });
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn empty_replay_dir_is_an_error() {
        let dir = std::env::temp_dir().join(format!("rsquake-replay-empty-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(ReplaySource::load(&dir).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    // ---- 取得ループ (仮想時計) ----

    /// tokio の仮想時計に連動する時計。
    struct TestClock {
        base: DateTime<Utc>,
        start: Instant,
    }

    impl TestClock {
        fn new(base: DateTime<Utc>) -> Self {
            Self {
                base,
                start: Instant::now(),
            }
        }
    }

    impl Clock for TestClock {
        fn now(&self) -> DateTime<Utc> {
            self.base + chrono::Duration::from_std(Instant::now() - self.start).unwrap()
        }
    }

    #[derive(Default)]
    struct Log {
        /// (要求を出した仮想時刻, 要求した画像の時刻)
        requests: Vec<(Instant, DateTime<Utc>)>,
        in_flight: usize,
        max_in_flight: usize,
    }

    /// (何回目の要求か, 要求した画像の時刻, 応答時点の現在時刻)
    type Reply = Box<dyn Fn(usize, DateTime<Utc>, DateTime<Utc>) -> Result<(), FetchError>>;

    struct FakeSource {
        latest: Option<DateTime<Utc>>,
        latency: Duration,
        reply: Reply,
        log: Rc<RefCell<Log>>,
        clock: TestClock,
    }

    /// 中断 (タイムアウト) されても `in_flight` を戻す。
    struct InFlight(Rc<RefCell<Log>>);

    impl Drop for InFlight {
        fn drop(&mut self) {
            self.0.borrow_mut().in_flight -= 1;
        }
    }

    impl Source for FakeSource {
        async fn latest(&self) -> Result<DateTime<Utc>, FetchError> {
            self.latest.ok_or(FetchError::Failed("no latest".into()))
        }

        async fn image(&self, _: ImageKind, at: DateTime<Utc>) -> Result<Vec<u8>, FetchError> {
            let n = {
                let mut l = self.log.borrow_mut();
                l.requests.push((Instant::now(), at));
                l.in_flight += 1;
                l.max_in_flight = l.max_in_flight.max(l.in_flight);
                l.requests.len() - 1
            };
            let _guard = InFlight(self.log.clone());
            tokio::time::sleep(self.latency).await;
            (self.reply)(n, at, self.clock.now()).map(|()| DUMMY_IMAGE.to_vec())
        }
    }

    /// `secs` 秒 (仮想時間) だけ回し、出たイベントと要求の記録を返す。
    async fn drive(source: FakeSource, secs: u64) -> (Vec<String>, Rc<RefCell<Log>>) {
        let log = source.log.clone();
        let events = Rc::new(RefCell::new(Vec::new()));
        let sink = events.clone();
        let t0 = Instant::now();
        let fut = run(
            source,
            TestClock::new(utc(11, 0, 0, 0)),
            ImageKind::RealtimeShindo,
            Arc::new(StationList::bundled()),
            move |e| {
                let at = Instant::now() - t0;
                sink.borrow_mut().push(match e {
                    KmoniEvent::Frame(f) => {
                        format!("{:?} frame {}", at.as_secs(), f.time.format("%H:%M:%S"))
                    }
                    KmoniEvent::Failed(m) => format!("{:?} failed {m}", at.as_secs()),
                });
                true
            },
        );
        let _ = timeout(Duration::from_secs(secs), fut).await;
        let out = events.borrow().clone();
        (out, log)
    }

    fn ok_source(latency: Duration, log: &Rc<RefCell<Log>>) -> FakeSource {
        FakeSource {
            latest: Some(utc(10, 59, 59, 0)),
            latency,
            reply: Box::new(|_, _, _| Ok(())),
            log: log.clone(),
            clock: TestClock::new(utc(11, 0, 0, 0)),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn requests_come_once_per_second_after_the_latest_lookup() {
        let log = Rc::new(RefCell::new(Log::default()));
        let (events, log) = drive(ok_source(Duration::from_millis(50), &log), 10).await;
        let l = log.borrow();
        // latest.json に 1 秒使うので、画像の最初の要求は 1 秒後。10 秒で 9〜10 回
        assert!((9..=10).contains(&l.requests.len()), "{}", l.requests.len());
        for w in l.requests.windows(2) {
            assert!(
                w[1].0 - w[0].0 >= Duration::from_secs(1),
                "{:?}",
                w[1].0 - w[0].0
            );
        }
        // 打ち切りの瞬間に出た要求だけは、結果が間に合わない
        assert!((l.requests.len() - 1..=l.requests.len()).contains(&events.len()));
    }

    #[tokio::test(start_paused = true)]
    async fn slow_response_skips_the_next_turn_and_never_overlaps() {
        let log = Rc::new(RefCell::new(Log::default()));
        let (_, log) = drive(ok_source(Duration::from_millis(1_500), &log), 20).await;
        let l = log.borrow();
        assert_eq!(l.max_in_flight, 1);
        assert!(l.requests.len() >= 5);
        for w in l.requests.windows(2) {
            assert!(w[1].0 - w[0].0 >= Duration::from_millis(1_500));
        }
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_is_a_failure_and_retrying_continues() {
        let log = Rc::new(RefCell::new(Log::default()));
        // 応答が 10 秒かかる。2 秒で切って失敗にし、そのあとも続ける
        let (events, log) = drive(ok_source(Duration::from_secs(10), &log), 12).await;
        assert!(events.len() >= 3, "{events:?}");
        assert!(
            events.iter().all(|e| e.contains("failed タイムアウト")),
            "{events:?}"
        );
        let l = log.borrow();
        assert_eq!(l.max_in_flight, 1);
        for w in l.requests.windows(2) {
            assert!(w[1].0 - w[0].0 >= Duration::from_secs(2));
        }
    }

    #[tokio::test(start_paused = true)]
    async fn failures_do_not_stop_the_loop() {
        let log = Rc::new(RefCell::new(Log::default()));
        let mut src = ok_source(Duration::from_millis(10), &log);
        src.reply = Box::new(|n, _, _| {
            if n < 3 {
                Err(FetchError::Failed("down".into()))
            } else {
                Ok(())
            }
        });
        let (events, _) = drive(src, 8).await;
        assert!(events[0].contains("failed down"), "{events:?}");
        assert!(events[1].contains("failed down") && events[2].contains("failed down"));
        assert!(events[3].contains("frame"), "{events:?}");
        assert!(events.len() >= 6);
    }

    #[tokio::test(start_paused = true)]
    async fn not_found_steps_back_until_the_image_exists() {
        let log = Rc::new(RefCell::new(Log::default()));
        // latest.json が無く公開遅延は既定の 2 秒。実際には 4 秒前の画像までしか公開されていない。
        let mut src = ok_source(Duration::from_millis(10), &log);
        src.latest = None;
        src.reply = Box::new(|_, at, now| {
            if at <= now - chrono::Duration::seconds(4) {
                Ok(())
            } else {
                Err(FetchError::NotFound)
            }
        });
        let (events, log) = drive(src, 60).await;
        let l = log.borrow();
        let first = events.first().expect("いつかは取れる");
        assert!(first.contains("frame"), "{first}");
        // 遅れが約 4 秒に届くまでは 404 が続き、その間は何も出さない。
        // 取れ始めたあとは取れ続け、遅れはそれ以上増えない。
        let n404 = l.requests.len() - events.len();
        assert!((8..=22).contains(&n404), "404 の回数 {n404}");
        assert!(events.iter().all(|e| e.contains("frame")), "{events:?}");
    }
}
