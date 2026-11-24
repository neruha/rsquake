//! デバッグメニュー用の仮想電文。すべて架空の値で、実際の電文は使わない。
//!
//! DMDATA の JSON は経由せず、正規化済みの `EewReport` を直接組み立てる。
//! 時刻は呼び出し側が渡す「今」を基準にする。

use chrono::{DateTime, Duration, Utc};

use crate::eew::report::{
    EewReport, Forecast, HypocenterKind, IntensityForecast, LgClass, Quake, RegionForecast, Shindo,
    TelegramStatus, Upper,
};

/// 仮想イベントの ID の先頭。本物と見分けるのに使う。
pub const ID_PREFIX: &str = "DEBUG-";

pub fn is_debug_id(id: &str) -> bool {
    id.starts_with(ID_PREFIX)
}

/// 電文の表記 (`5-` など) を、震度の序数 (0〜9) 順に並べたもの。
const SHINDO_KEYS: [&str; 10] = ["0", "1", "2", "3", "4", "5-", "5+", "6-", "6+", "7"];

pub fn shindo_label(index: u8) -> &'static str {
    Shindo::parse(SHINDO_KEYS[index.min(9) as usize]).map_or("?", Shindo::label)
}

/// 区域の予想。`to` は震度の表記のほか、`"over"` (以上)、`"?"` (不明) を取る。`from` の `"-"` は値なし。
type RegionSpec = (u16, &'static str, &'static str);

pub struct Preset {
    pub name: &'static str,
    hypo_name: &'static str,
    lat: f64,
    lon: f64,
    /// 深さ (km)。仮定震源は `None`
    pub depth: Option<f64>,
    depth_over_700: bool,
    /// マグニチュード。仮定震源と「M8 超」は `None`
    pub magnitude: Option<f64>,
    magnitude_over_8: bool,
    kind: HypocenterKind,
    regions: &'static [RegionSpec],
}

impl Preset {
    /// 区域の予想のうち、いちばん大きい震度の序数。
    pub fn max_index(&self) -> u8 {
        self.regions
            .iter()
            .filter_map(|&(_, from, to)| rep_index(from, to))
            .max()
            .unwrap_or(1)
    }

    pub fn is_assumed(&self) -> bool {
        self.kind == HypocenterKind::Assumed
    }
}

