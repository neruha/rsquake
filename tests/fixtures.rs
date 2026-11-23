//! すべてのフィクスチャが読めること、筋書きどおりにイベント状態が動くことを確かめる。

mod common;

use std::path::Path;

use chrono::{DateTime, Duration, Utc};
use rsquake::dev::replay::load_messages;
use rsquake::eew::report::{EewReport, HypocenterKind, Incoming, TelegramStatus, parse_message};
use rsquake::eew::store::{ApplyOutcome, EventStore};

fn reports(dir: &str) -> Vec<EewReport> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(dir);
    load_messages(&path)
        .unwrap()
        .iter()
        .enumerate()
        .map(|(i, m)| match parse_message(m, true) {
            Incoming::Report(r) => *r,
            other => panic!("{dir} の {} 件目が読めない: {other:?}", i + 1),
        })
        .collect()
}

#[test]
fn every_fixture_parses() {
    require_fixtures!(".");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut n = 0;
    for entry in std::fs::read_dir(root).unwrap() {
        let p = entry.unwrap().path();
        let name = p.file_name().unwrap().to_str().unwrap();
        // wolfx / kmoni / srev-replay は DMDATA の電文ではない
        if p.is_dir() && !matches!(name, "wolfx" | "kmoni" | "srev-replay") {
            n += reports(name).len();
        }
    }
    // 暫定措置: DMDATA の電文フィクスチャが無い (公開リポジトリ) ときは件数を問わない
    if n == 0 {
        eprintln!("skip: DMDATA の電文フィクスチャが無い");
        return;
    }
    assert!(n >= 60, "{n}");
}

/// 実時間で受けたものとして、受信時刻 = reportDateTime で流す。
fn play(store: &mut EventStore, rs: Vec<EewReport>) -> Vec<ApplyOutcome> {
    rs.into_iter()
        .map(|r| {
            let at = r.report_time;
            store.apply(r, at)
        })
        .collect()
}

#[test]
fn kamchatka_goes_warning_then_cancel() {
    require_fixtures!("kamchatka-false-alarm");
    let rs = reports("kamchatka-false-alarm");
    let last_time = rs.last().unwrap().report_time;
    let mut s = EventStore::new(false);
    let out = play(&mut s, rs);
    assert_eq!(out[0], ApplyOutcome::New);
    assert!(out.contains(&ApplyOutcome::BecameWarning));
    assert_eq!(*out.last().unwrap(), ApplyOutcome::Canceled);
    let active = s.active(last_time);
    assert_eq!(active.len(), 1);
    assert!(active[0].is_canceled());
    assert!(s.active(last_time + Duration::seconds(301)).is_empty());
}

#[test]
fn okinawa_level_starts_low_accuracy() {
    require_fixtures!("okinawa-level");
    let rs = reports("okinawa-level");
    assert_eq!(
        rs[0].quake.as_ref().unwrap().kind,
        HypocenterKind::LowAccuracy
    );
    assert_eq!(rs[0].quake.as_ref().unwrap().magnitude, None);
    assert_eq!(rs[0].quake.as_ref().unwrap().origin_time, None);
    let mut s = EventStore::new(false);
    let out = play(&mut s, rs);
    assert!(
        out.iter().all(|o| !matches!(o, ApplyOutcome::Rejected(_))),
        "{out:?}"
    );
}

#[test]
fn plum_is_assumed() {
    require_fixtures!("manual-plum");
    let rs = reports("manual-plum");
    let q = rs[0].quake.as_ref().unwrap();
    assert_eq!(q.kind, HypocenterKind::Assumed);
    assert_eq!(q.magnitude, None);
    assert_eq!(rs[1].quake.as_ref().unwrap().kind, HypocenterKind::Normal);
}

#[test]
fn event_lasts_5_min_after_last_report() {
    require_fixtures!("manual-deep");
    let rs = reports("manual-deep");
    let last: DateTime<Utc> = rs.last().unwrap().report_time;
    let mut s = EventStore::new(false);
    play(&mut s, rs);
    assert_eq!(s.active(last + Duration::seconds(300)).len(), 1);
    assert!(s.active(last + Duration::seconds(301)).is_empty());
}

#[test]
fn training_is_filtered_by_setting() {
    require_fixtures!("manual-training");
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/manual-training");
    let m = &load_messages(&path).unwrap()[0];
    assert!(matches!(parse_message(m, false), Incoming::Ignored(_)));
    match parse_message(m, true) {
        Incoming::Report(r) => assert_eq!(r.status, TelegramStatus::Training),
        other => panic!("{other:?}"),
    }
}

#[test]
fn envelope_is_unwrapped() {
    require_fixtures!("manual-cancel");
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/manual-cancel");
    let raw = &load_messages(&path).unwrap()[0];
    let data: serde_json::Value = serde_json::from_str(raw).unwrap();
    let env = serde_json::json!({
        "v": 2, "id": null, "originalId": "x", "type": "eew", "telegramType": "VXSE45",
        "infoType": "発表", "telegramStatus": "通常", "at": "2026-11-01T01:00:05Z", "data": data
    });
    assert!(matches!(
        parse_message(&env.to_string(), false),
        Incoming::Report(_)
    ));
    let mut env = env;
    env["telegramType"] = "VXSE44".into();
    assert!(matches!(
        parse_message(&env.to_string(), false),
        Incoming::Ignored(_)
    ));
}

#[test]
fn two_events_are_kept_apart() {
    require_fixtures!("manual-two-events");
    let rs = reports("manual-two-events");
    let t = rs[1].report_time;
    let mut s = EventStore::new(false);
    play(&mut s, rs);
    let active = s.active(t);
    assert_eq!(active.len(), 2);
    assert!(!active[0].ever_warning);
    assert!(active[1].ever_warning);
}
