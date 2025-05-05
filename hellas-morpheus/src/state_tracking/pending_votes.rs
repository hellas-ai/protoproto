use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};
use crate::*;

/// Pending voting flags for a view
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct PendingVotes {
    pub tr_1: BTreeMap<BlockKey, bool>,
    pub tr_2: BTreeMap<BlockKey, bool>,
    pub lead_1: BTreeMap<BlockKey, bool>,
    pub lead_2: BTreeMap<BlockKey, bool>,
    pub dirty: bool,
}