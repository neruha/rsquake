//! ローカルに WebSocket サーバーを立て、接続・status・envelope・再接続を確かめる。

mod common;

use std::sync::mpsc;
use std::time::Duration;

use futures_util::SinkExt;
use rsquake::eew::report::{Incoming, Upstream};
use rsquake::net::ws::{self, WsOptions};
use rsquake::net::{LinkState, NetEvent};
use tokio_tungstenite::tungstenite::Message;

fn envelope() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/manual-cancel/001.json"
    );
    let data: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    serde_json::json!({
        "v": 2, "id": null, "originalId": "x", "type": "eew", "telegramType": "VXSE45",
        "infoType": "発表", "telegramStatus": "通常", "at": "2026-11-01T01:00:05Z", "data": data
    })
    .to_string()
}

#[test]
fn connects_reads_status_and_reconnects() {
    require_fixtures!("manual-cancel");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let listener = rt
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        rt.block_on(async move {
            for _ in 0..2 {
                let (tcp, _) = listener.accept().await.unwrap();
                let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
                let status = r#"{"v":2,"type":"status","upstream":"connected","database":"connected","at":"x"}"#;
                ws.send(Message::text(status)).await.unwrap();
                ws.send(Message::text(envelope())).await.unwrap();
                ws.close(None).await.ok();
            }
        });
    });

    let (tx, rx) = mpsc::channel();
    ws::spawn(
        WsOptions {
            url: format!("ws://127.0.0.1:{port}/ws?name=rsquake&v=2&topics=eew"),
            source: rsquake::config::EewSource::Dmdata,
            accept_non_normal: false,
            test_mode: false,
            log_dir: None,
            region_codes: Default::default(),
            health_interval: None,
        },
        tx,
        egui::Context::default(),
    );

    let mut seen = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut connects = 0;
    let mut reports = 0;
    while std::time::Instant::now() < deadline && (connects < 2 || reports < 2) {
        if let Ok(e) = rx.recv_timeout(Duration::from_millis(200)) {
            match &e {
                NetEvent::Link(LinkState::Connected) => connects += 1,
                NetEvent::Message(Incoming::Report(_)) => reports += 1,
                _ => {}
            }
            seen.push(format!("{e:?}").chars().take(60).collect::<String>());
        }
    }
    assert_eq!(connects, 2, "{seen:#?}");
    assert_eq!(reports, 2, "{seen:#?}");
    assert!(
        seen.iter()
            .any(|s| s.contains(&format!("{:?}", Upstream::Connected)))
    );
    assert!(seen.iter().any(|s| s.contains("Disconnected")));
}
