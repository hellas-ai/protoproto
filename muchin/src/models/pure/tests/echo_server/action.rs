use crate::automaton::{Action, ActionKind, Uid};
use serde::{Deserialize, Serialize};
use type_uuid::TypeUuid;

#[derive(Clone, PartialEq, Eq, TypeUuid, Serialize, Deserialize, Debug)]
#[uuid = "04f45d4b-7484-4fe5-a6b2-651ef7e58ca9"]
pub enum EchoServerAction {
    Tick,
    PollSuccess { uid: Uid },
    PollError { uid: Uid, error: String },
    InitSuccess { instance: Uid },
    InitError { instance: Uid, error: String },
    InitListenerSuccess { listener: Uid },
    InitListenerError { listener: Uid, error: String },
    ListenerCloseEvent { listener: Uid },
    ConnectionEvent { listener: Uid, connection: Uid },
    CloseEvent { listener: Uid, connection: Uid },
    SendSuccess { uid: Uid },
    SendTimeout { uid: Uid },
    SendError { uid: Uid, error: String },
    RecvSuccess { uid: Uid, data: Vec<u8> },
    RecvTimeout { uid: Uid, partial_data: Vec<u8> },
    RecvError { uid: Uid, error: String },
}

impl Action for EchoServerAction {
    const KIND: ActionKind = ActionKind::Pure;
}
