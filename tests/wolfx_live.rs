//! 本物の Wolfx (`wss://ws-api.wolfx.jp/jma_eew`) につなぐ確認用テスト。通常のテストでは走らせない。
//!
//! `cargo test --test wolfx_live -- --ignored --nocapture`
//!
//! TLS でつながること、`query_jmaeew` と `ping` に応答があることを確かめる。
//! 最新の報は古いので捨てられる (進行中の地震が無ければ報は 0 件でよい)。

use std::sync::mpsc;
use std::time::Duration;

use rsquake::config::{Config, EewSource};
use rsquake::net::ws::{self, WsOptions};
use rsquake::net::{LinkState, NetEvent};

#[test]
#[ignore = "Wolfx に実際に接続する"]
fn connects_over_tls() {
    let config = Config {
        source: EewSource::Wolfx,
        ..Config::default()
    };
    let (tx, rx) = mpsc::channel();
    let region_codes = Default::default();
    let handle = ws::spawn(
        WsOptions::from_config(&config, region_codes),
        tx,
        egui::Context::default(),
    );
    let mut connected = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while std::time::Instant::now() < deadline {
        if let Ok(e) = rx.recv_timeout(Duration::from_millis(200)) {
            println!("{e:?}");
            if matches!(e, NetEvent::Link(LinkState::Connected)) {
                connected = true;
                break;
            }
        }
    }
    // 接続後の応答 (heartbeat / query_jmaeew) を少し待つ
    std::thread::sleep(Duration::from_secs(3));
    while let Ok(e) = rx.try_recv() {
        println!("{e:?}");
    }
    handle.stop();
    assert!(connected, "wss://ws-api.wolfx.jp/jma_eew につながらない");
}
