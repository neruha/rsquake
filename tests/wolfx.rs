//! Wolfx `jma_eew` の実データ (tests/fixtures/wolfx) を `EewReport` に変換して確かめる。
//! DMDATA の実電文 (kamchatka-false-alarm) とも報ごとに突き合わせる。

mod common;

use std::collections::HashMap;
use std::path::Path;

use chrono::{DateTime, Duration, Utc};
use rsquake::dev::replay::load_messages;
use rsquake::eew::report::{
    EewReport, Forecast, HypocenterKind, Incoming, RegionForecast, Shindo, TelegramStatus, Upper,
    parse_message,
};
use rsquake::eew::store::{ApplyOutcome, EventStore};
use rsquake::eew::wolfx::{self, RegionCodes};
use rsquake::geo::topo::Topo;

fn codes() -> RegionCodes {
    Topo::parse(include_bytes!("../assets/geojson/eew-prefregion.json"))
        .unwrap()
        .region_name_to_code()
}

fn raw(dir: &str) -> Vec<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(dir);
    load_messages(&path).unwrap()
}

fn wolfx_reports(dir: &str) -> Vec<EewReport> {
    let codes = codes();
    raw(&format!("wolfx/{dir}"))
        .iter()
        .enumerate()
        .map(|(i, m)| match wolfx::parse_message(m, true, &codes) {
            Incoming::Report(r) => *r,
            other => panic!("{dir} の {} 件目が読めない: {other:?}", i + 1),
        })
        .collect()
}

fn region<'a>(r: &'a EewReport, name: &str) -> &'a RegionForecast {
    r.intensity
        .as_ref()
        .unwrap()
        .regions
        .iter()
        .find(|x| x.name == name)
        .unwrap_or_else(|| panic!("{name} が無い"))
}

fn sh(s: &str) -> Option<Shindo> {
    Shindo::parse_ja(s)
}

#[test]
fn every_wolfx_fixture_parses() {
    require_fixtures!("wolfx");
    let mut n = 0;
    for d in [
        "aomori-warning-20251208",
        "kamchatka-false-alarm",
        "kumamoto-forecast-20261119",
        "kumamoto-warning-20261119",
        "low-accuracy-20260728",
    ] {
        n += wolfx_reports(d).len();
    }
    assert_eq!(n, 1 + 44 + 4 + 16 + 6);
}

#[test]
fn unknown_region_names_are_not_in_any_fixture() {
    require_fixtures!("wolfx");
    let codes = codes();
    for d in [
        "aomori-warning-20251208",
        "kamchatka-false-alarm",
        "kumamoto-warning-20261119",
    ] {
        for r in wolfx_reports(d).iter().filter(|r| !r.is_cancel) {
            for a in &r.intensity.as_ref().unwrap().regions {
                assert!(codes.contains_key(&a.name), "{}: {}", d, a.name);
                assert!(!a.code.is_empty(), "{}: {}", d, a.name);
            }
        }
    }
}

/// `OriginalText` の `EBI` 以降 (区域コード・S{最大}{最小}・時刻・区分の 4 つ組) と区域が一致する。
#[test]
fn region_codes_match_original_text() {
    require_fixtures!("wolfx");
    let codes = codes();
    for d in ["kumamoto-warning-20261119", "aomori-warning-20251208"] {
        for (m, r) in raw(&format!("wolfx/{d}")).iter().zip(wolfx_reports(d)) {
            let v: serde_json::Value = serde_json::from_str(m).unwrap();
            let text = v["OriginalText"].as_str().unwrap();
            let Some((_, ebi)) = text.split_once("EBI") else {
                continue;
            };
            let toks: Vec<&str> = ebi.split_whitespace().filter(|t| *t != "9999=").collect();
            let want: Vec<&str> = toks.chunks(4).map(|c| c[0]).collect();
            let got: Vec<&str> = r
                .intensity
                .as_ref()
                .unwrap()
                .regions
                .iter()
                .map(|x| x.code.as_str())
                .collect();
            assert_eq!(got, want, "{d} 第{}報", r.serial);
            assert!(
                r.intensity
                    .as_ref()
                    .unwrap()
                    .regions
                    .iter()
                    .all(|x| codes.contains_key(&x.name))
            );
        }
    }
}

