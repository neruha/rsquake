//! イベント状態。EventID ごとに最新報を持ち、報数・取消・表示終了を扱う。

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};

use super::expiry::expires_at;
use super::report::EewReport;

/// 取消済み・終了済みの EventID を覚えておく時間。
const FORGET_AFTER_SECS: i64 = 3600;

#[derive(Clone, Debug)]
pub struct EewEvent {
    pub latest: EewReport,
    /// 一度でも警報が出たか
    pub ever_warning: bool,
    /// 最初の報を受けた時刻
    pub first_received: DateTime<Utc>,
    /// 最後に報を採用した時刻
    pub last_updated: DateTime<Utc>,
    /// テストモードの時刻補正量
    pub time_offset: Duration,
    pub expires_at: DateTime<Utc>,
}

impl EewEvent {
    pub fn is_canceled(&self) -> bool {
        self.latest.is_cancel
    }

    pub fn is_active(&self, now: DateTime<Utc>) -> bool {
        now <= self.expires_at
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ApplyOutcome {
    New,
    Updated,
    /// 初めて警報になった
    BecameWarning,
    Canceled,
    Rejected(&'static str),
}

#[derive(Default)]
pub struct EventStore {
    events: HashMap<String, EewEvent>,
    /// 終わったイベントと、(忘れてよい時刻, 取消済みか)
    finished: HashMap<String, (DateTime<Utc>, bool)>,
    /// 終わったイベントの時刻補正量 (取消報を受けたときに使う)
    finished_offsets: HashMap<String, Duration>,
    /// テストモードの時刻補正をするか
    pub shift_times: bool,
}

impl EventStore {
    pub fn new(shift_times: bool) -> Self {
        Self {
            shift_times,
            ..Default::default()
        }
    }

    pub fn apply(&mut self, mut report: EewReport, now: DateTime<Utc>) -> ApplyOutcome {
        self.prune(now);
        let id = report.event_id.clone();
        let mut resurrect_offset = None;
        if let Some(&(_, canceled)) = self.finished.get(&id) {
            // 終了済みのイベントは通常報では戻さない。取消報だけは、見た警報が
            // 取り消されたことを伝えるために出す
            if !report.is_cancel || canceled {
                return ApplyOutcome::Rejected("終了済みのイベント");
            }
            self.finished.remove(&id);
            resurrect_offset = self.finished_offsets.remove(&id);
        }

        let Some(ev) = self.events.get_mut(&id) else {
            let offset = match resurrect_offset {
                Some(o) => o,
                None if self.shift_times => now - report.report_time,
                None => Duration::zero(),
            };
            report.shift(offset);
            let ever_warning = report.is_warning;
            let expires = expires_at(now);
            let outcome = if report.is_cancel {
                ApplyOutcome::Canceled
            } else {
                ApplyOutcome::New
            };
            self.events.insert(
                id,
                EewEvent {
                    latest: report,
                    ever_warning,
                    first_received: now,
                    last_updated: now,
                    time_offset: offset,
                    expires_at: expires,
                },
            );
            return outcome;
        };

        if ev.is_canceled() {
            return ApplyOutcome::Rejected("取消済みのイベント");
        }
        // 取消報は報数に関係なく適用する
        if !report.is_cancel && report.serial <= ev.latest.serial {
            return ApplyOutcome::Rejected("報数が古い");
        }
        report.shift(ev.time_offset);
        let became_warning = report.is_warning && !ev.ever_warning;
        let ever_warning = ev.ever_warning || report.is_warning;
        let expires = expires_at(now);
        let outcome = if report.is_cancel {
            ApplyOutcome::Canceled
        } else if became_warning {
            ApplyOutcome::BecameWarning
        } else {
            ApplyOutcome::Updated
        };
        ev.latest = report;
        ev.ever_warning = ever_warning;
        ev.last_updated = now;
        ev.expires_at = expires;
        outcome
    }

    fn finish(&mut self, id: String, now: DateTime<Utc>) {
        let (canceled, offset) = match self.events.remove(&id) {
            Some(e) => (e.is_canceled(), e.time_offset),
            None => (false, Duration::zero()),
        };
        self.finished_offsets.insert(id.clone(), offset);
        self.finished
            .insert(id, (now + Duration::seconds(FORGET_AFTER_SECS), canceled));
    }

    /// 表示終了したイベントを外し、古い記録を忘れる。変化があれば真。
    pub fn prune(&mut self, now: DateTime<Utc>) -> bool {
        let expired: Vec<String> = self
            .events
            .iter()
            .filter(|(_, e)| !e.is_active(now))
            .map(|(k, _)| k.clone())
            .collect();
        let changed = !expired.is_empty();
        for id in expired {
            self.finish(id, now);
        }
        self.finished.retain(|_, (forget_at, _)| now <= *forget_at);
        let finished = &self.finished;
        self.finished_offsets
            .retain(|k, _| finished.contains_key(k));
        changed
    }

    /// 有効なイベントを、最初に受けた順に返す。
    pub fn active(&self, now: DateTime<Utc>) -> Vec<&EewEvent> {
        let mut v: Vec<&EewEvent> = self.events.values().filter(|e| e.is_active(now)).collect();
        v.sort_by_key(|e| e.first_received);
        v
    }

    pub fn get(&self, id: &str) -> Option<&EewEvent> {
        self.events.get(id)
    }

    /// 条件に合う EventID のイベントと、終了済みの記録を消す (デバッグメニュー用)。
    pub fn remove_where(&mut self, f: impl Fn(&str) -> bool) {
        self.events.retain(|id, _| !f(id));
        self.finished.retain(|id, _| !f(id));
        self.finished_offsets.retain(|id, _| !f(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eew::report::{HypocenterKind, Quake, TelegramStatus};

    fn t(s: i64) -> DateTime<Utc> {
        "2026-01-01T00:00:00Z".parse::<DateTime<Utc>>().unwrap() + Duration::seconds(s)
    }

    fn report(id: &str, serial: u32, at: i64) -> EewReport {
        EewReport {
            event_id: id.into(),
            serial,
            status: TelegramStatus::Normal,
            report_time: t(at),
            is_cancel: false,
            is_last: false,
            is_warning: false,
            quake: Some(Quake {
                origin_time: Some(t(0)),
                arrival_time: Some(t(2)),
                kind: HypocenterKind::Normal,
                name: "x".into(),
                latitude: Some(35.0),
                longitude: Some(139.0),
                depth_km: Some(10.0),
                depth_over_700: false,
                magnitude: Some(4.0),
                magnitude_over_8: false,
            }),
            intensity: None,
        }
    }

    fn cancel(id: &str, serial: u32, at: i64) -> EewReport {
        let mut r = report(id, serial, at);
        r.is_cancel = true;
        r.quake = None;
        r
    }

    #[test]
    fn serial_must_increase() {
        let mut s = EventStore::new(false);
        assert_eq!(s.apply(report("a", 2, 5), t(5)), ApplyOutcome::New);
        assert!(matches!(
            s.apply(report("a", 2, 6), t(6)),
            ApplyOutcome::Rejected(_)
        ));
        assert!(matches!(
            s.apply(report("a", 1, 6), t(6)),
            ApplyOutcome::Rejected(_)
        ));
        assert_eq!(s.apply(report("a", 3, 7), t(7)), ApplyOutcome::Updated);
        assert_eq!(s.get("a").unwrap().latest.serial, 3);
    }

    #[test]
    fn cancel_ignores_serial_and_wins() {
        let mut s = EventStore::new(false);
        s.apply(report("a", 5, 5), t(5));
        assert_eq!(s.apply(cancel("a", 1, 6), t(6)), ApplyOutcome::Canceled);
        assert!(s.get("a").unwrap().is_canceled());
        assert!(matches!(
            s.apply(report("a", 9, 7), t(7)),
            ApplyOutcome::Rejected(_)
        ));
        assert!(s.get("a").unwrap().is_canceled());
    }

    #[test]
    fn became_warning_once() {
        let mut s = EventStore::new(false);
        s.apply(report("a", 1, 5), t(5));
        let mut w = report("a", 2, 6);
        w.is_warning = true;
        assert_eq!(s.apply(w.clone(), t(6)), ApplyOutcome::BecameWarning);
        w.serial = 3;
        assert_eq!(s.apply(w, t(7)), ApplyOutcome::Updated);
        // 警報が解除された報でも一度出た扱いは残る
        s.apply(report("a", 4, 8), t(8));
        assert!(s.get("a").unwrap().ever_warning);
        assert_eq!((s.get("a").unwrap().expires_at - t(8)).num_seconds(), 300);
    }

    #[test]
    fn expires_and_stays_finished() {
        let mut s = EventStore::new(false);
        s.apply(report("a", 1, 5), t(5));
        assert_eq!(s.active(t(305)).len(), 1);
        assert_eq!(s.active(t(306)).len(), 0);
        assert!(s.prune(t(306)));
        assert!(matches!(
            s.apply(report("a", 2, 306), t(306)),
            ApplyOutcome::Rejected(_)
        ));
        assert!(s.get("a").is_none());
    }

    #[test]
    fn stale_first_report_shows_for_5_min_after_receipt() {
        let mut s = EventStore::new(false);
        assert_eq!(s.apply(report("a", 1, 5), t(1000)), ApplyOutcome::New);
        assert_eq!(s.active(t(1300)).len(), 1);
        assert!(s.active(t(1301)).is_empty());
    }

    #[test]
    fn cancel_expires_5_min_after_receipt() {
        let mut s = EventStore::new(false);
        s.apply(report("a", 1, 5), t(5));
        s.apply(cancel("a", 2, 20), t(20));
        assert_eq!(s.active(t(320)).len(), 1);
        assert_eq!(s.active(t(321)).len(), 0);
    }

    #[test]
    fn test_mode_shifts_times() {
        let mut s = EventStore::new(true);
        // 1 時間前の電文を受ける
        assert_eq!(s.apply(report("a", 1, 5), t(3605)), ApplyOutcome::New);
        let e = s.get("a").unwrap();
        assert_eq!(e.latest.report_time, t(3605));
        assert_eq!(e.latest.quake.as_ref().unwrap().origin_time, Some(t(3600)));
        // 2 報目も同じ量だけずらす
        s.apply(report("a", 2, 8), t(3609));
        assert_eq!(s.get("a").unwrap().latest.report_time, t(3608));
    }

    #[test]
    fn forgets_finished_after_an_hour() {
        let mut s = EventStore::new(false);
        s.apply(report("a", 1, 5), t(5));
        s.prune(t(306));
        s.prune(t(306 + 3601));
        assert!(s.finished.is_empty());
    }

    #[test]
    fn cancel_after_expiry_is_shown_once() {
        let mut s = EventStore::new(false);
        s.apply(report("a", 1, 5), t(5));
        s.prune(t(306));
        assert_eq!(s.apply(cancel("a", 2, 600), t(600)), ApplyOutcome::Canceled);
        assert!(s.get("a").unwrap().is_canceled());
        assert_eq!(s.active(t(899)).len(), 1);
        s.prune(t(901));
        assert!(matches!(
            s.apply(cancel("a", 3, 910), t(910)),
            ApplyOutcome::Rejected(_)
        ));
        assert!(matches!(
            s.apply(report("a", 9, 910), t(910)),
            ApplyOutcome::Rejected(_)
        ));
    }
}