pub static PRESETS: &[Preset] = &[
    Preset {
        name: "能登沖 M7.2 (警報・北陸)",
        hypo_name: "能登半島沖",
        lat: 37.5,
        lon: 137.2,
        depth: Some(10.0),
        depth_over_700: false,
        magnitude: Some(7.2),
        magnitude_over_8: false,
        kind: HypocenterKind::Normal,
        regions: &[
            (390, "6-", "6+"),
            (391, "5+", "6-"),
            (381, "5+", "6-"),
            (380, "5-", "5+"),
            (400, "5-", "5+"),
            (370, "5-", "5+"),
            (420, "4", "5-"),
            (430, "4", "5-"),
        ],
    },
    Preset {
        name: "茨城南部 M5.4 (予報のみ・関東)",
        hypo_name: "茨城県南部",
        lat: 36.0,
        lon: 140.1,
        depth: Some(50.0),
        depth_over_700: false,
        magnitude: Some(5.4),
        magnitude_over_8: false,
        kind: HypocenterKind::Normal,
        regions: &[
            (301, "4", "4"),
            (341, "3", "4"),
            (331, "3", "4"),
            (350, "3", "4"),
            (311, "3", "3"),
            (361, "3", "3"),
        ],
    },
    Preset {
        name: "紀伊半島沖 M8超 (巨大地震)",
        hypo_name: "紀伊半島沖",
        lat: 33.0,
        lon: 136.0,
        depth: Some(20.0),
        depth_over_700: false,
        magnitude: None,
        magnitude_over_8: true,
        kind: HypocenterKind::Normal,
        regions: &[
            (550, "6+", "7"),
            (462, "6+", "7"),
            (631, "6-", "6+"),
            (622, "5+", "6-"),
            (443, "5+", "6-"),
            (451, "5+", "6-"),
            (521, "5-", "5+"),
        ],
    },
    Preset {
        name: "オホーツク海 深さ700km以上",
        hypo_name: "オホーツク海南部",
        lat: 46.0,
        lon: 145.5,
        depth: Some(700.0),
        depth_over_700: true,
        magnitude: Some(7.0),
        magnitude_over_8: false,
        kind: HypocenterKind::Normal,
        regions: &[
            (165, "3", "4"),
            (166, "3", "4"),
            (161, "3", "3"),
            (140, "2", "3"),
            (141, "2", "3"),
            (142, "2", "3"),
        ],
    },
    Preset {
        name: "山梨県東部 (レベル法・低精度)",
        hypo_name: "山梨県東部",
        lat: 35.5,
        lon: 138.8,
        depth: Some(10.0),
        depth_over_700: false,
        magnitude: Some(5.0),
        magnitude_over_8: false,
        kind: HypocenterKind::LowAccuracy,
        regions: &[
            (412, "4", "5-"),
            (411, "3", "4"),
            (441, "3", "4"),
            (360, "3", "3"),
            (421, "3", "3"),
        ],
    },
    Preset {
        name: "大阪府北部 (PLUM・仮定震源)",
        hypo_name: "大阪府北部",
        lat: 34.8,
        lon: 135.5,
        depth: None,
        depth_over_700: false,
        magnitude: None,
        magnitude_over_8: false,
        kind: HypocenterKind::Assumed,
        regions: &[
            (520, "5-", "5-"),
            (510, "4", "5-"),
            (531, "4", "4"),
            (501, "4", "4"),
            (540, "3", "4"),
        ],
    },
    Preset {
        name: "三陸沖 M7.9 (多数の区域)",
        hypo_name: "三陸沖",
        lat: 38.0,
        lon: 142.5,
        depth: Some(30.0),
        depth_over_700: false,
        magnitude: Some(7.9),
        magnitude_over_8: false,
        kind: HypocenterKind::Normal,
        regions: &[
            (211, "6-", "6+"),
            (221, "6-", "6+"),
            (251, "6-", "6+"),
            (210, "5+", "6-"),
            (212, "5+", "6-"),
            (220, "5+", "6-"),
            (222, "5+", "6-"),
            (250, "5-", "5+"),
            (252, "5-", "5+"),
            (213, "5-", "5+"),
            (231, "5-", "5+"),
            (233, "5-", "5+"),
            (202, "5-", "5+"),
            (203, "5-", "5+"),
            (241, "4", "5-"),
            (242, "4", "5-"),
            (230, "4", "5-"),
            (232, "4", "5-"),
            (200, "4", "4"),
            (201, "4", "4"),
            (240, "4", "4"),
            (243, "4", "4"),
            (300, "4", "5-"),
            (301, "4", "4"),
            (310, "3", "4"),
            (311, "3", "4"),
            (320, "3", "3"),
            (321, "3", "3"),
            (330, "3", "4"),
            (340, "3", "4"),
            (350, "3", "4"),
        ],
    },
    Preset {
        name: "釧路沖 M7.5 (以上・不明あり)",
        hypo_name: "釧路沖",
        lat: 42.5,
        lon: 144.5,
        depth: Some(40.0),
        depth_over_700: false,
        magnitude: Some(7.5),
        magnitude_over_8: false,
        kind: HypocenterKind::Normal,
        regions: &[
            (160, "5+", "over"),
            (161, "5-", "over"),
            (165, "4", "5-"),
            (166, "-", "?"),
        ],
    },
];

/// 震度の表記から序数を引く。
fn key_index(key: &str) -> Option<u8> {
    SHINDO_KEYS.iter().position(|k| *k == key).map(|i| i as u8)
}

/// 区域の代表の震度 (`to` があればそれ、`over` と不明なら `from`)。
fn rep_index(from: &str, to: &str) -> Option<u8> {
    key_index(to).or_else(|| key_index(from))
}

fn shindo(index: u8) -> Shindo {
    Shindo::parse(SHINDO_KEYS[index.min(9) as usize]).expect("表の値は震度として読める")
}

fn shift(index: u8, delta: i16) -> u8 {
    (index as i16 + delta).clamp(1, 9) as u8
}

fn region_forecast(from: &str, to: &str, delta: i16) -> Forecast<Shindo> {
    let to = match to {
        "over" => Upper::Over,
        "?" => Upper::Unknown,
        k => key_index(k).map_or(Upper::Unknown, |i| Upper::Value(shindo(shift(i, delta)))),
    };
    Forecast {
        from: key_index(from).map(|i| shindo(shift(i, delta))),
        to,
    }
}

