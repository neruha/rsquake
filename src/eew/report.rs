//! 受信層: envelope を検証し、表示に使う形 (`EewReport`) に正規化する。

use chrono::{DateTime, Duration, Utc};
use serde_json::Value;

use super::telegram::{self, EewTelegram};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TelegramStatus {
    Normal,
    Training,
    Test,
}

impl TelegramStatus {
    pub fn label(self) -> Option<&'static str> {
        match self {
            Self::Normal => None,
            Self::Training => Some("訓練"),
            Self::Test => Some("試験"),
        }
    }
}

/// 震度階級。内部では 0〜9 の序数で持つ（5弱=5、5強=6、6弱=7、6強=8、7=9）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Shindo(u8);

impl Shindo {
    const LABELS: [&'static str; 10] = ["0", "1", "2", "3", "4", "5弱", "5強", "6弱", "6強", "7"];

    pub fn parse(s: &str) -> Option<Self> {
        let i = match s {
            "0" => 0,
            "1" => 1,
            "2" => 2,
            "3" => 3,
            "4" => 4,
            "5-" => 5,
            "5+" => 6,
            "6-" => 7,
            "6+" => 8,
            "7" => 9,
            _ => return None,
        };
        Some(Self(i))
    }

    pub fn index(self) -> u8 {
        self.0
    }

    pub fn label(self) -> &'static str {
        Self::LABELS[self.0 as usize]
    }

    /// `5弱` などの表記。DMDATA の表記 (`5-`) も受ける。`不明` は `None`。
    pub fn parse_ja(s: &str) -> Option<Self> {
        let s = s.trim();
        Self::LABELS
            .iter()
            .position(|l| *l == s)
            .map(|i| Self(i as u8))
            .or_else(|| Self::parse(s))
    }
}

/// 長周期地震動階級 (0〜4)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LgClass(u8);

impl LgClass {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "0" | "1" | "2" | "3" | "4" => s.parse().ok().map(Self),
            _ => None,
        }
    }

    pub fn index(self) -> u8 {
        self.0
    }

    pub fn label(self) -> &'static str {
        ["0", "1", "2", "3", "4"][self.0 as usize]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Upper<T> {
    Value(T),
    Over,
    Unknown,
}

/// 電文の `from` / `to` をそのまま持つ予測値。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Forecast<T> {
    pub from: Option<T>,
    pub to: Upper<T>,
}

impl<T: Copy + PartialEq> Forecast<T> {
    /// 塗りや並べ替えに使う代表値。`to` が `over` / 不明なら `from`。
    pub fn representative(&self) -> Option<T> {
        match self.to {
            Upper::Value(v) => Some(v),
            Upper::Over | Upper::Unknown => self.from,
        }
    }

