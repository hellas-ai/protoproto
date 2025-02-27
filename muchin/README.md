# Muchin: A Composable State Machine Framework

Muchin is a Rust library designed for building complex, interacting state machines. It offers a modular and extensible architecture that facilitates the composition of large-scale state machines from smaller, reusable components called *models*. Muchin is particularly well-suited for scenarios requiring sophisticated state management and interaction with external systems (e.g., network I/O).

## Quick Rundown

*   **Modularity:** State machines are built from composable *models*, each encapsulating a specific piece of logic and state.
*   **Pure and Effectful Models:**  Clear separation between *pure* models (handling state transitions) and *effectful* models (interacting with the outside world).
*   **Action-Driven:** Models communicate through *actions*, providing a structured way to manage events and state changes.
*   **Dispatcher:**  A central *dispatcher* manages an action queue, enabling communication between models and handling action processing in a FIFO manner.
*   **Callbacks:**  A powerful mechanism for composing actions and handling results asynchronously, crucial for interactions between pure and effectful models.
*   **UIDs:**  Unique Identifiers (UIDs) provide a robust system for referencing resources across the state machine, regardless of model boundaries.
*   **Hierarchical Model Structure:**  Models are organized hierarchically, similar to libraries and executables, facilitating code organization and reuse.
*   **Testability:** Supports multiple *dispatcher* instances, allowing for simulated environments with interacting components (e.g., server and clients).
* **Recording and replay** Support recording the inputs to create reproducable, debugable outputs. The replay mode allow step-by-step analysis of a past event chain.

## Core Concepts

### Models

Models are the building blocks of Muchin state machines. Each model is responsible for a specific aspect of the state machine's functionality and can process a defined set of *actions*. Models are categorized into two types:

#### Pure Models

*   **Responsibility:** Handle internal state transitions of the state machine.  These represent the core logic of your application.
*   **Implementation:** Implement the `PureModel` trait with a `process_pure` function.
*   **`process_pure` Arguments:**
    1.  `State<Substate>`:  A mutable reference to the entire state machine state.  Allows read/write access to *any* substate.
    2.  `Self::Action`:  The specific action to be processed.
    3.  `Dispatcher`: A mutable reference to the dispatcher for queueing further actions.
*   **Capabilities:**
    *   Inspect the current action.
    *   Read and modify any part of the state machine's state (including substates of other models).
    *   Dispatch new actions (both pure and effectful) using the `dispatcher`.
*   **Restrictions:** Must *not* perform any side effects other than modifying the state machine's state. No I/O or external calls.

#### Effectful Models

*   **Responsibility:**  Bridge the gap between the pure state machine and the external world. Handle I/O operations, interact with external APIs, etc.
*   **Implementation:** Implement the `EffectfulModel` trait with a `process_effectful` function.
*   **`process_effectful` Arguments:**
    1.  `&mut self`: A mutable reference to the *effectful model's own* local state.  **Critically, effectful models have NO access to the main state machine state.**
    2.  `Self::Action`:  The specific action to be processed.
    3.  `Dispatcher`: A mutable reference to the dispatcher.
*   **Capabilities:**
    *   Inspect the current action.
    *   Read and modify their own local state.
    *   Perform side effects (I/O, external API calls, etc.).
    *   Dispatch new actions (both pure and effectful) using `dispatch_effect`.
*   **Restrictions:** Cannot access the main state machine state.  Communication back to the pure models is *only* through dispatching actions.

### Dispatcher

The `Dispatcher` is the heart of the Muchin state machine.  It plays several key roles:

*   **Action Queue:**  Manages a FIFO queue of actions to be processed.
*   **Action Dispatching:**  Provides methods for adding actions to the queue:
    *   `dispatch`:  For dispatching pure actions.
    *   `dispatch_effect`:  For dispatching effectful actions (enforces clarity about action type).
    *   `dispatch_back`: Used with callbacks (see below) to handle results of previously dispatched actions.
