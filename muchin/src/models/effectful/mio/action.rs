use crate::automaton::{Action, ActionKind, Redispatch, Timeout, Uid};
use serde::{Deserialize, Serialize};
use std::rc::Rc;
use type_uuid::TypeUuid;

/// `MioAction` is an enum representing various I/O related operations
/// that can be performed. These actions are dispatched for handling by the
/// `MioState` model. Each action variant includes the necessary parameters
/// for the operation and a callback `Redispatch` to dispatch the result
/// of the operation back to the caller Model.
///
/// Operations include:
/// - Poll creation, registration, and deregistration.
/// - TCP server and connection management: listen, accept, connect, close.
/// - Data transmission over TCP: write, read.
/// - Miscellaneous: event creation, polling events, getting peer address.
///
/// Note: `Uid` is used to uniquely identify instances of various Model-
/// specific objects like polls, connections, events etc.

#[derive(Clone, PartialEq, Eq, TypeUuid, Serialize, Deserialize, Debug)]
#[uuid = "6ade1356-d5fe-4c28-8fa9-fe4ee2fffc5f"]
pub enum MioEffectfulAction {
    PollCreate {
        poll: Uid,
        on_success: Redispatch<Uid>,
        on_error: Redispatch<(Uid, String)>,
    },
    PollRegisterTcpServer {
        poll: Uid,     // created by PollCreate
        listener: Uid, // created by TcpListen
        on_success: Redispatch<Uid>,
        on_error: Redispatch<(Uid, String)>,
    },
    PollRegisterTcpConnection {
        poll: Uid,       // created by PollCreate
        connection: Uid, // created by TcpAccept/TcpConnect
        on_success: Redispatch<Uid>,
        on_error: Redispatch<(Uid, String)>,
    },
    PollDeregisterTcpConnection {
        poll: Uid,       // created by PollCreate
        connection: Uid, // created by TcpAccept/TcpConnect
        on_success: Redispatch<Uid>,
        on_error: Redispatch<(Uid, String)>,
    },
    PollEvents {
        uid: Uid,    // passed back to call-back action to identify the request
        poll: Uid,   // created by PollCreate
        events: Uid, // created by EventsCreate
        timeout: Timeout,
        on_success: Redispatch<(Uid, Vec<MioEvent>)>,
        on_interrupted: Redispatch<Uid>,
        on_error: Redispatch<(Uid, String)>,
    },
    EventsCreate {
        uid: Uid,
        capacity: usize,
        on_success: Redispatch<Uid>,
    },
    TcpListen {
        listener: Uid,
        address: String,
        on_success: Redispatch<Uid>,
        on_error: Redispatch<(Uid, String)>,
    },
    TcpAccept {
        connection: Uid,
        listener: Uid, // created by TcpListen
        on_success: Redispatch<Uid>,
        on_would_block: Redispatch<Uid>,
        on_error: Redispatch<(Uid, String)>,
    },
    TcpConnect {
        connection: Uid,
        address: String,
        on_success: Redispatch<Uid>,
        on_error: Redispatch<(Uid, String)>,
    },
    TcpClose {
        connection: Uid, // created by TcpAccept/TcpConnect
        on_success: Redispatch<Uid>,
    },
    TcpWrite {
        uid: Uid,        // passed back to call-back action to identify the request
        connection: Uid, // created by TcpAccept/TcpConnect

        // Strictly speaking, we should pass a copy here instead of referencing memory,
        // but the Rc guarantees immutability, allowing safe and efficient data sharing.
        #[serde(
            serialize_with = "crate::automaton::serialize_rc_bytes",
            deserialize_with = "crate::automaton::deserialize_rc_bytes"
        )]
        data: Rc<[u8]>,
        on_success: Redispatch<Uid>,
        on_success_partial: Redispatch<(Uid, usize)>,
        on_interrupted: Redispatch<Uid>,
        on_would_block: Redispatch<Uid>,
        on_error: Redispatch<(Uid, String)>,
    },
    TcpRead {
        uid: Uid,        // passed back to call-back action to identify the request
        connection: Uid, // created by TcpAccept/TcpConnect
        len: usize,      // max number of bytes to read
        on_success: Redispatch<(Uid, Vec<u8>)>,
        on_success_partial: Redispatch<(Uid, Vec<u8>)>,
        on_interrupted: Redispatch<Uid>,
        on_would_block: Redispatch<Uid>,
        on_error: Redispatch<(Uid, String)>,
    },
    TcpGetPeerAddress {
        connection: Uid, // created by TcpAccept/TcpConnect
        on_success: Redispatch<(Uid, String)>,
        on_error: Redispatch<(Uid, String)>,
    },
}

impl Action for MioEffectfulAction {
    const KIND: ActionKind = ActionKind::Effectful;
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum TcpWriteResult {
    WrittenAll,
    WrittenPartial(usize),
    Interrupted,
    WouldBlock,
    Error(String),
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum TcpReadResult {
    ReadAll(Vec<u8>),
    ReadPartial(Vec<u8>),
    Interrupted,
    WouldBlock,
    Error(String),
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum TcpAcceptResult {
    Success,
    WouldBlock,
    Error(String),
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub struct MioEvent {
    pub token: Uid,
    pub readable: bool,
    pub writable: bool,
    pub error: bool,
    pub read_closed: bool,
    pub write_closed: bool,
    pub priority: bool,
    pub aio: bool,
    pub lio: bool,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum PollResult {
    Events(Vec<MioEvent>),
    Interrupted,
    Error(String),
}
