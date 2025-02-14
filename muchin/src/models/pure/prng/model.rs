use crate::automaton::{Dispatcher, ModelState, PureModel, RegisterModel, RunnerBuilder, State};

use super::{action::PRNGPureAction, state::PRNGState};

// `PRNGState` is an implementation of `PureModel` specifically used for
// managing the state of a pseudorandom number generator (PRNG).
//
// The model supports only one action, `Reseed`, which reseeds the PRNG with a
// provided `seed` parameter. While this action is available, it's not
// typically necessary to use it. Instead, Models can (and should) access the
// `PRNGState` directly through the `ModelState` interface.
//
// IMPORTANT: This implementation is designed for a fast and deterministic PRNG
// primarily intended for testing purposes. It should NOT be used for
// operations requiring cryptographic security due to its determinism and lack
// of cryptographic strength.
//
// TODO: implement a safe RNG (`EffectfulModel`).

impl RegisterModel for PRNGState {
    fn register<Substate: ModelState>(builder: RunnerBuilder<Substate>) -> RunnerBuilder<Substate> {
        builder.model_pure::<Self>()
    }
}

impl PureModel for PRNGState {
    type Action = PRNGPureAction;

    fn process_pure<Substate: ModelState>(
        state: &mut State<Substate>,
        action: Self::Action,
        _dispatcher: &mut Dispatcher,
    ) {
        let PRNGPureAction::Reseed { seed } = action;
        let prng_state: &mut PRNGState = state.substate_mut();

        prng_state.seed(seed);
    }
}