    fn parse(f: &telegram::MaxInt, p: fn(&str) -> Option<T>) -> Self {
        let to = match f.to.as_str() {
            "over" => Upper::Over,
            s => p(s).map(Upper::Value).unwrap_or(Upper::Unknown),
        };
        Self {
            from: p(&f.from),
            to,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HypocenterKind {
    /// IPF 法 2 点以上など、通常の震源。
    Normal,
    /// レベル法、IPF 法 1 点。震源の精度が低い。
    LowAccuracy,
    /// 仮定震源要素（PLUM 法のみ）。
    Assumed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Quake {
    pub origin_time: Option<DateTime<Utc>>,
    pub arrival_time: Option<DateTime<Utc>>,
    pub kind: HypocenterKind,
    pub name: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    /// 深さ (km)。「ごく浅い」は 0
    pub depth_km: Option<f64>,
    /// 深さが「700km 以上」
    pub depth_over_700: bool,
    pub magnitude: Option<f64>,
    /// 規模が「M8 を超える巨大地震」(数値は無い)
    pub magnitude_over_8: bool,
}

impl Quake {
    pub fn position(&self) -> Option<(f64, f64)> {
        Some((self.latitude?, self.longitude?))
    }

    /// 時間の基準。発生時刻があればそれ、無ければ検知時刻。
    pub fn base_time(&self) -> Option<DateTime<Utc>> {
        self.origin_time.or(self.arrival_time)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RegionForecast {
    pub code: String,
    pub name: String,
    pub max_int: Forecast<Shindo>,
    pub max_lg: Option<Forecast<LgClass>>,
    pub is_warning: bool,
    pub is_plum: bool,
    pub reached: bool,
    pub arrival_time: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IntensityForecast {
    pub max_int: Forecast<Shindo>,
    pub max_lg: Option<Forecast<LgClass>>,
    pub regions: Vec<RegionForecast>,
}

/// 正規化した 1 報。
#[derive(Clone, Debug, PartialEq)]
pub struct EewReport {
    pub event_id: String,
    pub serial: u32,
    pub status: TelegramStatus,
    pub report_time: DateTime<Utc>,
    pub is_cancel: bool,
    pub is_last: bool,
    pub is_warning: bool,
    pub quake: Option<Quake>,
    pub intensity: Option<IntensityForecast>,
}

impl EewReport {
    pub fn from_telegram(t: EewTelegram) -> Result<Self, String> {
        if let Some(schema) = &t.schema
            && schema.kind != "eew-information"
        {
            return Err(format!("schema {} は対象外", schema.kind));
        }
        let status = match t.status.as_str() {
            "通常" => TelegramStatus::Normal,
            "訓練" => TelegramStatus::Training,
            "試験" => TelegramStatus::Test,
            s => return Err(format!("未知の status: {s}")),
        };
        let serial: u32 = t
            .serial_no
            .trim()
            .parse()
            .map_err(|_| format!("serialNo が数値でない: {}", t.serial_no))?;
        let is_cancel = t.info_type == "取消" || t.body.is_canceled;

        if !is_cancel
            && let Some(title) = t.title_type.as_deref()
            && matches!(title, "リアルタイム震度" | "緊急地震速報配信テスト")
        {
            return Err(format!("{title} は対象外"));
        }

        let quake = if is_cancel {
            None
        } else {
            Some(convert_quake(t.body.earthquake.ok_or("earthquake が無い")?))
        };

        let intensity = if is_cancel {
            None
        } else {
            // 最大予測震度が無くても、細分区域の予想は捨てない
            t.body.intensity.map(|i| IntensityForecast {
                max_int: i
                    .forecast_max_int
                    .as_ref()
                    .map_or(UNKNOWN_SHINDO, |f| Forecast::parse(f, Shindo::parse)),
                max_lg: i
                    .forecast_max_lg_int
                    .as_ref()
                    .map(|f| Forecast::parse(f, LgClass::parse)),
                regions: i.regions.into_iter().map(convert_region).collect(),
            })
        };

        Ok(Self {
            event_id: t.event_id,
            serial,
            status,
            report_time: t.report_date_time,
            is_cancel,
            is_last: t.body.is_last_info,
            is_warning: !is_cancel && t.body.is_warning,
            quake,
            intensity,
        })
    }

    /// テストモードの時刻補正。報に含まれる時刻をすべて `d` だけずらす。
    pub fn shift(&mut self, d: Duration) {
        if d.is_zero() {
            return;
        }
        self.report_time += d;
        if let Some(q) = &mut self.quake {
            q.origin_time = q.origin_time.map(|t| t + d);
            q.arrival_time = q.arrival_time.map(|t| t + d);
        }
        if let Some(i) = &mut self.intensity {
            for r in &mut i.regions {
                r.arrival_time = r.arrival_time.map(|t| t + d);
            }
        }
    }
}

const UNKNOWN_SHINDO: Forecast<Shindo> = Forecast {
    from: None,
    to: Upper::Unknown,
};

fn parse_num(s: Option<&String>) -> Option<f64> {
    s?.trim().parse().ok()
}

/// 深さの `condition` が「ごく浅い」か。
fn is_very_shallow(c: &str) -> bool {
    c.contains("ごく浅い")
}

/// 深さの `condition` が「700km 以上」か (電文は全角で `７００ｋｍ以上`)。
fn is_over_700(c: &str) -> bool {
    let c: String = c.chars().map(to_ascii_narrow).collect();
    c.contains("700km以上")
}

/// 規模の `condition` が「M8 を超える巨大地震」か。
fn is_over_m8(c: &str) -> bool {
    let c: String = c.chars().map(to_ascii_narrow).collect();
    c.contains("M8を超える")
}

/// 全角の英数字を半角にする。
fn to_ascii_narrow(c: char) -> char {
    match c {
        '０'..='９' | 'Ａ'..='Ｚ' | 'ａ'..='ｚ' => {
            char::from_u32(c as u32 - 0xFEE0).unwrap_or(c)
        }
        _ => c,
    }
}

fn convert_quake(e: telegram::Earthquake) -> Quake {
    let h = e.hypocenter;
    let epicenter_accuracy = h
        .accuracy
        .as_ref()
        .and_then(|a| a.epicenters.first())
        .map(String::as_str);
    // DMDATA の説明: 震央精度 1 は「P波/S波レベル越え、IPF法(1点)、または仮定震源要素」
    let kind = if e.condition.as_deref() == Some("仮定震源要素") {
        HypocenterKind::Assumed
    } else if matches!(epicenter_accuracy, Some("0" | "1")) {
        HypocenterKind::LowAccuracy
    } else {
        HypocenterKind::Normal
    };
    let (latitude, longitude) = match &h.coordinate {
        Some(c) => (
            c.latitude
                .as_ref()
                .and_then(|v| parse_num(v.value.as_ref())),
            c.longitude
                .as_ref()
                .and_then(|v| parse_num(v.value.as_ref())),
        ),
        None => (None, None),
    };
    // 深さの condition: 「ごく浅い」は 0 km、「700km 以上」は 700 km として扱う。
    // それ以外の condition (不明など) は値を使わない。
    let depth_cond = h.depth.as_ref().and_then(|d| d.condition.as_deref());
    let depth_over_700 = depth_cond.is_some_and(is_over_700);
    let depth_km = h.depth.as_ref().and_then(|d| match d.condition.as_deref() {
        None => parse_num(d.value.as_ref()),
        Some(c) if is_very_shallow(c) => Some(0.0),
        Some(c) if is_over_700(c) => Some(parse_num(d.value.as_ref()).unwrap_or(700.0)),
        Some(_) => None,
    });
    let assumed = kind == HypocenterKind::Assumed;
    let mag_cond = e.magnitude.as_ref().and_then(|m| m.condition.as_deref());
    let magnitude_over_8 = !assumed && mag_cond.is_some_and(is_over_m8);
    let magnitude = e.magnitude.as_ref().and_then(|m| {
        if m.condition.is_some() || assumed {
            None
        } else {
            parse_num(m.value.as_ref())
        }
    });
    Quake {
        origin_time: e.origin_time,
        arrival_time: e.arrival_time,
        kind,
        name: h.name,
        latitude,
        longitude,
        depth_km,
        depth_over_700,
        magnitude,
        magnitude_over_8,
    }
}

fn convert_region(r: telegram::Region) -> RegionForecast {
    RegionForecast {
        max_int: Forecast::parse(&r.forecast_max_int, Shindo::parse),
        max_lg: r
            .forecast_max_lg_int
            .as_ref()
            .map(|f| Forecast::parse(f, LgClass::parse)),
        code: r.code,
        name: r.name,
        is_warning: r.is_warning,
        is_plum: r.is_plum,
        reached: r.condition.as_deref() == Some("既に主要動到達と推測"),
        arrival_time: r.arrival_time,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Upstream {
    Connected,
    Disconnected,
    Unknown,
}

#[derive(Debug)]
pub enum Incoming {
    Status(Upstream),
    Report(Box<EewReport>),
    Ignored(String),
}

/// WebSocket の 1 メッセージを解釈する。
///
/// v2 の envelope と、リプレイ用に envelope の無い生電文の両方を受け付ける。
/// `accept_non_normal` が偽のときは訓練報と試験報を捨てる。
pub fn parse_message(text: &str, accept_non_normal: bool) -> Incoming {
    let mut v: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => return Incoming::Ignored(format!("JSON として読めない: {e}")),
    };
    let data = if v.get("_schema").is_some() {
        v
    } else {
        match v.get("type").and_then(Value::as_str) {
            Some("status") => {
                let upstream = match v.get("upstream").and_then(Value::as_str) {
                    Some("connected") => Upstream::Connected,
                    Some("disconnected") => Upstream::Disconnected,
                    _ => Upstream::Unknown,
                };
                return Incoming::Status(upstream);
            }
            Some("eew") => {}
            other => return Incoming::Ignored(format!("type {other:?} は対象外")),
        }
        let telegram_type = v.get("telegramType").and_then(Value::as_str);
        if telegram_type != Some("VXSE45") {
            return Incoming::Ignored(format!("telegramType {telegram_type:?} は対象外"));
        }
        match v.get_mut("data") {
            Some(d) => d.take(),
            None => return Incoming::Ignored("data が無い".into()),
        }
    };
    let telegram: EewTelegram = match serde_json::from_value(data) {
        Ok(t) => t,
        Err(e) => return Incoming::Ignored(format!("電文を読めない: {e}")),
    };
    match EewReport::from_telegram(telegram) {
        Ok(r) => filter_status(r, accept_non_normal),
        Err(e) => Incoming::Ignored(e),
    }
}

/// `accept_non_normal` が偽のときは訓練報と試験報を捨てる。
pub fn filter_status(r: EewReport, accept_non_normal: bool) -> Incoming {
    if r.status != TelegramStatus::Normal && !accept_non_normal {
        Incoming::Ignored(format!("{:?} の電文は表示しない設定", r.status))
    } else {
        Incoming::Report(Box::new(r))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shindo_order_and_labels() {
        assert!(Shindo::parse("5-").unwrap() < Shindo::parse("5+").unwrap());
        assert_eq!(Shindo::parse("6+").unwrap().label(), "6強");
        assert_eq!(Shindo::parse("不明"), None);
    }

    #[test]
    fn shindo_parse_ja() {
        assert_eq!(Shindo::parse_ja("5弱"), Shindo::parse("5-"));
        assert_eq!(Shindo::parse_ja("6強"), Shindo::parse("6+"));
        assert_eq!(Shindo::parse_ja("5+"), Shindo::parse("5+"));
        assert_eq!(Shindo::parse_ja("0").unwrap().index(), 0);
        assert_eq!(Shindo::parse_ja("不明"), None);
        assert_eq!(Shindo::parse_ja(""), None);
    }

    #[test]
    fn forecast_representative() {
        let f = Forecast {
            from: Shindo::parse("4"),
            to: Upper::Over,
        };
        assert_eq!(f.representative(), Shindo::parse("4"));
        let f = Forecast {
            from: Shindo::parse("4"),
            to: Upper::Value(Shindo::parse("5-").unwrap()),
        };
        assert_eq!(f.representative(), Shindo::parse("5-"));
    }

    #[test]
    fn status_message() {
        let m = parse_message(
            r#"{"v":2,"type":"status","upstream":"disconnected","database":"connected","at":"x"}"#,
            false,
        );
        assert!(matches!(m, Incoming::Status(Upstream::Disconnected)));
    }

    #[test]
    fn non_eew_envelope_is_ignored() {
        let m = parse_message(
            r#"{"v":2,"type":"earthquake","telegramType":"VXSE53","data":{}}"#,
            false,
        );
        assert!(matches!(m, Incoming::Ignored(_)));
    }

    fn telegram(depth: &str, magnitude: &str, intensity: &str) -> String {
        format!(
            r#"{{"_schema":{{"type":"eew-information"}},"status":"通常","infoType":"発表",
            "reportDateTime":"2026-01-01T00:00:10Z","eventId":"x","serialNo":"1",
            "body":{{"isLastInfo":false,"isCanceled":false,"isWarning":false,
            "earthquake":{{"originTime":"2026-01-01T00:00:00Z","arrivalTime":"2026-01-01T00:00:01Z",
              "hypocenter":{{"name":"x","coordinate":{{"latitude":{{"value":"35.0"}},"longitude":{{"value":"139.0"}}}},
                "depth":{depth},"accuracy":{{"epicenters":["4","4"]}}}},
              "magnitude":{magnitude}}},
            "intensity":{intensity}}}}}"#
        )
    }

    fn report(text: &str) -> EewReport {
        match parse_message(text, false) {
            Incoming::Report(r) => *r,
            other => panic!("{other:?}"),
        }
    }

    const PLAIN_MAG: &str = r#"{"value":"5.0"}"#;
    const NO_INTENSITY: &str = "null";

    #[test]
    fn very_shallow_depth_is_zero() {
        let r = report(&telegram(
            r#"{"value":"0","condition":"ごく浅い"}"#,
            PLAIN_MAG,
            NO_INTENSITY,
        ));
        let q = r.quake.unwrap();
        assert_eq!(q.depth_km, Some(0.0));
        assert!(!q.depth_over_700);
    }

    #[test]
    fn deep_and_giant_conditions() {
        let r = report(&telegram(
            r#"{"value":"700","condition":"７００ｋｍ以上"}"#,
            r#"{"value":null,"condition":"Ｍ８を超える巨大地震"}"#,
            NO_INTENSITY,
        ));
        let q = r.quake.unwrap();
        assert_eq!(q.depth_km, Some(700.0));
        assert!(q.depth_over_700);
        assert_eq!(q.magnitude, None);
        assert!(q.magnitude_over_8);
    }

    #[test]
    fn unknown_depth_stays_unknown() {
        let r = report(&telegram(
            r#"{"value":null,"condition":"不明"}"#,
            PLAIN_MAG,
            NO_INTENSITY,
        ));
        assert_eq!(r.quake.unwrap().depth_km, None);
    }

    #[test]
    fn regions_survive_missing_max_int() {
        let r = report(&telegram(
            r#"{"value":"10"}"#,
            PLAIN_MAG,
            r#"{"regions":[{"code":"350","name":"東京都２３区","forecastMaxInt":{"from":"4","to":"4"}}]}"#,
        ));
        let i = r.intensity.unwrap();
        assert_eq!(i.max_int.to, Upper::Unknown);
        assert_eq!(i.regions.len(), 1);
    }
}
