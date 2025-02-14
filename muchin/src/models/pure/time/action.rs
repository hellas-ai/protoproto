use crate::automaton::{Action, ActionKind, Uid};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use type_uuid::TypeUuid;

#[derive(Clone, PartialEq, Eq, TypeUuid, Serialize, Deserialize, Debug)]
#[uuid = "1911e66d-e0e3-4efc-8952-c62f583059f6"]
pub enum TimeAction {
    UpdateCurrentTime,
    GetSystemTimeResult { uid: Uid, result: Duration },
}

impl Action for TimeAction {
    const KIND: ActionKind = ActionKind::Pure;
}
