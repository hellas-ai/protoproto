# State Machine Documentation

The state machine employs a modular, plugin-like system composed of *models*. Each model is linked with a specific set of *actions* that it is capable of processing. Models are classified into two categories: *pure* and *effectful*.

## Pure Models

Pure models are responsible for handling the state transitions of the state machine. Most of the state machine logic should be implemented as *pure* models. They implement a `process_pure` function to handle their associated actions.

- **Arguments** of `process_pure` function:
    1. `State` of the state machine.
    2. An *action* from the set of actions associated with the model.
    3. A *dispatcher* object.

- **Capabilities** of `process_pure` function:
    1. Inspect the current *action*.
    2. Access or modify the state of the state machine.
    3. *Dispatch* additional actions that can be handled by either *pure* or *effectful* models.

- **Restrictions**: It's critical that the code in this function does not invoke any other function that triggers side-effects (i.e., those that modify state outside the state machine or any IO operations).

## Effectful Models

Effectful models primarily act as a bridge between the pure models (the state machine) and the external environment. Their main aim is to abstract APIs designed to perform IO. They implement a `process_effectful` function to handle their associated actions.

- **Arguments** of `process_effectful` function:
    1. Local state of the model: this means the effectful model can only access its own state, which should be kept as concise and simple as possible.
    2. An *action* from the set of actions associated with the model.
    3. A *dispatcher* object.

- **Capabilities** of `process_effectful` function:
    1. Inspect the current *action*.
    2. Access or modify its own local state.
    3. Execute side-effects/IO.
    4. *Dispatch* other actions, which can be handled by *pure* or *effectful* models.

- **Restrictions**: An effectful model has no method to access the state of the state machine. Its ability to communicate with the state machine is only through dispatching *pure actions* (actions associated with a *pure* model).

## Dispatcher Object

The **dispatcher** serves a critical role in our state machine. It manages the action's queue, *dispatching* (enqueuing) and *processing* (dequeuing) actions, thus providing the means for models to communicate with each other, including effectful models which have no access to the state machine's state.

### Action Queue

Every time an action is "dispatched" it is added to the queue. Despite the fact that both pure and effectful actions end up in the same queue (RTTI is utilized to determine the appropriate model to handle these actions), there is a distinction in their dispatching mechanisms to enhance clarity about the type of action being dispatched:

- **Pure Actions**: The `dispatch` function is used to dispatch pure actions.
- **Effectful Actions**: Effectful actions can only be dispatched using the `dispatch_effect` function.

