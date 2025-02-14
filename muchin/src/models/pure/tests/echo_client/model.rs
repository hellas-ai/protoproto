use super::{
    action::EchoClientAction,
    state::{EchoClientState, EchoClientStatus},
};
use crate::automaton::{
    Dispatcher, ModelState, PureModel, RegisterModel, RunnerBuilder, State, Timeout, Uid,
};
use crate::{
    callback,
    models::pure::{
        net::{
            tcp::action::{TcpAction, TcpPollEvents},
            tcp_client::{action::TcpClientAction, state::TcpClientState},
        },
        prng::state::PRNGState,
        tests::echo_client::state::EchoClientConfig,
        time::model::update_time,
    },
};
use core::panic;
use log::{info, warn};
use rand::{Rng, RngCore};

// The `EchoClientState` acts as a simulated echo client, used for testing the
// functionality of the state-machine and its related models (`TcpClientState`,
// `TcpState`, `MioState`, `TimeState`). The echo client communicates with an
// echo server, which sends back any data it receives from the client.
//
// The `PureModel` implementation of the `EchoClientState` model processes
// `EchoClientAction::Tick` actions that are dispatched on each "tick" of the
// state-machine loop.
//
// During each "tick", the model performs two key tasks:
//
// 1. Updates the current time tracked by the state-machine.
//
// 2. Checks if the TCP client is ready. If it's not, the model initializes
//    the TCP client. If it is ready, a poll action is dispatched.
//
// The rest of the model's logic handles other action variants that:
//
// - Completes the initialization of the TCP client and connects it to the
//   echo server. If the connection request fails, the client makes up to
//   `max_connection_attempts` attempts to reconnect.
//   If this limit is exceeded, the client panics.
//
// - For each poll result the client sends random data to the echo server.
//   The size and content of this data are randomly generated using the
//   `PRNGState` model.
//
// - After sending data, the client dispatches a receive action to read the
//   server's response. A random timeout is generated using the `PRNGState`
//   model to simulate different network conditions.
//
// - When it receives data from the server, the client checks if the received
//   data matches the sent data. If not, the client panics.
//

// This model depends on `PRNGState` and `TcpClientState`.
impl RegisterModel for EchoClientState {
    fn register<Substate: ModelState>(builder: RunnerBuilder<Substate>) -> RunnerBuilder<Substate> {
        builder
            .register::<PRNGState>()
            .register::<TcpClientState>()
            .model_pure::<Self>()
    }
}

impl PureModel for EchoClientState {
    type Action = EchoClientAction;

