use super::{
    action::EchoServerAction,
    state::{EchoServerConfig, EchoServerState, EchoServerStatus},
};
use crate::automaton::{
    Dispatcher, ModelState, Objects, PureModel, RegisterModel, RunnerBuilder, State, Timeout, Uid,
};
use crate::{
    callback,
    models::pure::{
        net::{
            tcp::action::TcpAction,
            tcp_server::{action::TcpServerAction, state::TcpServerState},
        },
        tests::echo_server::state::Connection,
        time::model::update_time,
    },
};
use log::{info, warn};

/// The `EchoServerState` model simulates an echo server, used for testing the
/// functionality of the state-machine and its models (`TcpServerState`,
/// `TcpState`, `MioState`, `TimeState`). The echo server receives data from an
/// echo client and sends the same data back to the client.
///
/// This model provides a high-level interface for testing the state-machine's
/// handling of server-side TCP operations, which includes managing multiple
/// client connections.
///
/// The `PureModel` implementation of the `EchoServerState` model processes
/// `EchoServerAction::Tick` actions that are dispatched at each "tick" of the
/// state-machine loop.
///
/// During each "tick", the model performs two key tasks:
///
/// 1. Updates the current time tracked by the state-machine.
///
/// 2. Checks if the server is ready. If it's not, the model initializes the
///    server. If the server is ready, a poll action is dispatched to check for
///    incoming data.
///
/// The rest of the server's logic performs the following actions:
///
/// - It completes the initialization of the server and starts listening for
///   incoming connections. If the initialization fails, the server panics.
///
/// - For each poll result, the server receives data from connected clients.
///
/// - After receiving data, the server sends the same data back to the client.
///
/// This model depends on `TcpServerState`.
impl RegisterModel for EchoServerState {
    fn register<Substate: ModelState>(builder: RunnerBuilder<Substate>) -> RunnerBuilder<Substate> {
        builder.register::<TcpServerState>().model_pure::<Self>()
    }
}

impl PureModel for EchoServerState {
    type Action = EchoServerAction;

