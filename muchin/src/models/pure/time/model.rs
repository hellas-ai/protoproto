use super::{action::TimeAction, state::TimeState};
use crate::automaton::{
    Dispatcher, ModelState, PureModel, RegisterModel, RunnerBuilder, State, Timeout,
    TimeoutAbsolute, Uid,
};
use crate::callback;
use crate::models::effectful::time::{
    action::TimeEffectfulAction, state::TimeState as TimeStateEffectful,
};
use std::time::Duration;

impl RegisterModel for TimeState {
    fn register<Substate: ModelState>(builder: RunnerBuilder<Substate>) -> RunnerBuilder<Substate> {
        builder
            .register::<TimeStateEffectful>()
            .model_pure::<Self>()
    }
}

pub fn update_time<Substate: ModelState>(
    state: &mut State<Substate>,
    dispatcher: &mut Dispatcher,
) -> bool {
    let tick = state.substate_mut::<TimeState>().tick();

    if tick {
        dispatcher.dispatch(TimeAction::UpdateCurrentTime);
    }

    return tick;
}

pub fn get_current_time<Substate: ModelState>(state: &State<Substate>) -> u128 {
    state.substate::<TimeState>().now().as_millis()
}

pub fn get_timeout_absolute<Substate: ModelState>(
    state: &State<Substate>,
    timeout: Timeout,
) -> TimeoutAbsolute {
    // Convert relative the timeout we passed to absolute timeout by adding the current time
    match timeout {
        Timeout::Millis(ms) => {
            TimeoutAbsolute::Millis(get_current_time(state).saturating_add(ms.into()))
        }
        Timeout::Never => TimeoutAbsolute::Never,
    }
}

impl PureModel for TimeState {
    type Action = TimeAction;

    fn process_pure<Substate: ModelState>(
        state: &mut State<Substate>,
        action: Self::Action,
        dispatcher: &mut Dispatcher,
    ) {
        match action {
            TimeAction::UpdateCurrentTime => {
                dispatcher.dispatch_effect(TimeEffectfulAction::GetSystemTime {
                    uid: state.new_uid(),
                    on_result: callback!(|(uid: Uid, result: Duration)| {
                        TimeAction::GetSystemTimeResult { uid, result }
                    }),
                })
            }
            TimeAction::GetSystemTimeResult { uid: _, result } => {
                state.substate_mut::<TimeState>().set_time(result);
            }
        }
    }
}
