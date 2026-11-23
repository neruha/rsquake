//! Wolfx 用の WebSocket 接続を、ローカルのサーバーで確かめる。

mod common;

use std::sync::mpsc;
use std::time::Duration;

use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use rsquake::config::{Config, EewSource};
use rsquake::eew::report::{Incoming, Upstream};
use rsquake::geo::topo::Topo;
use rsquake::net::ws::{self, WsOptions, WsSupervisor};
use rsquake::net::{LinkState, NetEvent};
use tokio_tungstenite::tungstenite::Message;

fn codes() -> std::sync::Arc<std::collections::HashMap<String, String>> {
    std::sync::Arc::new(
        Topo::parse(include_bytes!("../assets/geojson/eew-prefregion.json"))
            .unwrap()
            .region_name_to_code(),
    )
}

/// フィクスチャの報を、報の時刻だけ差し替えて返す。
fn eew_json(announced: chrono::DateTime<Utc>, serial: u32) -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wolfx/kumamoto-warning-20261119/003.json"
    );
    let mut v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let jst = announced.with_timezone(&chrono::FixedOffset::east_opt(9 * 3600).unwrap());
    v["AnnouncedTime"] = jst.format("%Y/%m/%d %H:%M:%S").to_string().into();
    v["Serial"] = serial.into();
    v.to_string()
}

fn wolfx_opts(port: u16) -> WsOptions {
    WsOptions {
        url: format!("ws://127.0.0.1:{port}"),
        source: EewSource::Wolfx,
        accept_non_normal: false,
        test_mode: false,
        log_dir: None,
        region_codes: codes(),
        health_interval: None,
    }
}

fn collect(
    rx: &mpsc::Receiver<NetEvent>,
    secs: u64,
    until: impl Fn(&[NetEvent]) -> bool,
) -> Vec<NetEvent> {
    let mut seen = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(secs);
    while std::time::Instant::now() < deadline && !until(&seen) {
        if let Ok(e) = rx.recv_timeout(Duration::from_millis(100)) {
            seen.push(e);
        }
    }
    seen
}

fn reports(seen: &[NetEvent]) -> Vec<u32> {
    seen.iter()
        .filter_map(|e| match e {
            NetEvent::Message(Incoming::Report(r)) => Some(r.serial),
            _ => None,
        })
        .collect()
}

/// 接続したら `query_jmaeew` を送る。最初の報が古ければ捨て、新しい報と、そのあとの報は通す。
/// heartbeat は無視し、接続できたら上流の状態は不明にする。
#[test]
fn wolfx_query_stale_first_report_and_heartbeat() {
    require_fixtures!("wolfx/kumamoto-warning-20261119/003.json");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let listener = rt
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    let (got_tx, got_rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        rt.block_on(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
            ws.send(Message::text(
                r#"{"type":"heartbeat","ver":1,"id":"a","timestamp":1}"#,
            ))
            .await
            .unwrap();
            // 受け取ったテキストを記録する (query_jmaeew が来たら応答する)
            while let Some(Ok(m)) = ws.next().await {
                if let Message::Text(t) = m {
                    got_tx.send(t.to_string()).ok();
                    if t.as_str() == "query_jmaeew" {
                        // 古い 1 件 (捨てられる)
                        let old = Utc::now() - chrono::Duration::minutes(10);
                        ws.send(Message::text(eew_json(old, 1))).await.unwrap();
                        // 以降のリアルタイムの報 (最初の 1 件ではないので古くても通る必要は無いが、新しい)
                        ws.send(Message::text(eew_json(Utc::now(), 2)))
                            .await
                            .unwrap();
                        ws.send(Message::text(r#"{"type":"pong","timestamp":1}"#))
                            .await
                            .unwrap();
                    }
                }
            }
        });
    });

    let (tx, rx) = mpsc::channel();
    let handle = ws::spawn(wolfx_opts(port), tx, egui::Context::default());
    let seen = collect(&rx, 10, |s| reports(s).contains(&2));
    handle.stop();

    assert_eq!(
        got_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        "query_jmaeew"
    );
    assert_eq!(reports(&seen), vec![2], "{seen:#?}");
    assert!(
        seen.iter()
            .any(|e| matches!(e, NetEvent::Link(LinkState::Connected)))
    );
    assert!(
        seen.iter()
            .any(|e| matches!(e, NetEvent::Upstream(Upstream::Unknown)))
    );
}