    fn process_pure<Substate: ModelState>(
        state: &mut State<Substate>,
        action: Self::Action,
        dispatcher: &mut Dispatcher,
    ) {
        match action {
            EchoServerAction::Tick => {
                // Top-most model first task is to update the state-machine time.
                if update_time(state, dispatcher) {
                    return;
                }

                let EchoServerState {
                    status,
                    config: EchoServerConfig { poll_timeout, .. },
                    ..
                } = state.substate_mut();

                match status {
                    EchoServerStatus::Init => {
                        // Init TCP model
                        dispatcher.dispatch(TcpAction::Init {
                            instance: state.new_uid(),
                            on_success: callback!(|instance: Uid| EchoServerAction::InitSuccess { instance }),
                            on_error: callback!(|(instance: Uid, error: String)| EchoServerAction::InitError { instance, error }),
                        })
                    }
                    EchoServerStatus::Listening { .. } => {
                        let timeout = Timeout::Millis(*poll_timeout);

                        dispatcher.dispatch(TcpServerAction::Poll {
                            uid: state.new_uid(),
                            timeout,
                            on_success: callback!(|uid: Uid| EchoServerAction::PollSuccess { uid }),
                            on_error: callback!(|(uid: Uid, error: String)| EchoServerAction::PollError { uid, error }),
                        })
                    }
                }

                if update_time(state, dispatcher) {
                    return;
                }
            }
            EchoServerAction::InitSuccess { .. } => {
                let EchoServerState { config, .. } = state.substate();
                let address = config.address.clone();
                let max_connections = config.max_connections;

                // Init TcpServer model
                dispatcher.dispatch(TcpServerAction::New {
                    listener: state.new_uid(),
                    address,
                    max_connections,
                    on_success: callback!(|listener: Uid| EchoServerAction::InitListenerSuccess { listener }),
                    on_error: callback!(|(listener: Uid, error: String)| EchoServerAction::InitListenerError { listener, error }),
                    on_new_connection: callback!(|(listener: Uid, connection: Uid)| EchoServerAction::ConnectionEvent { listener, connection }),
                    on_connection_closed: callback!(|(listener: Uid, connection: Uid)| EchoServerAction::CloseEvent { listener, connection }),
                    on_listener_closed: callback!(|listener: Uid| EchoServerAction::ListenerCloseEvent { listener }),
                });
            }
            EchoServerAction::InitError { error, .. } => {
                panic!("Server initialization failed: {}", error)
            }
            EchoServerAction::InitListenerSuccess { .. } => {
                state.substate_mut::<EchoServerState>().status = EchoServerStatus::Listening {
                    connections: Objects::<Connection>::new(),
                }
            }
            EchoServerAction::InitListenerError { listener, error } => {
                panic!("Listener {:?} initialization failed: {}", listener, error)
            }
            EchoServerAction::ConnectionEvent { connection, .. } => {
                state
                    .substate_mut::<EchoServerState>()
                    .new_connection(connection);

                info!("|ECHO_SERVER| new connection {:?}", connection);
            }
            EchoServerAction::ListenerCloseEvent { .. } => {
                todo!()
            }
            EchoServerAction::CloseEvent { connection, .. } => {
                state
                    .substate_mut::<EchoServerState>()
                    .remove_connection(&connection);

                info!("|ECHO_SERVER| connection {:?} closed", connection);
            }
            EchoServerAction::PollSuccess { .. } => {
                let server_state: &EchoServerState = state.substate();
                let timeout = Timeout::Millis(server_state.config.recv_timeout);
                let count = 1024;

                for connection in server_state.connections_ready_to_recv() {
                    let uid = state.new_uid();

                    info!(
                        "|ECHO_SERVER| dispatching recv request {:?} ({} bytes), connection {:?}, timeout {:?}",
                        uid, count, connection, timeout
                    );

                    dispatcher.dispatch(TcpServerAction::Recv {
                        uid,
                        connection,
                        count,
                        timeout: timeout.clone(),
                        on_success: callback!(|(uid: Uid, data: Vec<u8>)| EchoServerAction::RecvSuccess { uid, data }),
                        on_timeout: callback!(|(uid: Uid, partial_data: Vec<u8>)| EchoServerAction::RecvTimeout { uid, partial_data }),
                        on_error: callback!(|(uid: Uid, error: String)| EchoServerAction::RecvError { uid, error }),
                    });

                    *state
                        .substate_mut::<EchoServerState>()
                        .get_connection_mut(&connection) = Connection::Receiving { request: uid };
                }
            }
            EchoServerAction::PollError { uid, error } => {
                panic!("Poll {:?} failed: {}", uid, error)
            }
            EchoServerAction::RecvSuccess { uid, data } => {
                let connection = state
                    .substate::<EchoServerState>()
                    .find_connection_uid_by_recv_uid(uid);
                let request = state.new_uid();

                // send data back to client
                dispatcher.dispatch(TcpServerAction::Send {
                    uid: request,
                    connection,
                    data: data.into(),
                    timeout: Timeout::Millis(100), // TODO: configurable
                    on_success: callback!(|uid: Uid| EchoServerAction::SendSuccess { uid }),
                    on_timeout: callback!(|uid: Uid| EchoServerAction::SendTimeout { uid }),
                    on_error: callback!(|(uid: Uid, error: String)| EchoServerAction::SendError { uid, error }),
                });

                *state
                    .substate_mut::<EchoServerState>()
                    .get_connection_mut(&connection) = Connection::Sending { request };
            }
            EchoServerAction::RecvTimeout { uid, partial_data } => {
                let connection = state
                    .substate::<EchoServerState>()
                    .find_connection_uid_by_recv_uid(uid);

                if partial_data.len() > 0 {
                    let request = state.new_uid();

                    // send partial data back to client
                    dispatcher.dispatch(TcpServerAction::Send {
                            uid: request,
                            connection,
                            data: partial_data.into(),
                            timeout: Timeout::Millis(100), // TODO: configurable
                            on_success: callback!(|uid: Uid| EchoServerAction::SendSuccess { uid }),
                            on_timeout: callback!(|uid: Uid| EchoServerAction::SendTimeout { uid }),
                            on_error: callback!(|(uid: Uid, error: String)| EchoServerAction::SendError { uid, error }),
                        });

                    *state
                        .substate_mut::<EchoServerState>()
                        .get_connection_mut(&connection) = Connection::Sending { request };
                } else {
                    // if we didn't receive anything in the time span close the connection
                    dispatcher.dispatch(TcpServerAction::Close { connection });
                    warn!("|ECHO_SERVER| recv {:?} timeout", uid)
                }
            }
            EchoServerAction::RecvError { uid, error } => {
                // CloseEvent is dispatched by the TcpServer model and handles the rest
                warn!("|ECHO_SERVER| recv {:?} error: {:?}", uid, error);
            }
            EchoServerAction::SendSuccess { uid } => {
                let server_state: &mut EchoServerState = state.substate_mut();
                let connection = server_state.find_connection_uid_by_send_uid(uid);

                *server_state.get_connection_mut(&connection) = Connection::Ready;
            }
            EchoServerAction::SendTimeout { uid } => {
                let connection = state
                    .substate_mut::<EchoServerState>()
                    .find_connection_uid_by_send_uid(uid);

                dispatcher.dispatch(TcpServerAction::Close { connection });
                warn!("|ECHO_SERVER| send {:?} timeout", uid)
            }
            EchoServerAction::SendError { uid, error } => {
                // CloseEvent is dispatched by the TcpServer model and handles the rest
                warn!("|ECHO_SERVER| send {:?} error: {:?}", uid, error)
            }
        }
    }
}