    fn process_pure<Substate: ModelState>(
        state: &mut State<Substate>,
        action: Self::Action,
        dispatcher: &mut Dispatcher,
    ) {
        match action {
            EchoClientAction::Tick => {
                // Top-most model first task is to update the state-machine time.
                if update_time(state, dispatcher) {
                    // The next `EchoClientAction::Tick` will have the updated time.
                    return;
                }

                let EchoClientState {
                    status,
                    config: EchoClientConfig { poll_timeout, .. },
                    ..
                } = state.substate_mut();

                match status {
                    EchoClientStatus::Init => {
                        // Init TCP model
                        dispatcher.dispatch(TcpAction::Init {
                            instance: state.new_uid(),
                            on_success: callback!(|instance: Uid| EchoClientAction::InitSuccess { instance }),
                            on_error: callback!(|(instance: Uid, error: String)| EchoClientAction::InitError { instance, error }),
                        })
                    }
                    EchoClientStatus::Connecting
                    | EchoClientStatus::Connected { .. }
                    | EchoClientStatus::Sending { .. }
                    | EchoClientStatus::Receiving { .. } => {
                        let timeout = Timeout::Millis(*poll_timeout);
                        // If the client is already initialized then we poll on each "tick".
                        dispatcher.dispatch(TcpClientAction::Poll {
                            uid: state.new_uid(),
                            timeout,
                            on_success: callback!(|(uid: Uid, events: TcpPollEvents)| EchoClientAction::PollSuccess { uid, events }),
                            on_error: callback!(|(uid: Uid, error: String)| EchoClientAction::PollError { uid, error }),
                        })
                    }
                }
            }
            EchoClientAction::InitSuccess { .. } => {
                let connection = state.new_uid();
                let client_state: &mut EchoClientState = state.substate_mut();

                client_state.status = EchoClientStatus::Connecting;
                connect(client_state, connection, dispatcher);
            }
            EchoClientAction::InitError { error, .. } => {
                panic!("Client initialization failed: {}", error)
            }
            EchoClientAction::ConnectSuccess { connection } => {
                let EchoClientState {
                    status,
                    connection_attempt,
                    ..
                } = state.substate_mut();

                if let EchoClientStatus::Connecting = status {
                    *status = EchoClientStatus::Connected { connection };
                    *connection_attempt = 0;
                } else {
                    unreachable!()
                }
            }
            EchoClientAction::ConnectTimeout { connection } => {
                let new_connection_uid = state.new_uid();
                let EchoClientState {
                    status,
                    connection_attempt,
                    config:
                        EchoClientConfig {
                            max_connection_attempts,
                            ..
                        },
                    ..
                } = state.substate_mut();

                if let EchoClientStatus::Connecting = status {
                    *connection_attempt += 1;

                    warn!(
                        "|ECHO_CLIENT| connection {:?} timeout, reconnection attempt {}",
                        connection, connection_attempt
                    );

                    assert!(connection_attempt < max_connection_attempts);
                    connect(state.substate_mut(), new_connection_uid, dispatcher);
                } else {
                    unreachable!()
                }
            }
            EchoClientAction::ConnectError { connection, error } => {
                let new_connection_uid = state.new_uid();
                let EchoClientState {
                    status,
                    connection_attempt,
                    config:
                        EchoClientConfig {
                            max_connection_attempts,
                            ..
                        },
                    ..
                } = state.substate_mut();

                if let EchoClientStatus::Connecting = status {
                    *connection_attempt += 1;

                    warn!(
                        "|ECHO_CLIENT| connection {:?} error: {}, reconnection attempt {}",
                        connection, error, connection_attempt
                    );

                    assert!(connection_attempt < max_connection_attempts);
                    connect(state.substate_mut(), new_connection_uid, dispatcher);
                } else {
                    unreachable!()
                }
            }
            EchoClientAction::CloseEvent { connection } => {
                info!("|ECHO_CLIENT| connection {:?} closed", connection);

                let new_connection_uid = state.new_uid();
                let client_state: &mut EchoClientState = state.substate_mut();

                client_state.status = EchoClientStatus::Connecting;
                connect(client_state, new_connection_uid, dispatcher);
            }
            EchoClientAction::PollSuccess { .. } => {
                // Send random data on every poll if there are no pending send/recv requests.
                if let EchoClientState {
                    status: EchoClientStatus::Connected { connection },
                    config: EchoClientConfig { max_send_size, .. },
                    ..
                } = state.substate()
                {
                    let connection = *connection;
                    let max_send_size = *max_send_size;
                    let request = state.new_uid();
                    let prng: &mut PRNGState = state.substate_mut();
                    let random_size = prng.rng.random_range(1..max_send_size) as usize;
                    let mut data: Vec<u8> = vec![0; random_size];

                    prng.rng.fill_bytes(&mut data[..]);

                    state.substate_mut::<EchoClientState>().status = EchoClientStatus::Sending {
                        connection,
                        request,
                        data: data.clone(),
                    };

                    dispatcher.dispatch(TcpClientAction::Send {
                        uid: request,
                        connection,
                        data: data.into(),
                        timeout: Timeout::Millis(200),
                        on_success: callback!(|uid: Uid| EchoClientAction::SendSuccess { uid }),
                        on_timeout: callback!(|uid: Uid| EchoClientAction::SendTimeout { uid }),
                        on_error: callback!(|(uid: Uid, error: String)| EchoClientAction::SendError { uid, error })
                    });
                }
            }
            EchoClientAction::PollError { uid, error } => {
                panic!("Poll {:?} failed: {}", uid, error)
            }
            EchoClientAction::SendSuccess { uid } => {
                // Receive back what we sent
                if let EchoClientState {
                    status:
                        EchoClientStatus::Sending {
                            connection,
                            request,
                            data,
                        },
                    config:
                        EchoClientConfig {
                            min_rnd_timeout,
                            max_rnd_timeout,
                            ..
                        },
                    ..
                } = state.substate()
                {
                    assert_eq!(uid, *request);
                    let connection = *connection;
                    let sent_data = data.clone();
                    let count = data.len();

                    // We randomize client's recv timeout to force it fail sometimes
                    let timeout_range = *min_rnd_timeout..*max_rnd_timeout;
                    let prng: &mut PRNGState = state.substate_mut();
                    let timeout = Timeout::Millis(prng.rng.random_range(timeout_range));

                    let request = state.new_uid();

                    info!(
                        "|ECHO_CLIENT| dispatching recv request {:?} ({} bytes) from connection {:?} with timeout {:?}",
                        request, count, connection, timeout
                    );

                    state.substate_mut::<EchoClientState>().status = EchoClientStatus::Receiving {
                        connection,
                        request,
                        sent_data,
                    };

                    dispatcher.dispatch(TcpClientAction::Recv {
                        uid: request,
                        connection,
                        count,
                        timeout,
                        on_success: callback!(|(uid: Uid, data: Vec<u8>)| EchoClientAction::RecvSuccess { uid, data }),
                        on_timeout: callback!(|(uid: Uid, partial_data: Vec<u8>)| EchoClientAction::RecvTimeout { uid, partial_data }),
                        on_error: callback!(|(uid: Uid, error: String)| EchoClientAction::RecvError { uid, error }),
                    });
                } else {
                    unreachable!()
                }
            }
            EchoClientAction::SendTimeout { uid } => {
                if let EchoClientState {
                    status: EchoClientStatus::Sending { connection, .. },
                    ..
                } = state.substate()
                {
                    let connection = *connection;
                    warn!(
                        "|ECHO_CLIENT| send {:?} timeout to connection {:?}",
                        uid, connection
                    );
                    dispatcher.dispatch(TcpClientAction::Close { connection })
                } else {
                    unreachable!()
                }
            }
            EchoClientAction::SendError { uid, error } => {
                if let EchoClientState {
                    status: EchoClientStatus::Sending { connection, .. },
                    ..
                } = state.substate()
                {
                    warn!(
                        "|ECHO_CLIENT| send {:?} to connection {:?} error: {}",
                        uid, connection, error
                    );
                } else {
                    unreachable!()
                }
            }
            EchoClientAction::RecvSuccess { uid, data } => {
                if let EchoClientState {
                    status:
                        EchoClientStatus::Receiving {
                            connection,
                            request,
                            sent_data,
                        },
                    ..
                } = state.substate()
                {
                    assert_eq!(uid, *request);
                    let connection = *connection;

                    if *sent_data != data {
                        panic!("Data mismatch: {:?} != {:?}", sent_data, data)
                    }

                    state.substate_mut::<EchoClientState>().status =
                        EchoClientStatus::Connected { connection };

                    info!(
                        "|ECHO_CLIENT| recv {:?} from connection {:?}, data matches.",
                        uid, connection
                    );
                } else {
                    unreachable!()
                }
            }
            EchoClientAction::RecvTimeout { uid, .. } => {
                if let EchoClientState {
                    status:
                        EchoClientStatus::Receiving {
                            connection,
                            request,
                            ..
                        },
                    ..
                } = state.substate()
                {
                    assert_eq!(uid, *request);
                    let connection = *connection;

                    warn!(
                        "|ECHO_CLIENT| recv {:?} timeout from connection {:?}",
                        uid, connection
                    );
                    dispatcher.dispatch(TcpClientAction::Close { connection })
                } else {
                    unreachable!()
                }
            }
            EchoClientAction::RecvError { uid, error } => {
                if let EchoClientState {
                    status:
                        EchoClientStatus::Receiving {
                            connection,
                            request,
                            ..
                        },
                    ..
                } = state.substate()
                {
                    assert_eq!(uid, *request);
                    let connection = *connection;

                    warn!(
                        "|ECHO_CLIENT| recv {:?} from connection {:?} error: {}",
                        uid, connection, error
                    );
                } else {
                    unreachable!()
                }
            }
        }
    }
}

fn connect(client_state: &EchoClientState, connection: Uid, dispatcher: &mut Dispatcher) {
    let EchoClientState {
        config:
            EchoClientConfig {
                connect_to_address,
                connect_timeout,
                ..
            },
        ..
    } = client_state;

    dispatcher.dispatch(TcpClientAction::Connect {
        connection,
        address: connect_to_address.clone(),
        timeout: connect_timeout.clone(),
        on_success: callback!(|connection: Uid| EchoClientAction::ConnectSuccess { connection }),
        on_timeout: callback!(|connection: Uid| EchoClientAction::ConnectTimeout { connection }),
        on_error: callback!(|(connection: Uid, error: String)| EchoClientAction::ConnectError { connection, error }),
        on_close: callback!(|connection: Uid| EchoClientAction::CloseEvent { connection })
    });
}