#[test]
fn kumamoto_warning_sequence() {
    require_fixtures!("wolfx");
    let rs = wolfx_reports("kumamoto-warning-20261119");
    assert_eq!(rs.len(), 16);
    assert!(rs.iter().all(|r| r.event_id == "20261119022225"));

    // 第 1 報: 仮定震源。M なし、PLUM の 4 区域、最小震度は不明
    let q = rs[0].quake.as_ref().unwrap();
    assert_eq!(q.kind, HypocenterKind::Assumed);
    assert_eq!(q.magnitude, None);
    assert_eq!(q.depth_km, None);
    assert!(rs[0].is_warning);
    let regions = &rs[0].intensity.as_ref().unwrap().regions;
    assert_eq!(regions.len(), 4);
    assert!(
        regions
            .iter()
            .all(|r| r.is_plum && !r.reached && r.arrival_time.is_none())
    );
    let r = region(&rs[0], "熊本県天草・芦北");
    assert_eq!(r.code, "743");
    assert_eq!(
        r.max_int,
        Forecast {
            from: None,
            to: Upper::Value(sh("5弱").unwrap())
        }
    );

    // 第 2 報: IPF 法 (5 点以上) で通常の震源に
    assert_eq!(rs[1].quake.as_ref().unwrap().kind, HypocenterKind::Normal);
    assert_eq!(rs[1].quake.as_ref().unwrap().magnitude, Some(4.9));

    // 第 3 報: M5.9、6 弱。到達済み・未到達・予報の区域が混ざる
    let q = rs[2].quake.as_ref().unwrap();
    assert_eq!(q.magnitude, Some(5.9));
    assert_eq!(q.depth_km, Some(10.0));
    assert_eq!(
        rs[2].intensity.as_ref().unwrap().max_int,
        Forecast {
            from: sh("6弱"),
            to: Upper::Value(sh("6弱").unwrap())
        }
    );
    let r = region(&rs[2], "熊本県天草・芦北");
    assert!(r.reached && !r.is_plum && r.arrival_time.is_none() && r.is_warning);
    assert_eq!(
        r.max_int,
        Forecast {
            from: sh("5強"),
            to: Upper::Value(sh("6弱").unwrap())
        }
    );
    let r = region(&rs[2], "熊本県球磨");
    assert!(!r.reached && !r.is_plum && r.is_warning);
    // 到達予測時刻 02:22:33 JST = 17:22:33 UTC 前日
    assert_eq!(
        r.arrival_time.unwrap(),
        "2026-11-18T17:22:33Z".parse::<DateTime<Utc>>().unwrap()
    );
    let r = region(&rs[2], "長崎県島原半島");
    assert!(!r.is_warning, "予報の区域");

    // 最終報
    let last = rs.last().unwrap();
    assert!(last.is_last);
    assert_eq!(last.intensity.as_ref().unwrap().regions.len(), 9);
    assert!(rs[..15].iter().all(|r| !r.is_last));

    // EventStore に順に入れると、初めての警報で BecameWarning、あとは更新
    let mut s = EventStore::new(false);
    let out: Vec<_> = rs
        .into_iter()
        .map(|r| {
            let at = r.report_time;
            s.apply(r, at)
        })
        .collect();
    assert_eq!(out[0], ApplyOutcome::New);
    assert!(
        out.iter().all(|o| !matches!(o, ApplyOutcome::Rejected(_))),
        "{out:?}"
    );
}

