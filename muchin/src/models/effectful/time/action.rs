use crate::automaton::{Action, ActionKind, Redispatch, Uid};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use type_uuid::TypeUuid;

#[derive(Clone, PartialEq, Eq, TypeUuid, Serialize, Deserialize, Debug)]
#[uuid = "3221c0d5-02f5-4ed6-bf79-29f40c5619f0"]
pub enum TimeEffectfulAction {
    GetSystemTime {
        uid: Uid,
        on_result: Redispatch<(Uid, Duration)>,
    },
}

impl Action for TimeEffectfulAction {
    const KIND: ActionKind = ActionKind::Effectful;
}