/// 仮想電文の細分区域の名前 (地図の `eew-region` の code と対応)。
fn region_name(code: u16) -> &'static str {
    match code {
        140 => "網走地方",
        141 => "北見地方",
        142 => "紋別地方",
        160 => "釧路地方北部",
        161 => "釧路地方中南部",
        165 => "根室地方北部",
        166 => "根室地方中部",
        200 => "青森県津軽北部",
        201 => "青森県津軽南部",
        202 => "青森県三八上北",
        203 => "青森県下北",
        210 => "岩手県沿岸北部",
        211 => "岩手県沿岸南部",
        212 => "岩手県内陸北部",
        213 => "岩手県内陸南部",
        220 => "宮城県北部",
        221 => "宮城県南部",
        222 => "宮城県中部",
        230 => "秋田県沿岸北部",
        231 => "秋田県沿岸南部",
        232 => "秋田県内陸北部",
        233 => "秋田県内陸南部",
        240 => "山形県庄内",
        241 => "山形県最上",
        242 => "山形県村山",
        243 => "山形県置賜",
        250 => "福島県中通り",
        251 => "福島県浜通り",
        252 => "福島県会津",
        300 => "茨城県北部",
        301 => "茨城県南部",
        310 => "栃木県北部",
        311 => "栃木県南部",
        320 => "群馬県北部",
        321 => "群馬県南部",
        330 => "埼玉県北部",
        331 => "埼玉県南部",
        340 => "千葉県北東部",
        341 => "千葉県北西部",
        350 => "東京都２３区",
        360 => "神奈川県東部",
        361 => "神奈川県西部",
        370 => "新潟県上越",
        380 => "富山県東部",
        381 => "富山県西部",
        390 => "石川県能登",
        391 => "石川県加賀",
        400 => "福井県嶺北",
        411 => "山梨県中・西部",
        412 => "山梨県東部・富士五湖",
        420 => "長野県北部",
        421 => "長野県中部",
        430 => "岐阜県飛騨",
        441 => "静岡県東部",
        443 => "静岡県西部",
        451 => "愛知県西部",
        462 => "三重県南部",
        501 => "滋賀県南部",
        510 => "京都府北部",
        520 => "大阪府北部",
        521 => "大阪府南部",
        531 => "兵庫県南東部",
        540 => "奈良県",
        550 => "和歌山県北部",
        622 => "愛媛県南予",
        631 => "高知県中部",
        _ => "区域",
    }
}

/// メニューで編集できる値。
#[derive(Clone, Debug)]
pub struct Params {
    pub preset: usize,
    pub magnitude: f64,
    pub depth: f64,
    /// 最大震度の序数。プリセットの値からの差で、全区域の震度をずらす
    pub max_index: u8,
    pub long_period: bool,
    pub status: TelegramStatus,
    /// 新しいイベントを作るとき、発生からの経過秒
    pub elapsed_secs: u32,
}

impl Params {
    pub fn for_preset(preset: usize) -> Self {
        let p = &PRESETS[preset];
        Self {
            preset,
            magnitude: p.magnitude.unwrap_or(6.0),
            depth: p.depth.unwrap_or(10.0),
            max_index: p.max_index(),
            long_period: false,
            status: TelegramStatus::Normal,
            elapsed_secs: 5,
        }
    }

    pub fn preset(&self) -> &'static Preset {
        &PRESETS[self.preset]
    }
}

