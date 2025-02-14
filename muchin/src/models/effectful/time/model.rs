use super::{action::TimeEffectfulAction, state::TimeState};
use crate::automaton::{
    Dispatcher, Effectful, EffectfulModel, ModelState, RegisterModel, RunnerBuilder,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// This is an `EffectfulModel` responsible for handling time-related actions.
//
// As of now, it supports one action: getting the system time.
//
// The `GetSystemTime` action gets the current system time since the UNIX_EPOCH
// and dispatches the result back as an `PureAction` defined by the caller.

impl RegisterModel for TimeState {
    fn register<Substate: ModelState>(builder: RunnerBuilder<Substate>) -> RunnerBuilder<Substate> {
        builder.model_effectful(Effectful::<Self>(Self()))
    }
}

impl EffectfulModel for TimeState {
    type Action = TimeEffectfulAction;

    fn process_effectful(&mut self, action: Self::Action, dispatcher: &mut Dispatcher) {
        match action {
            TimeEffectfulAction::GetSystemTime { uid, on_result } => {
                let result = if dispatcher.is_replayer() {
                    Duration::default() // Ignored
                } else {
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .expect("System clock set before UNIX_EPOCH")
                };

                dispatcher.dispatch_back(&on_result, (uid, result));
            }
        }
    }
}
