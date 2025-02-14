use super::{
    action::TcpServerAction,
    state::{Listener, PollRequest, RecvRequest, SendRequest, TcpServerState},
};
use crate::automaton::{
    Dispatcher, ModelState, PureModel, RegisterModel, RunnerBuilder, State, Uid,
};
use crate::{
    callback,
    models::pure::net::tcp::{
        action::{Event, ListenerEvent, TcpAction, TcpPollEvents},
        state::TcpState,
    },
};
use log::warn;

// The `TcpServerState` model is an abstraction layer over the `TcpState` model
// providing a simpler interface for working with TCP server operations.

// This model depends on the `TcpState` model.
impl RegisterModel for TcpServerState {
    fn register<Substate: ModelState>(builder: RunnerBuilder<Substate>) -> RunnerBuilder<Substate> {
        builder.register::<TcpState>().model_pure::<Self>()
    }
}

impl PureModel for TcpServerState {
    type Action = TcpServerAction;

    fn process_pure<Substate: ModelState>(
        state: &mut State<Substate>,
        action: Self::Action,
        dispatcher: &mut Dispatcher,
    ) {
        match action {
            TcpServerAction::New {
                address,
                listener,
                max_connections,
                on_success,
                on_error,
                on_new_connection,
                on_connection_closed,
                on_listener_closed,
            } => {
                state.substate_mut::<TcpServerState>().new_listener(
                    listener,
                    max_connections,
                    on_success,
                    on_error,
                    on_new_connection,
                    on_connection_closed,
                    on_listener_closed,
                );

                dispatcher.dispatch(TcpAction::Listen {
                    listener,
                    address,
                    on_success: callback!(|listener: Uid| TcpServerAction::NewSuccess { listener }),
                    on_error: callback!(|(listener: Uid, error: String)| TcpServerAction::NewError { listener, error })
                });
            }
            TcpServerAction::NewSuccess { listener } => {
                let Listener { on_success, .. } =
                    state.substate::<TcpServerState>().get_listener(&listener);

                dispatcher.dispatch_back(on_success, listener);
            }
            TcpServerAction::NewError { listener, error } => {
                let server_state: &mut TcpServerState = state.substate_mut();
                let Listener { on_error, .. } = server_state.get_listener(&listener);

                dispatcher.dispatch_back(on_error, (listener, error));
                server_state.remove_listener(&listener);
            }
            TcpServerAction::Poll {
                uid,
                timeout,
                on_success,
                on_error,
            } => {
                let server_state: &mut TcpServerState = state.substate_mut();
                let objects = server_state.listeners.keys().cloned().collect();

                server_state.set_poll_request(PollRequest {
                    on_success,
                    on_error,
                });
                dispatcher.dispatch(TcpAction::Poll {
                    uid,
                    objects,
                    timeout,
                    on_success: callback!(|(uid: Uid, events: TcpPollEvents)| TcpServerAction::PollSuccess { uid, events } ),
                    on_error: callback!(|(uid: Uid, error: String)| TcpServerAction::PollError { uid, error } ),
                })
            }
            TcpServerAction::PollSuccess { uid, events } => {
                let PollRequest { on_success, .. } =
                    state.substate_mut::<TcpServerState>().take_poll_request();

                process_poll_events(state, dispatcher, events);
                dispatcher.dispatch_back(&on_success, uid)
            }
            TcpServerAction::PollError { uid, error } => {
                let PollRequest { on_error, .. } =
                    state.substate_mut::<TcpServerState>().take_poll_request();

                dispatcher.dispatch_back(&on_error, (uid, error))
            }
            TcpServerAction::AcceptSuccess { connection } => {
                let (
                    listener,
                    Listener {
                        max_connections,
                        on_new_connection,
                        connections,
                        ..
                    },
                ) = state
                    .substate_mut::<TcpServerState>()
                    .get_connection_listener_mut(&connection);

                // When we reach the max allowed connections we close it, without notifications.
                // TODO: this could probably better handled at low-level by changing the TcpListener backlog.
                // Currently, MIO sets a fixed value of 1024.
                if connections.len() > *max_connections {
                    dispatcher.dispatch(TcpAction::Close {
                        connection,
                        on_success: callback!(|connection: Uid| {
                            TcpServerAction::CloseEventInternal { connection }
                        }),
                    })
                } else {
                    // otherwise we notify the model user of the new connection.
                    dispatcher.dispatch_back(on_new_connection, (*listener, connection))
                }
            }
            TcpServerAction::AcceptTryAgain { connection } => {
                // No new connections, ignore.
                let (_, listener_object) = state
                    .substate_mut::<TcpServerState>()
                    .get_connection_listener_mut(&connection);

                listener_object.remove_connection(&connection)
            }
            TcpServerAction::AcceptError { connection, error } => {
                let (_, listener_object) = state
                    .substate_mut::<TcpServerState>()
                    .get_connection_listener_mut(&connection);

                warn!("|TCP_SERVER| accept {:?} failed: {:?}", connection, error);
                listener_object.remove_connection(&connection)
            }
            TcpServerAction::Close { connection } => dispatcher.dispatch(TcpAction::Close {
                connection,
                on_success: callback!(|connection: Uid| TcpServerAction::CloseEventNotify {
                    connection
                }),
            }),
            TcpServerAction::CloseEventInternal { connection } => {
                let (_, listener_object) = state
                    .substate_mut::<TcpServerState>()
                    .get_connection_listener_mut(&connection);

                listener_object.remove_connection(&connection)
            }
            TcpServerAction::CloseEventNotify { connection } => {
                let (listener, listener_object) = state
                    .substate_mut::<TcpServerState>()
                    .get_connection_listener_mut(&connection);

                dispatcher.dispatch_back(
                    &listener_object.on_connection_closed,
                    (*listener, connection),
                );
                listener_object.remove_connection(&connection)
            }
            TcpServerAction::Send {
                uid,
                connection,
                data,
                timeout,
                on_success,
                on_timeout,
                on_error,
            } => {
                state
                    .substate_mut::<TcpServerState>()
                    .new_send_request(&uid, connection, on_success, on_timeout, on_error);

                dispatcher.dispatch(TcpAction::Send {
                    uid,
                    connection,
                    data,
                    timeout,
                    on_success: callback!(|uid: Uid| TcpServerAction::SendSuccess { uid }),
                    on_timeout: callback!(|uid: Uid| TcpServerAction::SendTimeout { uid }),
                    on_error: callback!(|(uid: Uid, error: String)| TcpServerAction::SendError { uid, error }),
                });
            }
            TcpServerAction::SendSuccess { uid } => {
                let SendRequest { on_success, .. } = state
                    .substate_mut::<TcpServerState>()
                    .take_send_request(&uid);

                dispatcher.dispatch_back(&on_success, uid)
            }
            TcpServerAction::SendTimeout { uid } => {
                let SendRequest { on_timeout, .. } = state
                    .substate_mut::<TcpServerState>()
                    .take_send_request(&uid);

                dispatcher.dispatch_back(&on_timeout, uid)
            }
            TcpServerAction::SendError { uid, error } => {
                let SendRequest {
                    connection,
                    on_error,
                    ..
                } = state
                    .substate_mut::<TcpServerState>()
                    .take_send_request(&uid);

                dispatcher.dispatch_back(&on_error, (uid, error));
                // close the connection on send errors
                dispatcher.dispatch(TcpAction::Close {
                    connection,
                    on_success: callback!(|connection: Uid| TcpServerAction::CloseEventNotify {
                        connection
                    }),
                });
            }
            TcpServerAction::Recv {
                uid,
                connection,
                count,
                timeout,
                on_success,
                on_timeout,
                on_error,
            } => {
                state
                    .substate_mut::<TcpServerState>()
                    .new_recv_request(&uid, connection, on_success, on_timeout, on_error);

                dispatcher.dispatch(TcpAction::Recv {
                    uid,
                    connection,
                    count,
                    timeout,
                    on_success: callback!(|(uid: Uid, data: Vec<u8>)| TcpServerAction::RecvSuccess { uid, data }),
                    on_timeout: callback!(|(uid: Uid, partial_data: Vec<u8>)| TcpServerAction::RecvTimeout { uid, partial_data }),
                    on_error: callback!(|(uid: Uid, error: String)| TcpServerAction::RecvError { uid, error }),
                });
            }
            TcpServerAction::RecvSuccess { uid, data } => {
                let RecvRequest { on_success, .. } = state
                    .substate_mut::<TcpServerState>()
                    .take_recv_request(&uid);

                dispatcher.dispatch_back(&on_success, (uid, data))
            }
            TcpServerAction::RecvTimeout { uid, partial_data } => {
                let RecvRequest { on_timeout, .. } = state
                    .substate_mut::<TcpServerState>()
                    .take_recv_request(&uid);

                dispatcher.dispatch_back(&on_timeout, (uid, partial_data))
            }
            TcpServerAction::RecvError { uid, error } => {
                let RecvRequest {
                    connection,
                    on_error,
                    ..
                } = state
                    .substate_mut::<TcpServerState>()
                    .take_recv_request(&uid);

                dispatcher.dispatch_back(&on_error, (uid, error));

                // close the connection on recv errors
                dispatcher.dispatch(TcpAction::Close {
                    connection,
                    on_success: callback!(|connection: Uid| TcpServerAction::CloseEventNotify {
                        connection
                    }),
                })
            }
        }
    }
}

fn process_poll_events<Substate: ModelState>(
    state: &mut State<Substate>,
    dispatcher: &mut Dispatcher,
    events: TcpPollEvents,
) {
    for (listener, ev) in events {
        if let Event::Listener(event) = ev {
            match event {
                ListenerEvent::AcceptPending => {
                    let connection = state.new_uid();
                    state
                        .substate_mut::<TcpServerState>()
                        .new_connection(connection, listener);

                    dispatcher.dispatch(TcpAction::Accept {
                        connection,
                        listener,
                        on_success: callback!(|connection: Uid| TcpServerAction::AcceptSuccess { connection }),
                        on_would_block: callback!(|connection: Uid| TcpServerAction::AcceptTryAgain { connection }),
                        on_error: callback!(|(connection: Uid, error: String)| TcpServerAction::AcceptError { connection, error }),
                    });
                }
                ListenerEvent::AllAccepted => (),
                ListenerEvent::Closed | ListenerEvent::Error => {
                    let Listener {
                        on_listener_closed, ..
                    } = state
                        .substate_mut::<TcpServerState>()
                        .remove_listener(&listener);

                    dispatcher.dispatch_back(&on_listener_closed, listener)
                }
            }
        } else {
            unreachable!()
        }
    }
}