> **Note**: there is a third dispatch function, namely `dispatch_back`, which is elaborated on in the [callbacks](#callbacks) section.

### Processing Actions

The *dispatcher* uses FIFO methodology for processing actions, which are dequeued and handled by the *Runner*. 

The runner's execution cycle invokes the `next_action` function of the *dispatcher* to dequeue an action. If the queue is empty, the `next_action` function will call a user-defined "tick" function to generate a *tick action* (more details about tick actions are provided in the [Model Hierarchy](#model-hierarchy) section).

> **Communication among Models**: despite pure models having the ability to access the state of the state machine, the dispatcher is the actual medium for models to interact with each other (except for a few cases).

## Actions

Each model defines a set of actions it can process. Each action is defined as a variant within an `enum` type. All possible actions the model can handle are described within this enum type.

### Action Traits

Each action type needs to implement the following traits: `#[derive(Clone, PartialEq, Eq, TypeUuid, Serialize, Deserialize, Debug)]`. 

#### UUID Requirement

The action type should be associated with a [UUID](https://www.uuidgenerator.net/). The reason behind this requirement is to facilitate RTTI, usually provided by Rust's `TypeId`. `TypeId`, however, does not cater for serialization/record/replay purposes, hence the need for UUIDs.

#### Action Kind 

The action type must implement the `Action` trait, which specifies the action's kind. This could be either pure or effectful. For instance, if we define our own `MyPureAction` type, we should implement it as follows:

```rust
impl Action for MyPureAction {
  const KIND: ActionKind = ActionKind::Pure;
}
```

### Callbacks

Callbacks serve as a mechanism for composing actions.

The primary use case often involves a caller (a model handling actions of type `A`) dispatching an action of type `B` and subsequently desiring the result from the processing of `B`. To achieve this, the action of type `B` may contain one or more callback values that are later filled by the caller. The caller (`A`) is then tasked with assigning the callback in such a manner that the processing of `B` will `dispatch_back` an action of type `A`.

Here is an example with code extracted from the *MIO model*. The action `MioEffectfulAction::PollCreate` can be dispatched by any model wishing to create a MIO poll object and is defined as follows:

```rust
pub enum MioEffectfulAction {
    PollCreate {
        poll: Uid,
        on_success: Redispatch<Uid>,
        on_error: Redispatch<(Uid, String)>,
    },
    ...
```

In this snippet, the `Redispatch<R>` type is utilized when specifying the callback fields of the action. The example showcases two callbacks: `on_success`, responsible for dispatching an action upon successful poll creation, and `on_error`, which handles error cases.

The type `R` encapsulates the result value of the processing of the `PollCreate` action. In the event of success, it represents the [UID](#uids) of the poll object (set by the caller via the `poll` field). Conversely, if an error occurs, it consists of a tuple comprising the `poll` value and a `String` describing the error. Notably, the UID is dual-purposed: it assigns the reference value for the poll object and identifies the dispatched action.

Subsequently, the MIO model utilizes `dispatch_back` for each scenario:

```rust
fn process_effectful(&mut self, action: Self::Action, dispatcher: &mut Dispatcher) {
    match action {
        MioEffectfulAction::PollCreate {
            poll,
            on_success,
            on_error,
        } => {
            // NOTE: use this pattern to inhibit side-effects when in replay mode
            let result = if dispatcher.is_replayer() {
                // This value is ignored and it is replaced by whatever it
                // is in the recording file.
                Ok(())
            } else {
                self.poll_create(poll)
            };

            match result {
                Ok(_) => dispatcher.dispatch_back(&on_success, poll),
                Err(error) => dispatcher.dispatch_back(&on_error, (poll, error)),
            }
        }
        ...
```

To conclude, let's demonstrate how a model can dispatch a `PollCreate` function and receive the results. This is exemplified in the *TCP model* during its initialization:

```rust
fn process_pure<Substate: ModelState>(
    state: &mut State<Substate>,
    action: Self::Action,
    dispatcher: &mut Dispatcher,
) {
    match action {
        TcpAction::Init {
            instance,
            on_success,
            on_error,
        } => {
            let poll = state.new_uid();
            let tcp_state: &mut TcpState = state.substate_mut();

            tcp_state.status = Status::InitPollCreate {
                instance,
                poll,
                on_success,
                on_error,
            };

            dispatcher.dispatch_effect(MioEffectfulAction::PollCreate {
                poll,
                on_success: callback!(|poll: Uid| TcpAction::PollCreateSuccess { poll }),
                on_error: callback!(|(poll: Uid, error: String)| TcpAction::PollCreateError { poll, error })
            });
        }
        ...
```

In this specific interaction, a pure model (TCP model) interacts with an effectful model (MIO model), using callbacks to incorporate "external world" information into the state machine. Although not mandatory, callbacks can also facilitate communication between two pure models.

> **Note**: one notable aspect is the use of the `callback!` macro for assigning callback values. This mechanism is needed for serializing callback information, crucial for supporting functionalities such as state snapshots and record/replay features.

## State

The state of the state machine is defined as:

```rust
pub struct State<Substates: ModelState> {
  pub uid_source: Uid,
  pub substates: Vec<Substates>,
  current_instance: usize,
}
```

For now, we will set aside the `current_instance` field and presume that `substates` is a single entity rather than a vector. The exact reason for its present structure will be discussed later in the [Multiple dispatchers for testing-scenarios](#multiple-dispatchers-for-testing-scenarios) section.

### UIDs

Many actions necessitate references to various resources utilized by the state machine models. Even though the state of the state machine is shared among (pure) models, usually, the models don't need awareness of the internal representations of other models' resources. Moreover, pure and effectful models need to reference resources held by one another, which they can't access directly.

UIDs serve as a straightforward solution for providing references in a state-machine wide perspective:

- Essentially, a `Uid` is a `u64` number that increases with each use.
- The `State::new_uid` function increments the `uid_source` field, shared amongst all models.
- The UID can be compared to a "file descriptor" used in Linux, with the only difference being that UID values should never be reused. This simplifies implementation.
- Most actions carry their own UID value. This is handy for pinpointing the source of other actions dispatched in response to the action itself.

> **Note**: effectful models cannot generate new `Uid` values as they lack access to the state machine state. However, this limitation is mitigated by the caller model generating and providing the UID value when requesting the effectful model to allocate a new resource.

### Substates

Every pure model state is a *substate* of the state machine state. Since models are capable of accessing the state of the state machine, they can therefore access their own substate or substates of other models.

Given the modular design of models, and the possibility to use distinct model configurations to form a specific state machine, a fixed `State` struct cannot be set. Conversely, different combinations of model states are abstracted in the `substates` field, which is [assigned](#substate) to the [top-most model](#top-most-model) when setting up the [runner](#runner) instance.

Given the state machine's `state` of `State` type, a model can access a *substate* of type `MyState` by invoking `state.substate::<MyState>()` (or for a mutable reference, `state.substate_mut::<MyState>()`). RTTI and the derived `ModelState` trait enable fetching references to different substates just via their type (each substate type should have precisely one instance).

In summary, substates allow a model to access the substates of the models they are aware of, without the requirement of knowing about the rest of the models in the current state machine configuration.
        
## Runner

The *Runner* is responsible for controlling the operation of the state machine.

Providing a builder pattern to facilitate the registration of models, a `RunnerBuilder` implementation allows us to establish various configurations for every `Runner` instance.

Included in the creation of a `Runner` object by the `RunnerBuilder`, are: the [state](#state) of the state machine, the models which have been [registered](#model-registration), and the [dispatcher](#dispatcher-object).

Upon execution, the `run()` function of a `Runner` commences the loop of the state machine. It processes the actions that originate from the action queue of the dispatcher, directing them to the appropriate `process_pure` or `process_effectful` handlers amongst the registered models. This is done in conjunction with both the state and the dispatcher object.

### Model hierarchy

The model architecture is structured akin to executable programs and libraries. Within this hierarchy, [effectful models](#effectful-models) can be viewed as low-level libraries, much like libc, primarily offering minimal abstractions over the OS syscalls (the external world). Stacked upon these are [pure models](#pure-models) providing varying levels of abstraction. The [top-most](#top-most-model) model resembles an executable program, utilizing all underlying models and providing the entry-point for the state machine execution.

In our current implementation, the model hierarchy is illustrated as follows:

```md
Echo-client model (top-most model)
├── TCP client model (pure)
│   └── TCP model (pure)
│       ├── MIO model (effectful)
│       └── Time model (pure)
│           └── Time model (effectful)
└── PRNG model (pure)
```

```md
Echo-server model (top-most model)
└── TCP server model
    └── TCP model (pure)
        ├── MIO model (effectful)
        └── Time model (pure)
            └── Time model (effectful)
```

```md
PNET simple-client model (top-most model)
└── PNET client model (pure)
    ├── TCP client model (pure)
    │   └── TCP model (pure)
    │       ├── MIO model (effectful)
    │       └── Time model (pure)
    │           └── Time model (effectful)
    └── PRNG model (pure, TODO: replace with effectful version)
```

```md
PNET echo-client model (top-most model)
├── PNET client model (pure)
│   ├── TCP client model (pure)
│   │   └── TCP model (pure)
│   │       ├── MIO model (effectful)
│   │       └── Time model (pure)
│   │           └── Time model (effectful)
│   └── PRNG model (pure, TODO: replace with effectful version)
└── PRNG model (pure)
```

```md
PNET echo-server model (top-most model)
└── PNET server model (pure)
    ├── TCP server model
    │   └── TCP model (pure)
    │       ├── MIO model (effectful)
    │       └── Time model (pure)
    │           └── Time model (effectful)
    └── PRNG model (pure, TODO: replace with effectful version)
```

#### Model registration

Model registration mirrors library dependencies by explicitly defining interconnections among models. Let's examine the model registration process for the TCP model:

```rust
// This model depends on the `TimeState` (pure) and `MioState` (effectful).
impl RegisterModel for TcpState {
    fn register<Substate: ModelState>(builder: RunnerBuilder<Substate>) -> RunnerBuilder<Substate> {
        builder
            .register::<TimeState>()
            .register::<MioState>()
            .model_pure::<Self>()
    }
}
```

This snippet showcases how the TCP model enlists its dependencies and completes its registration —an approach that aligns with the hierarchical structure established previously.

> **Note**: when model `A` is dependent on both `B` and `C`, and `B` is reliant on `C`, there is no requisite to register `C` from `A`, although redundant registrations are still permissible.

#### Top-most model

The top-most model serves as the entry point guiding the execution of a specific state-machine configuration. In our existing setup, five top-most models have been utilized for testing, including: echo-client, echo-server, their PNET counterparts, and a simple client that connects over PNET protocol and sends some user-defined data.

Top-most models are mandated to implement a "tick" action dispatched by the runner when the dispatch queue is empty.

##### Tick action

The tick action is exclusively integrated by top-most models, and provides the progression mechanism of the state machine. Top-most models usually implement tick action handling to perform tasks such as event polling and time updates.

When setting up the runner instance, the selected tick action influences the state machine's progress mechanism. For instance, configuring the simple-client model in the *berkeley_connect* test follows a template similar to the one shown below (the tick action is passed as the second argument of the `instance` invocation):

```rust
#[test]
fn connect() {
    RunnerBuilder::<PnetClient>::new()
        .register::<PnetClient>()
        .instance(
            PnetClient::from_config(ClientConfig {
                client: PnetSimpleClientConfig {
                    connect_to_address: "65.109.110.75:18302".to_string(),
                    connect_timeout: Timeout::Millis(2000),
                    poll_timeout: 1000,
                    max_connection_attempts: 10,
                    retry_interval_ms: 500,
                    send_data: b"\x13/multistream/1.0.0\n".to_vec(),
                    recv_data: b"\x13/multistream/1.0.0\n".to_vec(),
                    recv_timeout: Timeout::Millis(2000),
                },
                pnet: PnetClientConfig {
                    pnet_key: PnetKey::new(
                        "3c41383994b87449625df91769dff7b507825c064287d30fada9286f3f1cb15e",
                    ),
                    send_nonce_timeout: Timeout::Millis(2000),
                    recv_nonce_timeout: Timeout::Millis(2000),
                },
            }),
            || PnetSimpleClientAction::Tick.into(),
        )
        .build()
        .run()
}
```

##### Substate

In the previously discussed example, it was noticed that the first argument in the `instance` call is associated to the `PnetClient` type. This specific type is linked to the `substates` field of the state machine [state](#state) and merges the substates of all models that constitute the present runner configuration.

Let's explore how `PnetClient` is defined in this particular test:

```rust
#[derive(ModelState, Debug)]
pub struct PnetClient {
    pub prng: PRNGState,
    pub time: TimeState,
    pub tcp: TcpState,
    pub tcp_client: TcpClientState,
    pub pnet_client: PnetClientState,
    pub client: PnetSimpleClientState,
}
```

As illustrated, `PnetClient` is a struct wherein each field denotes the substate of every incorporated model. The `ModelState` derive macro generates the `substate()`/`substate_mut()` implementations, granting access to each field based on their respective types.

> **Note**: the absence of any dependency fields may lead to a runtime panic. In the future, it may be feasible to integrate a mechanism that could autonomously generate this structure from the engaged models, thus circumventing this limitation.

### Multiple dispatchers for testing-scenarios

Let's now delve into the actual implementation of the state machine state. The definition, outlined in the [state section](#state) of this documentation, is as follows:

```rust
pub struct State<Substates: ModelState> {
  pub uid_source: Uid,
  pub substates: Vec<Substates>,
  current_instance: usize,
}
```

The rationale behind adopting a vector of `Substates` and the `current_instance` field is to facilitate the registration of multiple instances of a top-level model within the same `Runner`. Each instance comes with an independent state for their model and all associated dependencies, yet shares the remainder of the state machine state (currently just the `Uid` generator).

In registering multiple top-level model instances, we can run different "programs" that interact with each other within the same state machine. This technique is implemented in the echo-network tests:

```rust
#[test]
fn echo_server_1_client() {
    RunnerBuilder::<EchoNetwork>::new()
        .register::<EchoNetwork>()
        .instance(
            EchoNetwork::EchoServer(EchoServer::from_config(EchoServerConfig {
                address: "127.0.0.1:8888".to_string(),
                max_connections: 1,
                poll_timeout: 100,
                recv_timeout: 500,
            })),
            || EchoServerAction::Tick.into(),
        )
        .instance(
            EchoNetwork::EchoClient(EchoClient::from_config(EchoClientConfig {
                connect_to_address: "127.0.0.1:8888".to_string(),
                connect_timeout: Timeout::Millis(1000),
                poll_timeout: 100,
                max_connection_attempts: 10,
                retry_interval_ms: 500,
                max_send_size: 10240,
                min_rnd_timeout: 1000,
                max_rnd_timeout: 10000,
            })),
            || EchoClientAction::Tick.into(),
        )
        .build()
        .run()
}
```

From the above example, it's evident that both echo-server and echo-client models are registered, facilitating their mutual interaction.

Furthermore, multiple instances of the same model can also be registered. A case in point is the registration of one echo server together with an arbitrary number of clients:

```rust
fn echo_server_n_clients(n_clients: u64) {
    let mut builder = RunnerBuilder::<EchoNetwork>::new()
        .register::<EchoNetwork>()
        .instance(
            EchoNetwork::EchoServer(EchoServer::from_config(EchoServerConfig {
                address: "127.0.0.1:8888".to_string(),
                max_connections: n_clients as usize,
                poll_timeout: 100 / n_clients,
                recv_timeout: 500 * n_clients,
            })),
            || EchoServerAction::Tick.into(),
        );

    for _ in 0..n_clients {
        builder = builder.instance(
            EchoNetwork::EchoClient(EchoClient::from_config(EchoClientConfig {
                connect_to_address: "127.0.0.1:8888".to_string(),
                connect_timeout: Timeout::Millis(1000 * n_clients),
                poll_timeout: 100 / n_clients,
                max_connection_attempts: 10,
                retry_interval_ms: 500,
                max_send_size: 1024 / n_clients,
                min_rnd_timeout: 1000,
                max_rnd_timeout: 1000 * n_clients,
            })),
            || EchoClientAction::Tick.into(),
        );
    }

    builder.build().run()
}
```

A crucial point to consider is that effectful models (and their state) will be shared to all the registered instances. This implies that when creating an effectful model, it's vital to ensure it accommodates such flexibility. This scenario also underscores why sharing the `Uid` generator is advantageous, as it averts potential collisions pertaining to resources allocated by effectful models.

> **Note**: one might observe the necessity to reduce the poll timeout when operating multiple clients. This is necessitated by the fact that each new instance of the TCP model (pure) will create an independent poll object. The poll timeout then blocks the state machine for the defined period in the absence of IO events. In the future, a mock version of the MIO model or even the TCP model can be introduced, which would eliminate any real IO and overcome this constraint.

Another detail worth acknowledging pertains to the `EchoNetwork` type definition. Instead of adopting a struct, an enum type is used, where each variant corresponds to a separate top-model:

```rust
#[derive(ModelState, Debug)]
pub struct EchoServer {
    pub time: TimeState,
    pub tcp: TcpState,
    pub tcp_server: TcpServerState,
    pub echo_server: EchoServerState,
}

#[derive(ModelState, Debug)]
pub struct EchoClient {
    pub prng: PRNGState,
    pub time: TimeState,
    pub tcp: TcpState,
    pub tcp_client: TcpClientState,
    pub echo_client: EchoClientState,
}

#[derive(ModelState, Debug)]
pub enum EchoNetwork {
    EchoServer(EchoServer),
    EchoClient(EchoClient),
}
```

Lastly, let's see how the runner operates with multiple dispatchers to equitably distribute "work" across different instances:

```rust
    // State-machine main loop. If the runner contains more than one instance,
    // it interleaves the processing of actions fairly for each instance.
    pub fn run(&mut self) {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
            .format(|buf, record| writeln!(buf, "[{}] {}", record.level(), record.args()))
            .init();

        loop {
            for instance in 0..self.dispatchers.len() {
                self.state.set_current_instance(instance);
                let dispatcher = &mut self.dispatchers[instance];

                if dispatcher.is_halted() {
                    return;
                }

                let action = dispatcher.next_action();
                self.process_action(action, instance)
            }
        }
    }
```

## Constructing a Basic Model

In this tutorial, we will walk through the process of creating a simple model to obtain the system time, making use of the concepts of the state machine that we have previously introduced.

### Defining an Effectful Time Model

Since the system time is sourced from the external world, we'll create an effectful model. The implementation of the model will be segmented into three sections:

- *state.rs*: Holds the local state of the model outside the state machine.
- *action.rs*: Contains the type definition for the actions associated with the model.
- *model.rs*: Contains the core implementation for processing the actions (`process_effectful`), and the code for the model's [registration](#model-registration).

#### state.rs

Although we do not need to maintain any state in this specific case, it is required to associate a state type with the model. Therefore, we'll implement a placeholder:

```rust
pub struct TimeState(); // placeholder
```

#### action.rs

We'll denote the action type as `TimeEffectfulAction`. To maintain clarity, we append `Action` to all our action definitions and prefix it with `Effectful` to highlight the nature of this action. Note that effectful actions are always dispatched with `dispatch_effectful`.

```rust
[derive(Clone, PartialEq, Eq, TypeUuid, Serialize, Deserialize, Debug)]
#[uuid = "3221c0d5-02f5-4ed6-bf79-29f40c5619f0"]
pub enum TimeEffectfulAction {
    GetSystemTime {
        uid: Uid,
        on_result: Redispatch<(Uid, Duration)>,
    },
}
```

An unique value is assigned to each action type definition in compliance with the [UUID requirement](#uuid-requirement). You can easily generate this unique value at https://www.uuidgenerator.net/.

This action type will have only one variant: `GetSystemTime`. This action contains a [uid](#uids) field for identifying the action, and an `on_result` [callback](#callbacks) that returns a tuple holding the `uid` value provided by the caller and a `Duration` object containing the current system time since `UNIX_EPOCH`.

Next, we provide the `Action` implementation, where `KIND` aligns with the `ActionKind::Effectful` variant:

```rust
impl Action for TimeEffectfulAction {
    const KIND: ActionKind = ActionKind::Effectful;
}
```

#### model.rs

We start with the model registration:

```rust
impl RegisterModel for TimeState {
    fn register<Substate: ModelState>(builder: RunnerBuilder<Substate>) -> RunnerBuilder<Substate> {
        builder.model_effectful(Effectful::<Self>(Self()))
    }
}
```

This model doesn't have any dependencies, so it just registers itself. Note that models are implemented based on their state type definition, that's why we always need a state type even if it's just a placeholder.

Moving on to the actual implementation:

```rust
impl EffectfulModel for TimeState {
    type Action = TimeEffectfulAction;

    fn process_effectful(&mut self, action: Self::Action, dispatcher: &mut Dispatcher) {
        match action {
            TimeEffectfulAction::GetSystemTime { uid, on_result } => {
                let result = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .expect("System clock set before UNIX_EPOCH")

                dispatcher.dispatch_back(&on_result, (uid, result));
            }
        }
    }
}
```

Here, we implemented `EffectfulModel` for our state type and linked it to the `TimeEffectfulAction` action type. We then implemented the `process_effectful` handler. When the `GetSystemTime` variant is matched, we perform the side effects (by calling `SystemTime::now()`), calculate the `Duration` since `UNIX_EPOCH`, and dispatch the result back using `dispatch_back` with the caller-defined *callback* (`on_result`).

### Defining a Pure Time Model

Fetching the current time is a frequent task, and it might be inefficient to dispatch an effectful action every time we need to get the time. Instead, we can store a copy of the system time in the [state machine's state](#state) which can be updated periodically, for instance, during the processing of a [tick action](#tick-action) by the *top-model*. The remaining pure models can then directly access the time from the state machine's state.

> This technique mirrors the optimization strategy employed by the Linux kernel for accessing system time from user-mode programs: the kernel maps a page containing the system time into each process, periodically updates it, and enables direct memory access by the programs, thereby negating the need for executing a system call.

The sections needed to implement a pure model are similar to the effectful model: *state.rs*, *action.rs* and *model.rs*.

#### state.rs

Here, we define `TimeState`. Although identical in name with the effectful model, they exist in unique namespaces (`pure::time::TimeState` and `effectful::time::TimeState`). In the effectful model, this struct served as a placeholder, but in this context, it represents actual state and forms part of the state machine's state.

```rust
#[derive(Default, Serialize, Deserialize, Debug)]
pub struct TimeState {
    now: Duration,
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

The `now` field in our state can be read using `now()` and set using `set_time()`. Pure models typically access the time through `state.substate::<TimeState>().now()`. It's uncommon for any pure model, other than the time model, to set the time; hence, `set_time` should be primarily used by the time model.

#### action.rs

The pure time model serves as a connector between other pure models and the effectful time model. Therefore, we implement two actions: the first to facilitate calls from other models, and the second to be used as a callback action when communicating with the effectful time model.

```rust
#[derive(Clone, PartialEq, Eq, TypeUuid, Serialize, Deserialize, Debug)]
#[uuid = "1911e66d-e0e3-4efc-8952-c62f583059f6"]
pub enum TimeAction {
    UpdateCurrentTime,
    GetSystemTimeResult { uid: Uid, result: Duration },
}
```

Following this definition, when a pure model wishes to update the system time in the state machine's state, it will dispatch the first variant: `TimeAction::UpdateCurrentTime`.

The `Action` implementation, in this case, corresponds to the `ActionKind::Pure` variant:

```rust
impl Action for TimeAction {
    const KIND: ActionKind = ActionKind::Pure;
}
```

#### model.rs

Considering the necessity to reference the effectful time model, we create an alias to prevent full namespace inclusion and avoid `TimeState` definition collisions:

```rust
use crate::models::effectful::time::{
    action::TimeEffectfulAction, state::TimeState as TimeStateEffectful,
};
```

Now, we register our model. This time, we will also register the **effectful model** as a dependency:

```rust
impl RegisterModel for TimeState {
    fn register<Substate: ModelState>(builder: RunnerBuilder<Substate>) -> RunnerBuilder<Substate> {
        builder.register::<TimeStateEffectful>().model_pure::<Self>()
    }
}
```

Here's the resultant implementation:

```rust
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
```

Upon receiving an `UpdateCurrentTime` action from another model, we dispatch the `GetSystemTime` action to the **effectful time model**. We then pass a callback instructing the effectful model to **dispatch the result back** in a `GetSystemTimeResult` action. Upon handling this action, we update the state machine's state using the time obtained from the result.



## Other
### Timeouts
TODO

### Other model examples
TODO