pub mod kmoni;
pub mod ws;

use crate::eew::report::{Incoming, Upstream};

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
