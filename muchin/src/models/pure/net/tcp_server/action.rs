use crate::automaton::{
    deserialize_rc_bytes, serialize_rc_bytes, Action, ActionKind, Redispatch, Timeout, Uid,
};
use crate::models::pure::net::tcp::action::TcpPollEvents;
use serde::{Deserialize, Serialize};
use std::rc::Rc;
use type_uuid::TypeUuid;

#[derive(Clone, PartialEq, Eq, TypeUuid, Serialize, Deserialize, Debug)]
#[uuid = "9bb1c88e-71c8-4a55-8074-cd3dd939a1fb"]
pub enum TcpServerAction {
    New {
        address: String,
        listener: Uid,
        max_connections: usize,
        on_success: Redispatch<Uid>,
        on_error: Redispatch<(Uid, String)>,
        on_new_connection: Redispatch<(Uid, Uid)>,
        on_connection_closed: Redispatch<(Uid, Uid)>,
        on_listener_closed: Redispatch<Uid>,
    },
    NewSuccess {
        listener: Uid,
    },
    NewError {
        listener: Uid,
        error: String,
    },
    Poll {
        uid: Uid,
        timeout: Timeout,
        on_success: Redispatch<Uid>,
        on_error: Redispatch<(Uid, String)>,
    },
    PollSuccess {
        uid: Uid,
        events: TcpPollEvents,
    },
    PollError {
        uid: Uid,
        error: String,
    },
    AcceptSuccess {
        connection: Uid,
    },
    AcceptTryAgain {
        connection: Uid,
    },
    AcceptError {
        connection: Uid,
        error: String,
    },
    Close {
        connection: Uid,
    },
    CloseEventNotify {
        connection: Uid,
    },
    CloseEventInternal {
        connection: Uid,
    },
    Send {
        uid: Uid,
        connection: Uid,
        #[serde(
            serialize_with = "serialize_rc_bytes",
            deserialize_with = "deserialize_rc_bytes"
        )]
        data: Rc<[u8]>,
        timeout: Timeout,
        on_success: Redispatch<Uid>,
        on_timeout: Redispatch<Uid>,
        on_error: Redispatch<(Uid, String)>,
    },
    SendSuccess {
        uid: Uid,
    },
    SendTimeout {
        uid: Uid,
    },
    SendError {
        uid: Uid,
        error: String,
    },
    Recv {
        uid: Uid,
        connection: Uid,
        count: usize, // number of bytes to read
        timeout: Timeout,
        on_success: Redispatch<(Uid, Vec<u8>)>,
        on_timeout: Redispatch<(Uid, Vec<u8>)>,
        on_error: Redispatch<(Uid, String)>,
    },
    RecvSuccess {
        uid: Uid,
        data: Vec<u8>,
    },
    RecvTimeout {
        uid: Uid,
        partial_data: Vec<u8>,
    },
    RecvError {
        uid: Uid,
        error: String,
    },
}

impl Action for TcpServerAction {
    const KIND: ActionKind = ActionKind::Pure;
}
