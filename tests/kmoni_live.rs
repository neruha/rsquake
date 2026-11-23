//! 本物の強震モニタにつなぐ確認用テスト。通常のテストでは走らせない。
//!
//! `cargo test --test kmoni_live -- --ignored --nocapture`

use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use rsquake::geo::stations::StationList;
use rsquake::net::NetEvent;
use rsquake::net::kmoni::{self, KmoniEvent, KmoniOptions};

#[test]
#[ignore = "NIED に実際に要求を出す"]
fn fetches_real_frames() {
    let stations = Arc::new(StationList::bundled());
    let (tx, rx) = mpsc::channel();
    kmoni::spawn(KmoniOptions::new(stations), tx, egui::Context::default());

    let mut frames = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(25);
    while std::time::Instant::now() < deadline && frames.len() < 4 {
        if let Ok(NetEvent::Kmoni(e)) = rx.recv_timeout(Duration::from_millis(500)) {
            match e {
                KmoniEvent::Frame(f) => {
                    let n = f.colors.iter().flatten().count();
                    println!("frame {} 観測点 {n}", f.time.format("%H:%M:%S"));
                    frames.push((f.time, n));
                }
                KmoniEvent::Failed(why) => println!("failed: {why}"),
            }
        }
    }
    assert!(frames.len() >= 3, "{frames:?}");
    assert!(frames.iter().all(|(_, n)| *n > 1_500));
    // 毎秒 1 枚ずつ進む
    for w in frames.windows(2) {
        assert!(w[1].0 > w[0].0, "{frames:?}");
    }
}