#[test]
fn forecast_has_no_regions() {
    require_fixtures!("wolfx");
    let rs = wolfx_reports("kumamoto-forecast-20261119");
    assert_eq!(rs.len(), 4);
    assert!(rs.iter().all(|r| !r.is_warning));
    assert!(
        rs.iter()
            .all(|r| r.intensity.as_ref().unwrap().regions.is_empty())
    );
    assert_eq!(rs[0].quake.as_ref().unwrap().magnitude, Some(3.4));
    assert_eq!(rs[0].quake.as_ref().unwrap().kind, HypocenterKind::Normal);
}

#[test]
fn low_accuracy_old_shape() {
    require_fixtures!("wolfx");
    let rs = wolfx_reports("low-accuracy-20260728");
    let q = rs[0].quake.as_ref().unwrap();
    assert_eq!(q.kind, HypocenterKind::LowAccuracy);
    // `Magnitude` が無く `Magunitude` だけの古い形でも M が読める
    assert_eq!(q.magnitude, Some(3.8));
    assert_eq!(q.depth_km, Some(10.0));
    assert_eq!(
        rs[0].intensity.as_ref().unwrap().max_int,
        Forecast {
            from: None,
            to: Upper::Unknown
        }
    );
}

#[test]
fn aomori_old_shape_warning() {
    require_fixtures!("wolfx");
    let rs = wolfx_reports("aomori-warning-20251208");
    assert_eq!(rs[0].intensity.as_ref().unwrap().regions.len(), 37);
    assert!(rs[0].is_warning);
    assert_eq!(rs[0].status, TelegramStatus::Normal);
}

/// DMDATA の実電文と Wolfx の同じ地震を、報ごとに突き合わせる。
#[test]
fn kamchatka_matches_dmdata() {
    require_fixtures!("kamchatka-false-alarm", "wolfx/kamchatka-false-alarm");
    let w = wolfx_reports("kamchatka-false-alarm");
    let d: Vec<EewReport> = raw("kamchatka-false-alarm")
        .iter()
        .map(|m| match parse_message(m, true) {
            Incoming::Report(r) => *r,
            o => panic!("{o:?}"),
        })
        .collect();
    let by_serial = |rs: &[EewReport]| -> HashMap<u32, usize> {
        rs.iter().enumerate().map(|(i, r)| (r.serial, i)).collect()
    };
    let wi = by_serial(&w);
    let mut compared = 0;
    for dr in &d {
        if dr.is_cancel {
            continue;
        }
        // 取消のあとの 42 報目は Wolfx にだけ isCancel の形で入る
        let Some(&i) = wi.get(&dr.serial) else {
            continue;
        };
        let wr = &w[i];
        if wr.is_cancel {
            continue;
        }
        compared += 1;
        let (dq, wq) = (dr.quake.as_ref().unwrap(), wr.quake.as_ref().unwrap());
        let tag = format!("第{}報", dr.serial);
        assert_eq!(wr.event_id, dr.event_id, "{tag}");
        assert_eq!(wr.is_warning, dr.is_warning, "{tag}");
        assert_eq!(wq.name, dq.name, "{tag}");
        assert_eq!(wq.kind, dq.kind, "{tag}");
        assert_eq!(wq.magnitude, dq.magnitude, "{tag}");
        assert_eq!(wq.depth_km, dq.depth_km, "{tag}");
        assert_eq!(wq.origin_time, dq.origin_time, "{tag}");
        assert!(
            (wq.latitude.unwrap() - dq.latitude.unwrap()).abs() < 1e-6,
            "{tag}"
        );
        assert!(
            (wq.longitude.unwrap() - dq.longitude.unwrap()).abs() < 1e-6,
            "{tag}"
        );
        let Some(di) = dr.intensity.as_ref() else {
            eprintln!(
                "{tag}: DMDATA に intensity が無い (Wolfx の区域は {})",
                wr.intensity.as_ref().map_or(0, |i| i.regions.len())
            );
            continue;
        };
        let wi_ = wr.intensity.as_ref().unwrap();
        assert_eq!(
            wi_.max_int.representative(),
            di.max_int.representative(),
            "{tag}"
        );
        let dm: HashMap<&str, &RegionForecast> =
            di.regions.iter().map(|r| (r.code.as_str(), r)).collect();
        for wr_ in &wi_.regions {
            let Some(dr_) = dm.get(wr_.code.as_str()) else {
                panic!("{tag}: {} が DMDATA に無い", wr_.name);
            };
            assert_eq!(wr_.name, dr_.name, "{tag}");
            // DMDATA の from "0" は、Wolfx では 不明 (None)
            let dfrom = dr_.max_int.from.filter(|s| s.index() != 0);
            assert_eq!(wr_.max_int.from, dfrom, "{tag} {}", wr_.name);
            let dto = match dr_.max_int.to {
                Upper::Value(s) if s.index() == 0 => Upper::Unknown,
                t => t,
            };
            assert_eq!(wr_.max_int.to, dto, "{tag} {}", wr_.name);
            assert_eq!(wr_.is_warning, dr_.is_warning, "{tag} {}", wr_.name);
            assert_eq!(wr_.reached, dr_.reached, "{tag} {}", wr_.name);
            assert_eq!(wr_.arrival_time, dr_.arrival_time, "{tag} {}", wr_.name);
        }
        // Wolfx は区域が欠けることはあっても、増えることは無い
        assert!(wi_.regions.len() <= di.regions.len(), "{tag}");
    }
    assert!(compared >= 30, "{compared}");
}

