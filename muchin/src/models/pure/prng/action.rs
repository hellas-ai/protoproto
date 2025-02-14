use crate::automaton::{Action, ActionKind};
use serde::{Deserialize, Serialize};
use type_uuid::TypeUuid;

#[allow(dead_code)]
#[derive(Clone, PartialEq, Eq, TypeUuid, Serialize, Deserialize, Debug)]
#[uuid = "98e309cc-5a05-4a19-9eaf-03d6deedbf0b"]
pub enum PRNGPureAction {
    Reseed { seed: u64 },
}

impl Action for PRNGPureAction {
    const KIND: ActionKind = ActionKind::Pure;
}