*   **Action Processing:**  Dequeues actions one at a time for the `Runner` to handle.
*   **Tick Actions:** When the action queue is empty, the `Dispatcher` calls a user-defined "tick" function to generate a *tick action*, driving the progression of the state machine (e.g., for time updates or event polling).
*   **Halt/Unhalt** Allow external access for stopping and restarting the execution.
*   **Replayer**: `is_replayer` return whether a replay session is currently active, useful to selectively inhibit side effects

### Actions

Actions represent events or commands within the state machine. Each model defines an `enum` type that lists all the actions it can handle.

*   **Action Traits:** Action types must implement several traits: `Clone`, `PartialEq`, `Eq`, `TypeUuid`, `Serialize`, `Deserialize`, and `Debug`.  The `TypeUuid` trait, combined with a unique [UUID](https://www.uuidgenerator.net/), is used for runtime type identification and serialization, which supports record and replay feature.
*   **`Action` Trait:** Every action type must implement the `Action` trait, specifying its `KIND` (either `ActionKind::Pure` or `ActionKind::Effectful`).
     This implementation enforces correct type handling during dispatch.
```rust
impl Action for MyPureAction {
  const KIND: ActionKind = ActionKind::Pure;
}
```

### Callbacks (Composition)

Callbacks are a crucial mechanism for composing actions and achieving asynchronous-like behavior within the synchronous, action-driven model.

*   **Use Case:**  A model (caller) dispatches an action that needs a result. The dispatched action uses callbacks to return the result by triggering another action on the caller.
*   **Example (from the MIO model):**

```rust
// Defining the action with callbacks.
pub enum MioEffectfulAction {
    PollCreate {
        poll: Uid,
        on_success: Redispatch<Uid>, // Callback for success.
        on_error: Redispatch<(Uid, String)>, // Callback for failure.
    },
    ...
}

//Handling the action and dispatching back the result using the callback:
fn process_effectful(&mut self, action: Self::Action, dispatcher: &mut Dispatcher) {
    match action {
        MioEffectfulAction::PollCreate {
            poll,
            on_success,
            on_error,
        } => {
            // ... (Perform side effect - poll creation) ...

            match result {
                Ok(_) => dispatcher.dispatch_back(&on_success, poll), // Success.
                Err(error) => dispatcher.dispatch_back(&on_error, (poll, error)), // Failure.
            }
        }
        ...
//Using the `callback!` macro from the caller side:
dispatcher.dispatch_effect(MioEffectfulAction::PollCreate {
    poll,
    on_success: callback!(|poll: Uid| TcpAction::PollCreateSuccess { poll }),
    on_error: callback!(|(poll: Uid, error: String)| TcpAction::PollCreateError { poll, error })
});
```

*   **`Redispatch<R>`:** A special type used for callback fields in actions. It holds information about which action to dispatch back and the type of the result (`R`).
*   **`callback!` Macro:**  This macro simplifies creating and serializing callbacks, enabling features like state snapshots and record/replay.

### State

The global state of the Muchin state machine is defined as follows:

```rust
pub struct State<Substates: ModelState> {
  pub uid_source: Uid, // Global UID generator.
  pub substates: Vec<Substates>,  // Collection of substates (one per top-level model instance).
  current_instance: usize, // Index of the currently active instance.
}
```
*  **Multiple Instances:**  The `substates` field is a *vector*. While in the main loop you should care only for the `current_instance`. 

#### UIDs

UIDs (Unique Identifiers) are 64-bit integers used to reference resources across the entire state machine. They serve a purpose similar to file descriptors, but UIDs *are never reused*.

*   **`Uid` Type:** A wrapper around `u64`.
*   **`State::new_uid()`:** Generates a new, monotonically increasing UID.
*   **Purpose:** Provides a global, consistent way to refer to resources (like network connections, timers, etc.) without models needing to know the internal representation of those resources.  Essential for communication between pure and effectful models.
* **Generation**: Uid should be created only by *pure models* and passed down when dispatching *effectful actions*

#### Substates

Each *pure* model has its own *substate*, which is a part of the global state machine state. Pure models can access their own substate and the substates of other models they depend on.

*   **`substates` Field:**  Contains instances of all model states used in the state machine configuration.  The type of this field is defined by the *top-most model* (see below).
*   **`ModelState` Trait:**  Provides `state()` and `state_mut()` methods to access substates by their type (using runtime type information - RTTI).  This is typically derived using the `#[derive(ModelState)]` macro.
*   **Example (accessing a substate):**

```rust
let time_state: &mut TimeState = state.substate_mut::<TimeState>();
```

### Runner

The `Runner` drives the execution of the state machine.

*   **`RunnerBuilder`:** A builder pattern is used to configure and create a `Runner` instance:
    *   **`register()`:** Registers models, establishing their dependencies.
    *   **`instance()`:**  Creates a new instance of a top-level model, initializing its substate and providing a tick action.
    *   **`build()`:** Constructs the `Runner` object.
*   **`run()`:**  Starts the main loop of the state machine:
    1.  Selects an instance to run (in round-robin fashion for multiple instances).
    2.  Retrieves the next action from the `Dispatcher`.
    3.  Calls the appropriate `process_pure` or `process_effectful` handler on the registered model based on the action's type.

#### Model Hierarchy

Muchin models have a hierarchical structure:

*   **Effectful Models:**  Represent low-level, side-effectful operations (like system calls). Analogous to system libraries (e.g., libc).
*   **Pure Models:**  Provide varying levels of abstraction, built on top of other pure and effectful models.
*   **Top-Most Model:**  Represents the entry point of the state machine. Analogous to an executable program. It is responsible for defining the "tick" action.
*    **Multiple Top-most model** is useful for implementing interacting instances within the state machine

##### Model Registration

Models are registered to specify the dependency.
*  dependencies with lowest-level at first.
*  only direct dependency

Example (TCP model registration):

```rust
// This model depends on the `TimeState` (pure) and `MioState` (effectful).
impl RegisterModel for TcpState {
    fn register<Substate: ModelState>(builder: RunnerBuilder<Substate>) -> RunnerBuilder<Substate> {
        builder
            .register::<TimeState>()
            .register::<MioState>()
            .model_pure::<Self>() // Finally, registers itself.
    }
}
```

##### Tick Action

The tick action is handled by the *top-most model* and provides a way for the state machine to advance. It's typically used for tasks like:
 * updating internal state based on time, and 
 *  event polling

##### Substate Definition

The top-most model also defines the type of the `substates` field in the global `State`. This struct brings together the substates of all models involved in the specific runner configuration.

Example (for a PNET client):
```rust
#[derive(ModelState, Debug)]
pub struct PnetClient {
    pub prng: PRNGState,      // PRNG model state
    pub time: TimeState,      // Time model state
    pub tcp: TcpState,       // TCP model state
    pub tcp_client: TcpClientState, // TCP client model state
    pub pnet_client: PnetClientState, // PNET client model state
    pub client: PnetSimpleClientState, // Simple client model state (top-most in this example)
}
```

### Multiple Dispatchers

Muchin allows creating a state with multiple instances of top-level models.  Each model operates within a separate state, allowing you to run distinct instances or components concurrently and provide test environment, while they all interact through *effectful models*.

## Building a Simple Model:  System Time Example

Let's build a model to access the system time, illustrating the key concepts.

### 1. Effectful Time Model (`effectful::time`)

This model handles interacting with the OS to get the system time.

**`state.rs`:**

```rust
// effectful::time::state.rs
pub struct TimeState(); // No local state needed.
```
**`action.rs`:**
```rust
// effectful::time::action.rs
use crate::automaton::{Action, ActionKind, Redispatch, Uid};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use type_uuid::TypeUuid;

#[derive(Clone, PartialEq, Eq, TypeUuid, Serialize, Deserialize, Debug)]
#[uuid = "3221c0d5-02f5-4ed6-bf79-29f40c5619f0"] // Generate a UUID!
pub enum TimeEffectfulAction {
    GetSystemTime {
        uid: Uid, // Unique ID for this request.
        on_result: Redispatch<(Uid, Duration)>, // Callback to return the time.
    },
}

impl Action for TimeEffectfulAction {
    const KIND: ActionKind = ActionKind::Effectful;
}
```
**`model.rs`:**

```rust
// effectful::time::model.rs
use super::{action::TimeEffectfulAction, state::TimeState};
use crate::automaton::{
    Dispatcher, Effectful, EffectfulModel, ModelState, RegisterModel, RunnerBuilder,
};
use std::time::{SystemTime, UNIX_EPOCH};

impl RegisterModel for TimeState {
    fn register<Substate: ModelState>(builder: RunnerBuilder<Substate>) -> RunnerBuilder<Substate> {
        builder.model_effectful(Effectful::<Self>(Self())) // Registers the effectful model.
    }
}

impl EffectfulModel for TimeState {
    type Action = TimeEffectfulAction;

    fn process_effectful(&mut self, action: Self::Action, dispatcher: &mut Dispatcher) {
        match action {
            TimeEffectfulAction::GetSystemTime { uid, on_result } => {
                 let result = if dispatcher.is_replayer() {
                    // ignored on replay
                    SystemTime::now()
                      .duration_since(UNIX_EPOCH)
                      .expect("System clock set before UNIX_EPOCH");
                 }

                dispatcher.dispatch_back(&on_result, (uid, result));
            }
        }
    }
}
```
### 2. Pure Time Model (`pure::time`)

This model provides access to the system time within the state machine, updating it periodically.
**`state.rs`:**
```rust
// pure::time::state.rs
#[derive(Default, Serialize, Deserialize, Debug)]
pub struct TimeState {
    now: Duration, // Cached system time.
}

impl TimeState {
    pub fn now(&self) -> &Duration {
        &self.now
    }

    pub fn set_time(&mut self, time: Duration) {
        self.now = time;
    }
}
```


**`action.rs`:**

```rust
// pure::time::action.rs
use crate::automaton::{Action, ActionKind, Uid};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use type_uuid::TypeUuid;

#[derive(Clone, PartialEq, Eq, TypeUuid, Serialize, Deserialize, Debug)]
#[uuid = "1911e66d-e0e3-4efc-8952-c62f583059f6"] // Generate a UUID!
pub enum TimeAction {
    UpdateCurrentTime, // Request to update the cached time.
    GetSystemTimeResult { uid: Uid, result: Duration }, // Callback from the effectful model.
}

impl Action for TimeAction {
    const KIND: ActionKind = ActionKind::Pure;
}
```

**`model.rs`:**
```rust
// pure::time::model.rs
use super::{action::TimeAction, state::TimeState};
use crate::automaton::{
    Dispatcher, ModelState, PureModel, RegisterModel, RunnerBuilder, State, Uid,
};
use crate::callback;
use crate::models::effectful::time::{
    action::TimeEffectfulAction, state::TimeState as TimeStateEffectful, // Alias to avoid name conflicts
};
use std::time::Duration;

impl RegisterModel for TimeState {
    fn register<Substate: ModelState>(builder: RunnerBuilder<Substate>) -> RunnerBuilder<Substate> {
        builder
            .register::<TimeStateEffectful>() // Registers the effectful time model as a dependency
            .model_pure::<Self>()
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
                // Dispatch an effectful action to get the current system time.
                dispatcher.dispatch_effect(TimeEffectfulAction::GetSystemTime {
                    uid: state.new_uid(),
                    on_result: callback!(|(uid: Uid, result: Duration)| {
                        TimeAction::GetSystemTimeResult { uid, result }
                    }), // Uses a callback.
                });
            }
            TimeAction::GetSystemTimeResult { uid: _, result } => {
                // Update the cached time in the state.
                state.substate_mut::<TimeState>().set_time(result);
            }
        }
    }
}
```