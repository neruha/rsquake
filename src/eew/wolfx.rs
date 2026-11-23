//! Wolfx Open API (`jma_eew`) の受信と、`EewReport` への変換。
//!
//! 仕様と実データの調査は `docs/plans/wolfx-plan.md` の 1 章。

use std::collections::HashMap;

use chrono::{DateTime, Duration, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use serde::Deserialize;
use serde_json::Value;

use super::report::{
    EewReport, Forecast, HypocenterKind, Incoming, IntensityForecast, Quake, RegionForecast,
    Shindo, TelegramStatus, Upper, filter_status,
};

/// Wolfx が `大東島` / `宮古島` と書く場合に備えた別名。地図の name は県名つき。
pub const REGION_ALIASES: [(&str, &str); 2] = [("大東島", "803"), ("宮古島", "804")];

/// 接続直後の `query_jmaeew` の応答など、この時間より古い `AnnouncedTime` の報は捨てる。
pub const STALE_AFTER: Duration = Duration::minutes(3);

/// `WarnArea` の `Chiiki` (名前) から細分区域コードを引く表。
pub type RegionCodes = HashMap<String, String>;

/// 地図の (code, name) の組から表を作り、別名を足す。code が無い地物は飛ばす。
pub fn region_codes<'a>(
    features: impl IntoIterator<Item = (Option<&'a str>, &'a str)>,
) -> RegionCodes {
    let mut map: RegionCodes = features
        .into_iter()
        .filter_map(|(code, name)| Some((name.to_string(), code?.to_string())))
        .collect();
    for (alias, code) in REGION_ALIASES {
        map.entry(alias.to_string())
            .or_insert_with(|| code.to_string());
    }
    map
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct WolfxEew {
    #[serde(default)]
    pub code_type: Option<String>,
    #[serde(default)]
    pub issue: Option<Issue>,
    #[serde(rename = "EventID")]
    pub event_id: String,
    pub serial: u32,
    pub announced_time: String,
    #[serde(default)]
    pub origin_time: Option<String>,
    #[serde(default)]
    pub hypocenter: Option<String>,
    #[serde(default)]
    pub latitude: Option<f64>,
    #[serde(default)]
    pub longitude: Option<f64>,
    #[serde(default)]
    pub magnitude: Option<f64>,
    /// 古い形は `Magnitude` が無く、綴りの違うこちらだけ
    #[serde(rename = "Magunitude", default)]
    pub magunitude: Option<f64>,
    #[serde(default)]
    pub depth: Option<f64>,
    #[serde(default)]
    pub max_intensity: Option<String>,
    #[serde(default)]
    pub accuracy: Option<Accuracy>,
    #[serde(default)]
    pub warn_area: Vec<WarnArea>,
    #[serde(rename = "isTraining", default)]
    pub is_training: bool,
    #[serde(rename = "isAssumption", default)]
    pub is_assumption: bool,
    #[serde(rename = "isWarn", default)]
    pub is_warn: bool,
    #[serde(rename = "isFinal", default)]
    pub is_final: bool,
    #[serde(rename = "isCancel", default)]
    pub is_cancel: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Issue {
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Accuracy {
    #[serde(default)]
    pub epicenter: Option<String>,
    #[serde(default)]
    pub depth: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct WarnArea {
    pub chiiki: String,
    #[serde(default)]
    pub shindo1: String,
    #[serde(default)]
    pub shindo2: String,
    #[serde(default)]
    pub time: String,
    #[serde(rename = "Type", default)]
    pub kind: String,
    /// 実データでは文字列。型が違っても落ちないよう `Value` で受ける
    #[serde(default)]
    pub arrive: Value,
}

/// WebSocket の 1 メッセージを解釈する。
///
/// `heartbeat` / `pong` は無視。`type` が `jma_eew`、または `type` が無く `EventID` がある
/// (HTTP GET の形) ものを報として読む。
pub fn parse_message(text: &str, accept_non_normal: bool, codes: &RegionCodes) -> Incoming {
    let v: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => return Incoming::Ignored(format!("JSON として読めない: {e}")),
    };
    match v.get("type").and_then(Value::as_str) {
        Some("jma_eew") => {}
        None if v.get("EventID").is_some() => {}
        other => return Incoming::Ignored(format!("type {other:?} は対象外")),
    }
    let w: WolfxEew = match serde_json::from_value(v) {
        Ok(w) => w,
        Err(e) => return Incoming::Ignored(format!("電文を読めない: {e}")),
    };
    match EewReport::from_wolfx(w, codes) {
        Ok(r) => filter_status(r, accept_non_normal),
        Err(e) => Incoming::Ignored(e),
    }
}

/// 報の `AnnouncedTime` が `now` から見て古すぎるか。
pub fn is_stale(report: &EewReport, now: DateTime<Utc>) -> bool {
    now - report.report_time > STALE_AFTER
}

fn jst() -> FixedOffset {
    FixedOffset::east_opt(9 * 3600).expect("JST")
}

/// `YYYY/MM/DD HH:mm:ss` (JST) を UTC にする。
fn parse_time(s: &str) -> Option<DateTime<Utc>> {
    let naive = NaiveDateTime::parse_from_str(s.trim(), "%Y/%m/%d %H:%M:%S").ok()?;
    jst()
        .from_local_datetime(&naive)
        .single()
        .map(|t| t.with_timezone(&Utc))
}

/// 到達予測時刻 `HHMMSS` (JST)。日付は `announced` から取り、報より前になるときは翌日にする。
fn parse_arrival(hhmmss: &str, announced: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let s = hhmmss.trim();
    if s.len() != 6 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let time = NaiveTime::from_hms_opt(
        s[0..2].parse().ok()?,
        s[2..4].parse().ok()?,
        s[4..6].parse().ok()?,
    )?;
    let date: NaiveDate = announced.with_timezone(&jst()).date_naive();
    let at = |d: NaiveDate| {
        jst()
            .from_local_datetime(&d.and_time(time))
            .single()
            .map(|t| t.with_timezone(&Utc))
    };
    let t = at(date)?;
    // 報の時刻より 12 時間以上前なら日付をまたいだとみなす
    if announced - t > Duration::hours(12) {
        at(date.succ_opt()?)
    } else {
        Some(t)
    }
}

fn is_level_method(code_type: Option<&str>) -> bool {
    code_type.is_some_and(|c| c.contains("最大予測震度のみ"))
}

/// `Arrive` の文字列から (到達済み, PLUM)。知らない値は (false, false)。
fn classify_arrive(v: &Value) -> (bool, bool) {
    match v.as_str() {
        Some(s) if s.starts_with("既に到達") => (true, false),
        Some(s) if s.contains("PLUM") => (false, true),
        Some("未到達") => (false, false),
        other => {
            tracing::debug!("知らない Arrive: {other:?} / {v}");
            (false, false)
        }
    }
}

fn region_forecast(a: WarnArea, announced: DateTime<Utc>, codes: &RegionCodes) -> RegionForecast {
    let (reached, is_plum) = classify_arrive(&a.arrive);
    let code = match codes.get(&a.chiiki) {
        Some(c) => c.clone(),
        None => {
            tracing::warn!("未定義の区域名: {}", a.chiiki);
            String::new()
        }
    };
    // `不明` は DMDATA の from/to "0" に当たる。最大が不明なら to は Unknown
    let to = match Shindo::parse_ja(&a.shindo1) {
        Some(s) => Upper::Value(s),
        None => Upper::Unknown,
    };
    let arrival_time = if reached || is_plum {
        None
    } else {
        parse_arrival(&a.time, announced)
    };
    RegionForecast {
        code,
        name: a.chiiki,
        max_int: Forecast {
            from: Shindo::parse_ja(&a.shindo2),
            to,
        },
        max_lg: None,
        is_warning: a.kind == "警報",
        is_plum,
        reached,
        arrival_time,
    }
}

impl EewReport {
    pub fn from_wolfx(mut w: WolfxEew, codes: &RegionCodes) -> Result<Self, String> {
        let report_time = parse_time(&w.announced_time)
            .ok_or_else(|| format!("AnnouncedTime を読めない: {}", w.announced_time))?;
        let status = if w.is_training {
            TelegramStatus::Training
        } else if w.issue.as_ref().and_then(|i| i.status.as_deref()) == Some("試験") {
            TelegramStatus::Test
        } else {
            TelegramStatus::Normal
        };
        // 取消は震源も区域も読まない (取消電文の 0 や空文字を震源として扱わない)
        let (quake, intensity) = if w.is_cancel {
            (None, None)
        } else {
            let areas = std::mem::take(&mut w.warn_area);
            (
                Some(convert_quake(&w)),
                Some(convert_intensity(areas, &w, report_time, codes)),
            )
        };
        Ok(Self {
            event_id: w.event_id,
            serial: w.serial,
            status,
            report_time,
            is_cancel: w.is_cancel,
            is_last: w.is_final,
            is_warning: !w.is_cancel && w.is_warn,
            quake,
            intensity,
        })
    }
}
fn convert_quake(w: &WolfxEew) -> Quake {
    let level = is_level_method(w.code_type.as_deref());
    let epicenter = w
        .accuracy
        .as_ref()
        .and_then(|a| a.epicenter.as_deref())
        .unwrap_or_default();
    let kind = if w.is_assumption {
        HypocenterKind::Assumed
    } else if level || epicenter.starts_with("P 波") || epicenter.starts_with("不明") {
        HypocenterKind::LowAccuracy
    } else {
        HypocenterKind::Normal
    };
    let time = w.origin_time.as_deref().and_then(parse_time);
    // 深さの精度が分かっているのに 0 なら「ごく浅い」。精度が「不明」の 0 は未設定
    let depth_known = w
        .accuracy
        .as_ref()
        .and_then(|a| a.depth.as_deref())
        .is_some_and(|d| !d.trim().is_empty() && !d.starts_with("不明"));
    let depth_km = match w.depth {
        _ if level || kind == HypocenterKind::Assumed => None,
        Some(d) if d > 0.0 => Some(d),
        Some(d) if d == 0.0 && depth_known => Some(0.0),
        _ => None,
    };
    let magnitude = match w.magnitude.or(w.magunitude) {
        Some(m) if m > 0.0 && kind != HypocenterKind::Assumed => Some(m),
        _ => None,
    };
    // レベル法の OriginTime は検知時刻。DMDATA と同じく arrival_time 側に入れる
    let (origin_time, arrival_time) = if level { (None, time) } else { (time, None) };
    Quake {
        origin_time,
        arrival_time,
        kind,
        name: w.hypocenter.clone().unwrap_or_default(),
        latitude: w.latitude,
        longitude: w.longitude,
        depth_km,
        depth_over_700: depth_km.is_some_and(|d| d >= 700.0),
        magnitude,
        magnitude_over_8: false,
    }
}

fn convert_intensity(
    areas: Vec<WarnArea>,
    w: &WolfxEew,
    announced: DateTime<Utc>,
    codes: &RegionCodes,
) -> IntensityForecast {
    // 最大予測震度は DMDATA と同じく from == to。`不明` は from なし・to 不明
    let max_int = match w.max_intensity.as_deref().and_then(Shindo::parse_ja) {
        Some(s) => Forecast {
            from: Some(s),
            to: Upper::Value(s),
        },
        None => Forecast {
            from: None,
            to: Upper::Unknown,
        },
    };
    IntensityForecast {
        max_int,
        max_lg: None,
        regions: areas
            .into_iter()
            .map(|a| region_forecast(a, announced, codes))
            .collect(),
    }
}
