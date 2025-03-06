// Simulator that runs a mock network of nodes
//
// Time is "logical", we don't actually wait for anything to happen
// We call set_now to simulate the passage of time in single-step increments
//
// At each step, we deliver messages that are ready to be delivered.
// We process each message to completion, check timeouts, check block production eligibility, and finally advance the state of the simulation.

use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
};

use crate::*;