/// 接続後、最初の報が新しければそのまま通す (進行中の地震を拾う)。
#[test]
fn wolfx_fresh_first_report_passes() {
    require_fixtures!("wolfx/kumamoto-warning-20261119/003.json");
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
            let (tcp, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
            while let Some(Ok(m)) = ws.next().await {
                if matches!(&m, Message::Text(t) if t.as_str() == "query_jmaeew") {
                    let t = Utc::now() - chrono::Duration::seconds(60);
                    ws.send(Message::text(eew_json(t, 7))).await.unwrap();
                }
            }
        });
    });
    let (tx, rx) = mpsc::channel();
    let handle = ws::spawn(wolfx_opts(port), tx, egui::Context::default());
    let seen = collect(&rx, 10, |s| !reports(s).is_empty());
    handle.stop();
    assert_eq!(reports(&seen), vec![7], "{seen:#?}");
}

/// 受信元を切り替えると、古いスレッドは止まり、そのあとは古い `NetEvent` が届かない。
#[test]
fn supervisor_switches_source_and_old_thread_goes_quiet() {
    require_fixtures!("wolfx/kumamoto-warning-20261119/003.json");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    // 古い受信元 (DMDATA 側): 接続を受けて黙って待つ。切られたら記録する
    let old = rt
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .unwrap();
    let old_port = old.local_addr().unwrap().port();
    let new = rt
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .unwrap();
    let new_port = new.local_addr().unwrap().port();
    let (closed_tx, closed_rx) = mpsc::channel::<()>();
    std::thread::spawn(move || {
        rt.block_on(async move {
            tokio::spawn(async move {
                let (tcp, _) = old.accept().await.unwrap();
                let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
                while let Some(Ok(_)) = ws.next().await {}
                closed_tx.send(()).ok();
            });
            let (tcp, _) = new.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
            while let Some(Ok(m)) = ws.next().await {
                if matches!(&m, Message::Text(t) if t.as_str() == "query_jmaeew") {
                    ws.send(Message::text(eew_json(Utc::now(), 3)))
                        .await
                        .unwrap();
                }
            }
        });
    });

    let mut config = Config {
        server: format!("ws://127.0.0.1:{old_port}"),
        ..Config::default()
    };
    config.wolfx.url = format!("ws://127.0.0.1:{new_port}");
    let (tx, rx) = mpsc::channel();
    let mut sup = WsSupervisor::start(&config, tx, egui::Context::default(), codes());
    let seen = collect(&rx, 5, |s| {
        s.iter()
            .any(|e| matches!(e, NetEvent::Link(LinkState::Connected)))
    });
    assert!(
        seen.iter()
            .any(|e| matches!(e, NetEvent::Link(LinkState::Connected))),
        "{seen:#?}"
    );

    assert!(
        !sup.reconnect_if_changed(&config),
        "変更が無ければつなぎ直さない"
    );
    config.source = EewSource::Wolfx;
    assert!(sup.reconnect_if_changed(&config));

    // 古い接続は閉じられる
    closed_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("古い接続が閉じない");
    // 新しい受信元の報が届く
    let seen = collect(&rx, 10, |s| reports(s).contains(&3));
    assert_eq!(reports(&seen), vec![3], "{seen:#?}");
    // 古い世代の Link(Disconnected) などは、切り替えのあとに届かない
    assert!(
        !seen
            .iter()
            .any(|e| matches!(e, NetEvent::Link(LinkState::Disconnected))),
        "{seen:#?}"
    );
}
