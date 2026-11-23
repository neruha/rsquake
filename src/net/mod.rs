pub mod kmoni;
pub mod ws;

use crate::eew::report::{Incoming, Upstream};

/// TLS (wss / https) の暗号プロバイダを入れる。何度呼んでもよい (入っていれば何もしない)。
/// reqwest のクライアントを作る前と、wss につなぐ前に呼ぶ。
pub fn install_tls_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkState {
    Connecting,
    Connected,
    Disconnected,
}

#[derive(Debug)]
pub enum NetEvent {
    Link(LinkState),
    Upstream(Upstream),
    Message(Incoming),
    Kmoni(kmoni::KmoniEvent),
}
