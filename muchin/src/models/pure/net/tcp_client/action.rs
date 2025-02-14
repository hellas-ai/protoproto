use crate::automaton::{
    deserialize_rc_bytes, serialize_rc_bytes, Action, ActionKind, Redispatch, Timeout, Uid,
};
use crate::models::pure::net::tcp::action::TcpPollEvents;
use serde::{Deserialize, Serialize};
use std::rc::Rc;
use type_uuid::TypeUuid;

#[derive(Clone, PartialEq, Eq, TypeUuid, Serialize, Deserialize, Debug)]
#[uuid = "f15cd869-0966-4ab5-881c-530bc0fe95e6"]
pub enum TcpClientAction {
    Connect {
        connection: Uid,
        address: String,
        timeout: Timeout,
        on_success: Redispatch<Uid>,
        on_timeout: Redispatch<Uid>,
        on_error: Redispatch<(Uid, String)>,
        on_close: Redispatch<Uid>,
    },
    ConnectSuccess {
        connection: Uid,
    },
    ConnectTimeout {
        connection: Uid,
    },
    ConnectError {
        connection: Uid,
        error: String,
    },
    Poll {
        uid: Uid,
        timeout: Timeout,
        on_success: Redispatch<(Uid, TcpPollEvents)>,
        on_error: Redispatch<(Uid, String)>,
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

impl Action for TcpClientAction {
    const KIND: ActionKind = ActionKind::Pure;
}
