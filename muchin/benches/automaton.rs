//! Microbenchmarking the automaton
#![feature(generic_const_exprs)]
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use muchin::automaton::{
    Action, ActionKind, Dispatcher, ModelState, PureModel, RegisterModel, RunnerBuilder, State,
};
use muchin_model_state_derive::ModelState;
use serde::{Deserialize, Serialize};
use type_uuid::TypeUuid;

// Simple no-op action for benchmarking core dispatch overhead
#[derive(Clone, PartialEq, Eq, TypeUuid, Serialize, Deserialize, Debug)]
#[uuid = "11111111-1111-1111-1111-111111111111"]
enum NoOpAction {
    Tick,
    Message,
}

impl Action for NoOpAction {
    const KIND: ActionKind = ActionKind::Pure;
}

// Minimal state for benchmarking
#[derive(ModelState, Debug, Default)]
struct NoOpState {}

impl RegisterModel for NoOpState {
    fn register<Substate: ModelState>(builder: RunnerBuilder<Substate>) -> RunnerBuilder<Substate> {
        builder.model_pure::<Self>()
    }
}

impl PureModel for NoOpState {
    type Action = NoOpAction;

    fn process_pure<Substate: ModelState>(
        _state: &mut State<Substate>,
        _action: NoOpAction,
        _dispatcher: &mut Dispatcher,
    ) {
        // No-op handler
    }
}

// State that dispatches additional actions until counter exhausted
#[derive(Clone, PartialEq, Eq, TypeUuid, Serialize, Deserialize, Debug)]
#[uuid = "22222222-2222-2222-2222-222222222222"]
enum RequeueAction {
    Tick,
    Requeue,
}

impl Action for RequeueAction {
    const KIND: ActionKind = ActionKind::Pure;
}

#[derive(ModelState, Debug)]
struct RequeueState {
    reset_to: usize,
    counter: usize,
}

impl Default for RequeueState {
    fn default() -> Self {
        Self {
            reset_to: 0,
            counter: 0,
        }
    }
}

impl RegisterModel for RequeueState {
    fn register<Substate: ModelState>(builder: RunnerBuilder<Substate>) -> RunnerBuilder<Substate> {
        builder.model_pure::<Self>()
    }
}

impl PureModel for RequeueState {
    type Action = RequeueAction;

    fn process_pure<Substate: ModelState>(
        state: &mut State<Substate>,
        action: Self::Action,
        dispatcher: &mut Dispatcher,
    ) {
        let state = state.substate_mut::<Self>();
        match action {
            RequeueAction::Tick => {
                state.counter = state.reset_to;
                dispatcher.dispatch(RequeueAction::Requeue);
            }
            RequeueAction::Requeue => {
                if state.counter > 0 {
                    state.counter -= 1;
                    dispatcher.dispatch(RequeueAction::Requeue);
                } else {
                    dispatcher.halt();
                }
            }
        }
    }
}

#[derive(ModelState, Debug)]
struct RequeueRunner {
    state: RequeueState,
}

// Benchmark scenarios
fn bench_dispatch(c: &mut Criterion) {
    env_logger::init();

    let mut group = c.benchmark_group("automaton_dispatch");

    group.bench_function("noop_step_one_dispatcher", |b| {
        let mut runner = RunnerBuilder::<NoOpState>::new()
            .register::<NoOpState>()
            .instance(NoOpState {}, || NoOpAction::Tick.into())
            .build();

        b.iter(|| {
            black_box({
                runner.step();
            })
        });
    });

    let tick_cycle = |b: &mut criterion::Bencher<'_>, count: &usize| {
        let mut runner = RunnerBuilder::<RequeueRunner>::new()
            .register::<RequeueState>()
            .instance(
                RequeueRunner {
                    state: RequeueState {
                        reset_to: *count,
                        counter: 0,
                    },
                },
                || RequeueAction::Tick.into(),
            )
            .build();

        b.iter(|| {
            black_box({
                runner.run();
                runner.unhalt();
            })
        });
    };

    group.bench_with_input("tick_cycle_16", &16, tick_cycle);
    group.bench_with_input("tick_cycle_128", &128, tick_cycle);
    group.bench_with_input("tick_cycle_1024", &1024, tick_cycle);
    group.bench_with_input("tick_cycle_8192", &8192, tick_cycle);
    group.bench_with_input("tick_cycle_16384", &16384, tick_cycle);

    group.finish();
}

criterion_group!(benches, bench_dispatch);
criterion_main!(benches);