#[test]
fn kamchatka_both_cancel_shapes_cancel() {
    require_fixtures!("wolfx/kamchatka-false-alarm");
    let w = wolfx_reports("kamchatka-false-alarm");
    let n = w.len();
    // 043: 最終報 (第 42 報) と同じ中身に isCancel が付いた形 / 044: 震源の無い取消電文 (第 37 報)
    let (a, b) = (&w[n - 2], &w[n - 1]);
    assert_eq!((a.serial, a.is_cancel), (42, true));
    assert_eq!((b.serial, b.is_cancel), (37, true));
    assert!(a.quake.is_none() && b.quake.is_none());

    // どちらの形でも、取消として EventStore に出る (取消は報数を見ない)
    for cancel in [a, b] {
        let mut s = EventStore::new(false);
        let mut out = Vec::new();
        for r in &w[..n - 2] {
            let at = r.report_time;
            out.push(s.apply(r.clone(), at));
        }
        assert!(
            out.iter().all(|o| !matches!(o, ApplyOutcome::Rejected(_))),
            "{out:?}"
        );
        assert!(out.contains(&ApplyOutcome::BecameWarning));
        let at = cancel.report_time;
        assert_eq!(s.apply(cancel.clone(), at), ApplyOutcome::Canceled);
        assert!(s.active(at)[0].is_canceled());
    }
}
#[test]
fn cancel_reports_have_no_quake() {
    require_fixtures!("wolfx");
    // DB の 2 通りの取消の形を、JSON を作って確かめる
    let codes = codes();
    let base = raw("wolfx/kumamoto-warning-20261119")[15].clone();
    let mut v: serde_json::Value = serde_json::from_str(&base).unwrap();
    v["isCancel"] = true.into();
    match wolfx::parse_message(&v.to_string(), false, &codes) {
        Incoming::Report(r) => {
            assert!(r.is_cancel && r.quake.is_none() && r.intensity.is_none());
            assert!(!r.is_warning);
        }
        o => panic!("{o:?}"),
    }
    // 震源が空・0 の取消電文
    let last = raw("wolfx/kamchatka-false-alarm").pop().unwrap();
    match wolfx::parse_message(&last, false, &codes) {
        Incoming::Report(r) => {
            assert!(r.is_cancel && r.quake.is_none() && r.intensity.is_none());
            assert_eq!(r.serial, 37);
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn http_shape_without_type_is_accepted_and_others_are_ignored() {
    require_fixtures!("wolfx");
    let codes = codes();
    let mut v: serde_json::Value =
        serde_json::from_str(&raw("wolfx/kumamoto-forecast-20261119")[3]).unwrap();
    v.as_object_mut().unwrap().remove("type");
    assert!(matches!(
        wolfx::parse_message(&v.to_string(), false, &codes),
        Incoming::Report(_)
    ));
    for text in [
        r#"{"type":"heartbeat","ver":1,"id":"x","timestamp":1}"#,
        r#"{"type":"pong","timestamp":1}"#,
        r#"{"type":"all_eew"}"#,
        "not json",
    ] {
        assert!(
            matches!(
                wolfx::parse_message(text, false, &codes),
                Incoming::Ignored(_)
            ),
            "{text}"
        );
    }
}

#[test]
fn training_is_filtered_by_setting() {
    require_fixtures!("wolfx");
    let codes = codes();
    let mut v: serde_json::Value =
        serde_json::from_str(&raw("wolfx/kumamoto-warning-20261119")[0]).unwrap();
    v["isTraining"] = true.into();
    let text = v.to_string();
    assert!(matches!(
        wolfx::parse_message(&text, false, &codes),
        Incoming::Ignored(_)
    ));
    match wolfx::parse_message(&text, true, &codes) {
        Incoming::Report(r) => assert_eq!(r.status, TelegramStatus::Training),
        o => panic!("{o:?}"),
    }
}

#[test]
fn level_method_has_detection_time_and_low_accuracy() {
    require_fixtures!("wolfx");
    let codes = codes();
    let mut v: serde_json::Value =
        serde_json::from_str(&raw("wolfx/kumamoto-warning-20261119")[1]).unwrap();
    v["CodeType"] = "最大予測震度のみの緊急地震速報".into();
    match wolfx::parse_message(&v.to_string(), false, &codes) {
        Incoming::Report(r) => {
            let q = r.quake.unwrap();
            assert_eq!(q.kind, HypocenterKind::LowAccuracy);
            assert_eq!(q.origin_time, None);
            assert!(q.arrival_time.is_some());
            assert_eq!(q.depth_km, None);
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn okinawa_aliases_resolve() {
    require_fixtures!("wolfx");
    let codes = codes();
    let mut v: serde_json::Value =
        serde_json::from_str(&raw("wolfx/kumamoto-warning-20261119")[2]).unwrap();
    v["WarnArea"][0]["Chiiki"] = "大東島".into();
    v["WarnArea"][1]["Chiiki"] = "宮古島".into();
    match wolfx::parse_message(&v.to_string(), false, &codes) {
        Incoming::Report(r) => {
            let rg = &r.intensity.unwrap().regions;
            assert_eq!(rg[0].code, "803");
            assert_eq!(rg[1].code, "804");
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn unknown_region_name_gets_empty_code_but_stays() {
    require_fixtures!("wolfx");
    let codes = codes();
    let mut v: serde_json::Value =
        serde_json::from_str(&raw("wolfx/kumamoto-warning-20261119")[2]).unwrap();
    v["WarnArea"][0]["Chiiki"] = "存在しない区域".into();
    match wolfx::parse_message(&v.to_string(), false, &codes) {
        Incoming::Report(r) => {
            let rg = &r.intensity.unwrap().regions;
            assert_eq!(rg[0].code, "");
            assert_eq!(rg[0].name, "存在しない区域");
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn arrive_accepts_unexpected_types() {
    require_fixtures!("wolfx");
    let codes = codes();
    for arrive in [
        serde_json::json!(true),
        serde_json::json!(null),
        serde_json::json!("???"),
    ] {
        let mut v: serde_json::Value =
            serde_json::from_str(&raw("wolfx/kumamoto-warning-20261119")[2]).unwrap();
        v["WarnArea"][0]["Arrive"] = arrive;
        match wolfx::parse_message(&v.to_string(), false, &codes) {
            Incoming::Report(r) => {
                let a = &r.intensity.unwrap().regions[0];
                assert!(!a.reached && !a.is_plum);
            }
            o => panic!("{o:?}"),
        }
    }
}

#[test]
fn arrival_time_crosses_midnight() {
    require_fixtures!("wolfx");
    let codes = codes();
    let mut v: serde_json::Value =
        serde_json::from_str(&raw("wolfx/kumamoto-warning-20261119")[2]).unwrap();
    v["AnnouncedTime"] = "2026/11/19 23:59:50".into();
    v["WarnArea"][2]["Time"] = "000010".into();
    match wolfx::parse_message(&v.to_string(), false, &codes) {
        Incoming::Report(r) => {
            let a = &r.intensity.as_ref().unwrap().regions[2];
            let t = a.arrival_time.unwrap();
            assert_eq!(t - r.report_time, Duration::seconds(20));
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn bad_announced_time_is_ignored() {
    require_fixtures!("wolfx");
    let codes = codes();
    let mut v: serde_json::Value =
        serde_json::from_str(&raw("wolfx/kumamoto-warning-20261119")[0]).unwrap();
    v["AnnouncedTime"] = "yesterday".into();
    assert!(matches!(
        wolfx::parse_message(&v.to_string(), false, &codes),
        Incoming::Ignored(_)
    ));
}

#[test]
fn stale_check() {
    require_fixtures!("wolfx");
    let rs = wolfx_reports("kumamoto-forecast-20261119");
    let t = rs[0].report_time;
    assert!(!wolfx::is_stale(&rs[0], t + Duration::minutes(2)));
    assert!(wolfx::is_stale(&rs[0], t + Duration::minutes(4)));
}

/// eqsend-core の areas.json にある 188 区域名が、地図から作った表ですべて引ける。
#[test]
fn eqsend_core_area_names_resolve() {
    require_fixtures!("wolfx/area-names.txt");
    let names_text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wolfx/area-names.txt"),
    )
    .unwrap();
    let codes = codes();
    let names: Vec<&str> = names_text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();
    assert_eq!(names.len(), 188);
    for n in names {
        assert!(codes.contains_key(n), "{n}");
    }
}

#[test]
fn replay_reads_both_shapes() {
    require_fixtures!("wolfx");
    use rsquake::dev::replay::parse_any;
    let codes = codes();
    for m in raw("wolfx/kumamoto-warning-20261119").iter().chain(
        // 暫定措置: DMDATA の電文フィクスチャは公開リポジトリに無い
        if common::has_fixture("kamchatka-false-alarm") {
            raw("kamchatka-false-alarm")
        } else {
            Vec::new()
        }
        .iter(),
    ) {
        assert!(matches!(parse_any(m, &codes), Incoming::Report(_)), "{m}");
    }
}

#[test]
fn zero_depth_with_known_accuracy_is_very_shallow() {
    let codes = codes();
    let msg = r#"{"type":"jma_eew","EventID":"x","Serial":1,"AnnouncedTime":"2026/01/01 09:00:10","OriginTime":"2026/01/01 09:00:00","Hypocenter":"x","Latitude":35.0,"Longitude":139.0,"Magnitude":4.0,"Depth":0,"MaxIntensity":"3","Accuracy":{"Epicenter":"IPF 法（5 点以上）","Depth":"IPF 法（5 点以上）"}}"#;
    let Incoming::Report(r) = wolfx::parse_message(msg, true, &codes) else {
        panic!()
    };
    assert_eq!(r.quake.unwrap().depth_km, Some(0.0));
    let unknown = msg.replace("IPF 法（5 点以上）\"}", "不明、未設定時、キャンセル時\"}");
    let Incoming::Report(r) = wolfx::parse_message(&unknown, true, &codes) else {
        panic!()
    };
    assert_eq!(r.quake.unwrap().depth_km, None);
}
