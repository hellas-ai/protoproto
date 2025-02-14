use crate::automaton::{Action, ActionKind, Uid};
use crate::models::pure::net::tcp::action::TcpPollEvents;
use serde::{Deserialize, Serialize};
use type_uuid::TypeUuid;

#[derive(Clone, PartialEq, Eq, TypeUuid, Serialize, Deserialize, Debug)]
#[uuid = "6f8ab34b-2f20-49ff-a4a5-3573ff86fc61"]
pub enum EchoClientAction {
    Tick,
    PollSuccess { uid: Uid, events: TcpPollEvents },
    PollError { uid: Uid, error: String },
    InitSuccess { instance: Uid },
    InitError { instance: Uid, error: String },
    ConnectSuccess { connection: Uid },
    ConnectTimeout { connection: Uid },
    ConnectError { connection: Uid, error: String },
    CloseEvent { connection: Uid },
    SendSuccess { uid: Uid },
    SendTimeout { uid: Uid },
    SendError { uid: Uid, error: String },
    RecvSuccess { uid: Uid, data: Vec<u8> },
    RecvTimeout { uid: Uid, partial_data: Vec<u8> },
    RecvError { uid: Uid, error: String },
}

impl Action for EchoClientAction {
    const KIND: ActionKind = ActionKind::Pure;
}
