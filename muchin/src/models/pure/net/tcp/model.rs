use super::{
    action::{ListenerEvent, TcpAction},
    state::{ConnectionStatus, EventUpdater, Listener, RecvRequest, Status, TcpState},
    util::*,
};
use crate::automaton::{
    Dispatcher, ModelState, PureModel, RegisterModel, RunnerBuilder, State, TimeoutAbsolute, Uid,
};
use crate::{
    callback,
    models::{
        effectful::mio::{
            action::{MioEffectfulAction, MioEvent},
            state::MioState,
        },
        pure::{
            net::tcp::state::{Connection, ConnectionType, PollRequest},
            time::{
                model::{get_current_time, get_timeout_absolute},
                state::TimeState,
            },
        },
    },
};
use core::panic;

// The `TcpState` model handles the state of a TCP connection system, which is
// built on top of the `MioState` model. It processes the outcomes of external
// inputs (the results of `MioState` actions).
//
// This model facilitates various operations, including:
// - Creating polls.
// - Establishing connections to remote peers.
// - Listening for connections.
// - Sending and receiving data.
//
// Another feature provided by this model is timeout support for the async IO.
// While the `TcpState` model simplifies some aspects of the `MioState` model,
// it's still pretty low-level. For simpler use, there are the `TcpClientState`
// and `TcpServerState` models, which are built on top of the `TcpState` model.

// This model depends on the `TimeState` (pure) and `MioState` (effectful).
impl RegisterModel for TcpState {
    fn register<Substate: ModelState>(builder: RunnerBuilder<Substate>) -> RunnerBuilder<Substate> {
        builder
            .register::<TimeState>()
            .register::<MioState>()
            .model_pure::<Self>()
    }
}

