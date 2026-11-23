//! 複数 EEW の切り替えを、フィクスチャのリプレイで確かめる。

mod common;

use std::path::Path;

use chrono::{DateTime, Duration, Utc};
use rsquake::dev::replay::load_messages;
use rsquake::eew::display::DisplayState;
use rsquake::eew::report::{EewReport, Incoming, parse_message};
use rsquake::eew::store::EventStore;

fn reports(dir: &str) -> Vec<EewReport> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(dir);
    load_messages(&path)
        .unwrap()
        .iter()
        .map(|m| match parse_message(m, true) {
            Incoming::Report(r) => *r,
            other => panic!("{other:?}"),
        })
        .collect()
}

struct Sim {
    store: EventStore,
    display: DisplayState,
    now: DateTime<Utc>,
}

impl Sim {
    fn new(start: DateTime<Utc>) -> Self {
        Self {
            store: EventStore::new(false),
            display: DisplayState::default(),
            now: start,
        }
    }

    /// 時刻を `now` まで進めながら 1 秒ごとに更新し、報を受ける。
    fn advance_to(&mut self, to: DateTime<Utc>) {
        while self.now < to {
            self.now += Duration::seconds(1);
            self.tick();
        }
    }

    fn tick(&mut self) -> Option<String> {
        self.store.prune(self.now);
        let active = self.store.active(self.now);
        let ids: Vec<&str> = active.iter().map(|e| e.latest.event_id.as_str()).collect();
        self.display
            .update(&ids, Duration::seconds(5), self.now)
            .map(str::to_string)
    }

    fn receive(&mut self, r: EewReport) -> Option<String> {
        let id = r.event_id.clone();
        let outcome = self.store.apply(r, self.now);
        self.display.notify(&id, &outcome);
        self.tick()
    }
}

#[test]
fn two_events_alternate() {
    require_fixtures!("manual-two-events");
    let rs = reports("manual-two-events");
    let (a, b) = (rs[0].event_id.clone(), rs[1].event_id.clone());
    let mut sim = Sim::new(rs[0].report_time);
    assert_eq!(sim.receive(rs[0].clone()).as_deref(), Some(a.as_str()));
    sim.advance_to(rs[1].report_time);
    // 2 つ目が発生したので割り込む
    assert_eq!(sim.receive(rs[1].clone()).as_deref(), Some(b.as_str()));
    let started = sim.now;
    sim.advance_to(started + Duration::seconds(4));
    assert_eq!(sim.tick().as_deref(), Some(b.as_str()));
    sim.advance_to(started + Duration::seconds(5));
    assert_eq!(sim.tick().as_deref(), Some(a.as_str()));
    sim.advance_to(started + Duration::seconds(10));
    assert_eq!(sim.tick().as_deref(), Some(b.as_str()));
}

#[test]
fn three_events_with_first_warning_interrupt() {
    require_fixtures!("manual-three-events");
    let rs = reports("manual-three-events");
    let ids: Vec<String> = rs.iter().map(|r| r.event_id.clone()).collect();
    let (a, b, c) = (&ids[0], &ids[1], &ids[2]);
    assert_eq!(ids[3], *a);
    let mut sim = Sim::new(rs[0].report_time);

    assert_eq!(sim.receive(rs[0].clone()).as_deref(), Some(a.as_str()));
    sim.advance_to(rs[1].report_time);
    assert_eq!(sim.receive(rs[1].clone()).as_deref(), Some(b.as_str()));
    // b の表示中に c が発生しても、b を途中で切らない
    sim.advance_to(rs[2].report_time);
    assert_eq!(sim.receive(rs[2].clone()).as_deref(), Some(b.as_str()));
    // 割り込みの順に、b、c と 5 秒ずつ出る
    let seen: Vec<String> = (0..12)
        .map(|_| {
            sim.advance_to(sim.now + Duration::seconds(1));
            sim.tick().unwrap()
        })
        .collect();
    let first_c = seen.iter().position(|id| id == c).expect("c が出る");
    assert!(seen[..first_c].iter().all(|id| id == b));
    assert!(
        seen[first_c..first_c + 5].iter().all(|id| id == c),
        "{seen:?}"
    );

    // a で初めて警報が出ると、いま何を出していても割り込む (割り込み中なら 1 間隔待つ)
    sim.advance_to(rs[3].report_time.max(sim.now + Duration::seconds(10)));
    sim.receive(rs[3].clone());
    let mut shown = None;
    for _ in 0..6 {
        sim.advance_to(sim.now + Duration::seconds(1));
        shown = sim.tick();
        if shown.as_deref() == Some(a.as_str()) {
            break;
        }
    }
    assert_eq!(shown.as_deref(), Some(a.as_str()));
}

#[test]
fn cancel_interrupts_and_event_leaves_after_expiry() {
    require_fixtures!("manual-cancel");
    let rs = reports("manual-cancel");
    let id = rs[0].event_id.clone();
    let mut sim = Sim::new(rs[0].report_time);
    sim.receive(rs[0].clone());
    sim.advance_to(rs[2].report_time);
    assert_eq!(sim.receive(rs[2].clone()).as_deref(), Some(id.as_str()));
    sim.advance_to(sim.now + Duration::seconds(301));
    assert_eq!(sim.tick(), None);
}
