use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Default, Serialize, Deserialize, Debug)]
pub struct TimeState {
    now: Duration,
    tick: bool,
}

impl TimeState {
    pub fn now(&self) -> &Duration {
        &self.now
    }

    pub fn set_time(&mut self, time: Duration) {
        self.now = time;
    }

    pub fn tick(&mut self) -> bool {
        self.tick = !self.tick;
        self.tick
    }
}