impl PureModel for TcpState {
    type Action = TcpAction;

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
            TcpAction::PollCreateSuccess { .. } => {
                let events = state.new_uid();
                let tcp_state: &mut TcpState = state.substate_mut();

                if let Status::InitPollCreate {
                    instance,
                    poll,
                    on_success,
                    ..
                } = tcp_state.status.clone()
                {
                    // Dispatch next action to continue initialization
                    dispatcher.dispatch_effect(MioEffectfulAction::EventsCreate {
                        uid: events,
                        capacity: 1024,
                        on_success: callback!(|uid: Uid| TcpAction::EventsCreate { uid }),
                    });

                    // next init state
                    tcp_state.status = Status::InitEventsCreate {
                        instance,
                        poll,
                        events,
                        on_success,
                    };
                } else {
                    unreachable!()
                }
            }
            TcpAction::PollCreateError { error, .. } => {
                let tcp_state: &mut TcpState = state.substate_mut();

                if let Status::InitPollCreate {
                    instance, on_error, ..
                } = tcp_state.status.clone()
                {
                    // dispatch error to caller
                    dispatcher.dispatch_back(&on_error, (instance, error));
                    tcp_state.status = Status::InitError { instance };
                } else {
                    unreachable!()
                }
            }
            TcpAction::EventsCreate { .. } => {
                let tcp_state: &mut TcpState = state.substate_mut();

                if let Status::InitEventsCreate {
                    instance,
                    poll,
                    events,
                    on_success,
                } = tcp_state.status.clone()
                {
                    dispatcher.dispatch_back(&on_success, instance);
                    tcp_state.status = Status::Ready {
                        instance,
                        poll,
                        events,
                    };
                } else {
                    unreachable!()
                }
            }
            TcpAction::Listen {
                listener,
                address,
                on_success,
                on_error,
            } => {
                let tcp_state: &mut TcpState = state.substate_mut();

                tcp_state.new_listener(listener, address.clone(), on_success, on_error);
                dispatcher.dispatch_effect(MioEffectfulAction::TcpListen {
                    listener,
                    address,
                    on_success: callback!(|listener: Uid| TcpAction::ListenSuccess { listener }),
                    on_error: callback!(|(listener: Uid, error: String)| TcpAction::ListenError { listener, error })
                });
            }
            TcpAction::ListenSuccess { listener } => {
                // If the listen operation was successful we register the listener in the MIO poll object.
                if let Status::Ready { poll, .. } = state.substate_mut::<TcpState>().status {
                    dispatcher.dispatch_effect(MioEffectfulAction::PollRegisterTcpServer {
                        poll,
                        listener,
                        on_success: callback!(|listener: Uid| TcpAction::RegisterListenerSuccess { listener }),
                        on_error: callback!(|(listener: Uid, error: String)| TcpAction::RegisterListenerError { listener, error }),
                    });
                } else {
                    unreachable!()
                };
            }
            TcpAction::ListenError { listener, error } => {
                let tcp_state: &mut TcpState = state.substate_mut();
                let Listener { on_error, .. } = tcp_state.get_listener(&listener);

                dispatcher.dispatch_back(on_error, (listener, error));
                tcp_state.remove_listener(&listener);
            }
            TcpAction::RegisterListenerSuccess { listener } => {
                let tcp_state: &TcpState = state.substate();
                let Listener { on_success, .. } = tcp_state.get_listener(&listener);

                dispatcher.dispatch_back(on_success, listener);
            }
            TcpAction::RegisterListenerError { listener, error } => {
                let tcp_state = state.substate_mut::<TcpState>();
                let Listener { on_error, .. } = tcp_state.get_listener(&listener);

                dispatcher.dispatch_back(&on_error, (listener, error));
                tcp_state.remove_listener(&listener)
            }
            TcpAction::Accept {
                connection,
                listener,
                on_success,
                on_would_block,
                on_error,
            } => {
                let tcp_state: &mut TcpState = state.substate_mut();

                if let ListenerEvent::AcceptPending = tcp_state.get_listener(&listener).events() {
                    tcp_state.new_connection(
                        connection,
                        ConnectionType::Incoming {
                            listener,
                            on_success,
                            on_would_block,
                            on_error,
                        },
                        TimeoutAbsolute::Never,
                    );
                    dispatcher.dispatch_effect(MioEffectfulAction::TcpAccept {
                        connection,
                        listener,
                        on_success: callback!(|connection: Uid| TcpAction::AcceptSuccess { connection }),
                        on_would_block: callback!(|connection: Uid| TcpAction::AcceptTryAgain { connection }),
                        on_error: callback!(|(connection: Uid, error: String)| TcpAction::AcceptError { connection, error })
                    });
                } else {
                    unreachable!()
                }
            }
            TcpAction::AcceptSuccess { connection } => {
                let tcp_state: &mut TcpState = state.substate_mut();

                if let ConnectionType::Incoming { .. } =
                    tcp_state.get_connection(&connection).conn_type
                {
                    if let Status::Ready { poll, .. } = tcp_state.status {
                        dispatcher.dispatch_effect(MioEffectfulAction::PollRegisterTcpConnection {
                            poll,
                            connection,
                            on_success: callback!(|connection: Uid| TcpAction::RegisterConnectionSuccess { connection }),
                            on_error: callback!(|(connection: Uid, error: String)| TcpAction::RegisterConnectionError { connection, error }),
                        })
                    } else {
                        unreachable!()
                    }
                } else {
                    unreachable!()
                }
            }
            TcpAction::AcceptTryAgain { connection } => {
                let tcp_state: &mut TcpState = state.substate_mut();

                if let ConnectionType::Incoming {
                    listener,
                    on_would_block,
                    ..
                } = tcp_state.get_connection(&connection).conn_type.clone()
                {
                    dispatcher.dispatch_back(&on_would_block, connection);

                    let events = tcp_state.get_listener_mut(&listener).events_mut();

                    if let ListenerEvent::AcceptPending = events {
                        *events = ListenerEvent::AllAccepted;
                        tcp_state.remove_connection(&connection)
                    } else {
                        unreachable!()
                    }
                } else {
                    unreachable!()
                }
            }
            TcpAction::AcceptError { connection, error } => {
                let tcp_state: &mut TcpState = state.substate_mut();

                if let ConnectionType::Incoming { on_error, .. } =
                    tcp_state.get_connection(&connection).conn_type.clone()
                {
                    dispatcher.dispatch_back(&on_error, (connection, error));
                    tcp_state.remove_connection(&connection)
                } else {
                    unreachable!()
                };
            }
            TcpAction::Connect {
                connection,
                address,
                timeout,
                on_success,
                on_timeout,
                on_error,
            } => {
                let timeout = get_timeout_absolute(state, timeout);

                state.substate_mut::<TcpState>().new_connection(
                    connection,
                    ConnectionType::Outgoing {
                        on_success,
                        on_timeout,
                        on_error,
                    },
                    timeout,
                );
                dispatcher.dispatch_effect(MioEffectfulAction::TcpConnect {
                    connection,
                    address,
                    on_success: callback!(|connection: Uid| TcpAction::ConnectSuccess { connection }),
                    on_error: callback!(|(connection: Uid, error: String)| TcpAction::ConnectError { connection, error })
                });
            }
            TcpAction::ConnectSuccess { connection } => {
                if let Status::Ready { poll, .. } = state.substate::<TcpState>().status {
                    dispatcher.dispatch_effect(MioEffectfulAction::PollRegisterTcpConnection {
                        poll,
                        connection,
                        on_success: callback!(|connection: Uid| TcpAction::RegisterConnectionSuccess { connection }),
                        on_error: callback!(|(connection: Uid, error: String)| TcpAction::RegisterConnectionError { connection, error }),
                    });
                } else {
                    unreachable!()
                };
            }
            TcpAction::ConnectError { connection, error } => {
                let tcp_state: &mut TcpState = state.substate_mut();

                if let ConnectionType::Outgoing { on_error, .. } =
                    tcp_state.get_connection(&connection).conn_type.clone()
                {
                    dispatcher.dispatch_back(&on_error, (connection, error));
                    tcp_state.remove_connection(&connection);
                } else {
                    unreachable!()
                };
            }
            TcpAction::RegisterConnectionSuccess { connection } => {
                // Ignore outgoing connections
                if let ConnectionType::Incoming { on_success, .. } = state
                    .substate::<TcpState>()
                    .get_connection(&connection)
                    .conn_type
                    .clone()
                {
                    dispatcher.dispatch_back(&on_success, connection);
                }
            }
            TcpAction::RegisterConnectionError { connection, error } => {
                let conn = state
                    .substate_mut::<TcpState>()
                    .get_connection_mut(&connection);

                conn.status = ConnectionStatus::CloseRequestInternal;
                dispatcher.dispatch_effect(MioEffectfulAction::TcpClose {
                    connection,
                    on_success: callback!(|connection: Uid| TcpAction::CloseSuccess { connection }),
                });

                dispatcher.dispatch_back(
                    &conn.conn_type.on_error(),
                    (
                        connection,
                        format!("Error registering connection {:?}: {}", connection, error),
                    ),
                )
            }
            TcpAction::Close {
                connection,
                on_success,
            } => {
                let tcp_state: &mut TcpState = state.substate_mut();

                if let Status::Ready { poll, .. } = tcp_state.status {
                    tcp_state.get_connection_mut(&connection).status =
                        ConnectionStatus::CloseRequestNotify { on_success };

                    // before closing the stream remove it from the poll object
                    dispatcher.dispatch_effect(MioEffectfulAction::PollDeregisterTcpConnection {
                        poll,
                        connection,
                        on_success: callback!(|connection: Uid| TcpAction::DeregisterConnectionSuccess { connection }),
                        on_error: callback!(|(connection: Uid, error: String)| TcpAction::DeregisterConnectionError { connection, error })
                    });
                } else {
                    unreachable!()
                };
            }
            TcpAction::DeregisterConnectionSuccess { connection } => {
                dispatcher.dispatch_effect(MioEffectfulAction::TcpClose {
                    connection,
                    on_success: callback!(|connection: Uid| TcpAction::CloseSuccess { connection }),
                })
            }
            TcpAction::DeregisterConnectionError { connection, error } => {
                panic!("DeregisterConnectionError {:?}: {}", connection, error)
            }
            TcpAction::CloseSuccess { connection } => {
                let tcp_state: &mut TcpState = state.substate_mut();

                match tcp_state.get_connection(&connection).status.clone() {
                    ConnectionStatus::CloseRequestNotify { on_success } => {
                        dispatcher.dispatch_back(&on_success, connection);
                        tcp_state.remove_connection(&connection)
                    }
                    ConnectionStatus::CloseRequestInternal => {
                        tcp_state.remove_connection(&connection)
                    }
                    _ => unreachable!(),
                }
            }
            TcpAction::Poll {
                uid,
                objects,
                timeout,
                on_success,
                on_error,
            } => {
                let tcp_state: &mut TcpState = state.substate_mut();

                if let Status::Ready { poll, events, .. } = tcp_state.status {
                    tcp_state.new_poll(uid, objects, timeout.clone(), on_success, on_error);
                    dispatcher.dispatch_effect(MioEffectfulAction::PollEvents {
                        uid,
                        poll,
                        events,
                        timeout,
                        on_success: callback!(|(uid: Uid, events: Vec<MioEvent>)| TcpAction::PollSuccess { uid, events }),
                        on_interrupted: callback!(|uid: Uid| TcpAction::PollInterrupted { uid }),
                        on_error: callback!(|(uid: Uid, error: String)| TcpAction::PollError { uid, error })
                    })
                } else {
                    unreachable!()
                };
            }
            TcpAction::PollSuccess { uid, events } => {
                let current_time = get_current_time(state);
                handle_poll_success(state.substate_mut(), dispatcher, current_time, uid, events)
            }
            TcpAction::PollInterrupted { uid } => {
                let tcp_state: &TcpState = state.substate();
                // if the syscall was interrupted we re-dispatch the MIO action
                if let Status::Ready { poll, events, .. } = tcp_state.status {
                    dispatcher.dispatch_effect(MioEffectfulAction::PollEvents {
                        uid,
                        poll,
                        events,
                        timeout: tcp_state.get_poll_request(&uid).timeout.clone(),
                        on_success: callback!(|(uid: Uid, events: Vec<MioEvent>)| TcpAction::PollSuccess { uid, events }),
                        on_interrupted: callback!(|uid: Uid| TcpAction::PollInterrupted { uid }),
                        on_error: callback!(|(uid: Uid, error: String)| TcpAction::PollError { uid, error }),
                    })
                } else {
                    unreachable!()
                };
            }
            TcpAction::PollError { uid, error } => {
                let tcp_state: &mut TcpState = state.substate_mut();
                let PollRequest { on_error, .. } = tcp_state.get_poll_request(&uid);

                dispatcher.dispatch_back(&on_error, (uid, error));
                tcp_state.remove_poll_request(&uid)
            }
            // dispatched from process_pending_connections()
            TcpAction::GetPeerAddressSuccess { connection, .. } => {
                let conn = state
                    .substate_mut::<TcpState>()
                    .get_connection_mut(&connection);

                if let Connection {
                    status: ConnectionStatus::PendingCheck,
                    conn_type: ConnectionType::Outgoing { on_success, .. },
                    ..
                } = conn
                {
                    conn.status = ConnectionStatus::Established;
                    dispatcher.dispatch_back(on_success, connection);
                } else {
                    unreachable!()
                };
            }
            TcpAction::GetPeerAddressError { connection, error } => {
                let tcp_state: &mut TcpState = state.substate_mut();

                if let Connection {
                    status: ConnectionStatus::PendingCheck,
                    conn_type: ConnectionType::Outgoing { on_error, .. },
                    ..
                } = tcp_state.get_connection_mut(&connection)
                {
                    dispatcher.dispatch_back(on_error, (connection, error));
                    tcp_state.remove_connection(&connection)
                } else {
                    unreachable!()
                };
            }
            TcpAction::Send {
                uid,
                connection,
                data,
                timeout,
                on_success,
                on_timeout,
                on_error,
            } => {
                let timeout = get_timeout_absolute(state, timeout);
                let tcp_state: &mut TcpState = state.substate_mut();

                if !tcp_state.has_connection(&connection) {
                    dispatcher.dispatch_back(
                        &on_error,
                        (uid, format!("No such connection: {:?}", connection)),
                    );
                } else {
                    tcp_state.new_send_request(
                        uid, connection, data, false, timeout, on_success, on_timeout, on_error,
                    );
                    dispatch_send(tcp_state, dispatcher, uid)
                }
            }
            // dispatched from dispatch_send()
            TcpAction::SendSuccess { uid } => {
                let tcp_state = state.substate_mut::<TcpState>();

                dispatcher.dispatch_back(&tcp_state.get_send_request(&uid).on_success, uid);
                tcp_state.remove_send_request(&uid)
            }
            TcpAction::SendSuccessPartial { uid, count } => {
                let current_time = get_current_time(state);
                let tcp_state = state.substate_mut::<TcpState>();

                tcp_state.get_send_request_mut(&uid).bytes_sent += count;
                handle_send_common(tcp_state, dispatcher, current_time, uid, true)
            }
            TcpAction::SendErrorInterrupted { uid } => {
                let current_time = get_current_time(state);

                handle_send_common(state.substate_mut(), dispatcher, current_time, uid, true)
            }
            TcpAction::SendErrorTryAgain { uid } => {
                let current_time = get_current_time(state);

                handle_send_common(state.substate_mut(), dispatcher, current_time, uid, false)
            }
            TcpAction::SendError { uid, error } => {
                let tcp_state: &mut TcpState = state.substate_mut();

                dispatcher.dispatch_back(&tcp_state.get_send_request(&uid).on_error, (uid, error));
                tcp_state.remove_send_request(&uid)
            }
            TcpAction::Recv {
                uid,
                connection,
                count,
                timeout,
                on_success,
                on_timeout,
                on_error,
            } => {
                let timeout = get_timeout_absolute(state, timeout);
                let tcp_state: &mut TcpState = state.substate_mut();

                if !tcp_state.has_connection(&connection) {
                    dispatcher.dispatch_back(
                        &on_error,
                        (uid, format!("No such connection: {:?}", connection)),
                    );
                } else {
                    tcp_state.new_recv_request(
                        uid, connection, count, false, timeout, on_success, on_timeout, on_error,
                    );
                    dispatch_recv(tcp_state, dispatcher, uid)
                }
            }
            TcpAction::RecvSuccess { uid, data } => {
                let tcp_state: &mut TcpState = state.substate_mut();
                let RecvRequest {
                    buffered_data,
                    remaining_bytes,
                    on_success,
                    ..
                } = tcp_state.get_recv_request_mut(&uid);

                *remaining_bytes = remaining_bytes
                    .checked_sub(data.len())
                    .expect("Received more data than requested");
                buffered_data.extend_from_slice(&data);
                dispatcher.dispatch_back(&on_success, (uid, buffered_data.clone()));
                tcp_state.remove_recv_request(&uid);
            }
            TcpAction::RecvSuccessPartial {
                uid,
                partial_data: data,
            } => {
                let current_time = get_current_time(state);
                let tcp_state: &mut TcpState = state.substate_mut();
                let RecvRequest {
                    buffered_data,
                    remaining_bytes,
                    ..
                } = tcp_state.get_recv_request_mut(&uid);

                *remaining_bytes = remaining_bytes
                    .checked_sub(data.len())
                    .expect("Received more data than requested");
                buffered_data.extend_from_slice(&data);
                handle_recv_common(tcp_state, dispatcher, current_time, uid, true)
            }
            TcpAction::RecvErrorInterrupted { uid } => {
                let current_time = get_current_time(state);

                handle_recv_common(state.substate_mut(), dispatcher, current_time, uid, true)
            }
            TcpAction::RecvErrorTryAgain { uid } => {
                let current_time = get_current_time(state);

                handle_recv_common(state.substate_mut(), dispatcher, current_time, uid, false)
            }
            TcpAction::RecvError { uid, error } => {
                let tcp_state = state.substate_mut::<TcpState>();

                dispatcher.dispatch_back(&tcp_state.get_recv_request(&uid).on_error, (uid, error));
                tcp_state.remove_recv_request(&uid)
            }
        }
    }
}
