//! 接続後に proxy の上流が切れたことを `GET /health` で拾えるかを、ローカルのサーバーで確かめる。

use std::sync::mpsc;
use std::time::Duration;

use futures_util::SinkExt;
use rsquake::config::EewSource;
use rsquake::eew::report::Upstream;
use rsquake::net::NetEvent;
use rsquake::net::ws::{self, WsOptions};
use tokio::io::AsyncWriteExt;
use tokio_tungstenite::tungstenite::Message;

/// 同じポートで WebSocket と `GET /health` を受ける。
/// `/health` は常に上流切断を返し、WebSocket は接続直後に「上流は接続中」と送る。
fn serve() -> u16 {
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
            let mut sockets = Vec::new();
            loop {
                let (mut tcp, _) = listener.accept().await.unwrap();
                let mut head = [0u8; 16];
                let n = tcp.peek(&mut head).await.unwrap_or(0);
                if head[..n].starts_with(b"GET /health") {
                    let body = r#"{"status":"degraded","database":"connected","upstream":"disconnected"}"#;
                    let res = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    tcp.write_all(res.as_bytes()).await.ok();
                    tcp.shutdown().await.ok();
                } else {
                    let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
                    let status = r#"{"v":2,"type":"status","upstream":"connected","database":"connected","at":"x"}"#;
                    ws.send(Message::text(status)).await.unwrap();
                    // 接続は開けたままにする
                    sockets.push(ws);
                }
            }
        });
    });
    port
}

fn opts(port: u16, health: Option<Duration>) -> WsOptions {
    WsOptions {
        url: format!("ws://127.0.0.1:{port}/ws?name=rsquake&v=2&topics=eew"),
        source: EewSource::Dmdata,
        accept_non_normal: false,
        test_mode: false,
        log_dir: None,
        region_codes: Default::default(),
        health_interval: health,
    }
}

fn upstreams(rx: &mpsc::Receiver<NetEvent>, secs: u64, want: usize) -> Vec<Upstream> {
    let mut out = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(secs);
    while std::time::Instant::now() < deadline && out.len() < want {
        if let Ok(NetEvent::Upstream(u)) = rx.recv_timeout(Duration::from_millis(100)) {
            out.push(u);
        }
    }
    out
}

#[test]
fn health_reports_upstream_going_down_after_connect() {
    let port = serve();
    let (tx, rx) = mpsc::channel();
    let handle = ws::spawn(
        opts(port, Some(Duration::from_millis(300))),
        tx,
        egui::Context::default(),
    );
    // 接続直後の status → しばらくして /health の結果
    let got = upstreams(&rx, 10, 2);
    handle.stop();
    assert_eq!(got, vec![Upstream::Connected, Upstream::Disconnected]);
}

#[test]
fn health_is_not_polled_when_disabled() {
    let port = serve();
    let (tx, rx) = mpsc::channel();
    let handle = ws::spawn(opts(port, None), tx, egui::Context::default());
    let got = upstreams(&rx, 2, 2);
    handle.stop();
    assert_eq!(
        got,
        vec![Upstream::Connected],
        "status だけで、/health は見ない"
    );
}