/// 1 報を組み立てる。`origin` は発生時刻 (同じイベントの報では同じ値を渡す)。
pub fn build_report(
    params: &Params,
    event_id: &str,
    serial: u32,
    origin: DateTime<Utc>,
    now: DateTime<Utc>,
    is_last: bool,
) -> EewReport {
    let preset = params.preset();
    let assumed = preset.is_assumed();
    let delta = params.max_index as i16 - preset.max_index() as i16;

    let regions: Vec<RegionForecast> = preset
        .regions
        .iter()
        .map(|&(code, from, to)| {
            let max_int = region_forecast(from, to, delta);
            let rep = max_int.representative();
            let idx = rep.map_or(0, Shindo::index);
            let max_lg = (params.long_period && idx >= 4).then(|| {
                let (a, b) = if idx >= 6 { ("2", "3") } else { ("1", "2") };
                Forecast {
                    from: LgClass::parse(a),
                    to: LgClass::parse(b).map_or(Upper::Unknown, Upper::Value),
                }
            });
            RegionForecast {
                code: code.to_string(),
                name: region_name(code).to_string(),
                max_int,
                max_lg,
                is_warning: idx >= 5,
                is_plum: assumed,
                reached: false,
                arrival_time: None,
            }
        })
        .collect();

    let max_rep = regions
        .iter()
        .filter_map(|r| r.max_int.representative())
        .max();
    let max_int = match max_rep {
        Some(m) => {
            let over = regions
                .iter()
                .any(|r| r.max_int.to == Upper::Over && r.max_int.from == Some(m));
            Forecast {
                from: regions.iter().filter_map(|r| r.max_int.from).max(),
                to: if over { Upper::Over } else { Upper::Value(m) },
            }
        }
        None => Forecast {
            from: None,
            to: Upper::Unknown,
        },
    };
    let max_lg = regions
        .iter()
        .filter_map(|r| r.max_lg)
        .max_by_key(|f| f.representative().map_or(0, |l| l.index()));
    let is_warning = regions.iter().any(|r| r.is_warning);

    let quake = Quake {
        origin_time: (!assumed).then_some(origin),
        arrival_time: Some(origin + Duration::seconds(2)),
        kind: preset.kind,
        name: preset.hypo_name.to_string(),
        latitude: Some(preset.lat),
        longitude: Some(preset.lon),
        depth_km: if assumed {
            None
        } else if preset.depth_over_700 {
            Some(700.0)
        } else {
            Some(params.depth.round())
        },
        depth_over_700: preset.depth_over_700,
        magnitude: (preset.magnitude.is_some() && !assumed)
            .then(|| (params.magnitude * 10.0).round() / 10.0),
        magnitude_over_8: preset.magnitude_over_8,
    };

    EewReport {
        event_id: event_id.to_string(),
        serial,
        status: params.status,
        report_time: now,
        is_cancel: false,
        is_last,
        is_warning,
        quake: Some(quake),
        intensity: Some(IntensityForecast {
            max_int,
            max_lg,
            regions,
        }),
    }
}

/// 取消報。
pub fn build_cancel(
    event_id: &str,
    serial: u32,
    status: TelegramStatus,
    now: DateTime<Utc>,
) -> EewReport {
    EewReport {
        event_id: event_id.to_string(),
        serial,
        status,
        report_time: now,
        is_cancel: true,
        is_last: true,
        is_warning: false,
        quake: None,
        intensity: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        "2026-01-01T00:00:00Z".parse().unwrap()
    }

    #[test]
    fn every_preset_builds_a_valid_report() {
        for (i, preset) in PRESETS.iter().enumerate() {
            let p = Params::for_preset(i);
            let r = build_report(&p, "DEBUG-0001", 1, now(), now(), false);
            let q = r.quake.as_ref().unwrap();
            assert!(q.position().is_some(), "{}", preset.name);
            let intensity = r.intensity.as_ref().unwrap();
            assert!(!intensity.regions.is_empty());
            assert!(is_debug_id(&r.event_id));
        }
    }

    #[test]
    fn region_codes_match_the_bundled_map() {
        let topo = crate::geo::topo::Topo::parse(include_bytes!(
            "../../assets/geojson/eew-prefregion.json"
        ))
        .unwrap();
        let names = topo.region_name_to_code();
        let codes: std::collections::HashSet<&str> = names.values().map(String::as_str).collect();
        for preset in PRESETS {
            for &(code, _, _) in preset.regions {
                assert!(codes.contains(code.to_string().as_str()), "code {code}");
                // 名前も地図のものと同じ (区域名の表の打ち間違いを防ぐ)
                assert_eq!(
                    names.get(region_name(code)).map(String::as_str),
                    Some(code.to_string().as_str()),
                    "code {code}"
                );
            }
        }
    }

    #[test]
    fn max_index_shifts_every_region() {
        let mut p = Params::for_preset(1);
        let base = p.max_index;
        p.max_index = base + 2;
        let r = build_report(&p, "DEBUG-0001", 1, now(), now(), false);
        let max = r.intensity.unwrap().max_int.representative().unwrap();
        assert_eq!(max.index(), base + 2);
    }

    #[test]
    fn warning_follows_intensity() {
        let quiet = build_report(&Params::for_preset(1), "DEBUG-1", 1, now(), now(), false);
        assert!(!quiet.is_warning);
        let loud = build_report(&Params::for_preset(0), "DEBUG-1", 1, now(), now(), false);
        assert!(loud.is_warning);
    }

    #[test]
    fn cancel_has_no_body() {
        let c = build_cancel("DEBUG-1", 3, TelegramStatus::Normal, now());
        assert!(c.is_cancel && c.quake.is_none() && c.intensity.is_none());
    }
}
