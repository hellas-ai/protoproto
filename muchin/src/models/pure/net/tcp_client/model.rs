use super::{
    action::TcpClientAction,
    state::{RecvRequest, SendRequest, TcpClientState},
};
use crate::automaton::{
    Dispatcher, ModelState, PureModel, RegisterModel, RunnerBuilder, State, Uid,
};
use crate::{
    callback,
    models::pure::net::{
        tcp::{action::TcpAction, state::TcpState},
        tcp_client::state::Connection,
    },
};

// The `TcpClientState` model is an abstraction layer over the `TcpState` model
// providing a simpler interface for working with TCP client operations.

// This model depends on the `TcpState` model.
impl RegisterModel for TcpClientState {
    fn register<Substate: ModelState>(builder: RunnerBuilder<Substate>) -> RunnerBuilder<Substate> {
        builder.register::<TcpState>().model_pure::<Self>()
    }
}

impl PureModel for TcpClientState {
    type Action = TcpClientAction;

    fn process_pure<Substate: ModelState>(
        state: &mut State<Substate>,
        action: Self::Action,
        dispatcher: &mut Dispatcher,
    ) {
        match action {
            TcpClientAction::Poll {
                uid,
                timeout,
                on_success,
                on_error,
            } => dispatcher.dispatch(TcpAction::Poll {
                uid,
                objects: Vec::new(),
                timeout,
                on_success,
                on_error,
            }),
            TcpClientAction::Connect {
                connection,
                address,
                timeout,
                on_success,
                on_timeout,
                on_error,
                on_close,
            } => {
                state
                    .substate_mut::<TcpClientState>()
                    .new_connection(connection, on_success, on_timeout, on_error, on_close);

                dispatcher.dispatch(TcpAction::Connect {
                    connection,
                    address,
                    timeout,
                    on_success: callback!(|connection: Uid| TcpClientAction::ConnectSuccess { connection }),
                    on_timeout: callback!(|connection: Uid| TcpClientAction::ConnectTimeout { connection }),
                    on_error: callback!(|(connection: Uid, error: String)| TcpClientAction::ConnectError { connection, error }),
                });
            }
            TcpClientAction::ConnectSuccess { connection } => {
                let Connection { on_success, .. } = state
                    .substate::<TcpClientState>()
                    .get_connection(&connection);

                dispatcher.dispatch_back(on_success, connection);
            }
            TcpClientAction::ConnectTimeout { connection } => {
                let Connection { on_timeout, .. } = state
                    .substate::<TcpClientState>()
                    .get_connection(&connection);

                dispatcher.dispatch_back(on_timeout, connection);
            }
            TcpClientAction::ConnectError { connection, error } => {
                let Connection { on_error, .. } = state
                    .substate::<TcpClientState>()
                    .get_connection(&connection);

                dispatcher.dispatch_back(on_error, (connection, error));
            }
            TcpClientAction::Close { connection } => dispatcher.dispatch(TcpAction::Close {
                connection,
                on_success: callback!(|connection: Uid| TcpClientAction::CloseEventNotify {
                    connection
                }),
            }),
            TcpClientAction::CloseEventNotify { connection } => {
                let client_state: &mut TcpClientState = state.substate_mut();
                let Connection { on_close, .. } = client_state.get_connection(&connection);

                dispatcher.dispatch_back(&on_close, connection);
                client_state.remove_connection(&connection);
            }
            TcpClientAction::CloseEventInternal { connection } => {
                state
                    .substate_mut::<TcpClientState>()
                    .remove_connection(&connection);
            }
            TcpClientAction::Send {
                uid,
                connection,
                data,
                timeout,
                on_success,
                on_timeout,
                on_error,
            } => {
                state
                    .substate_mut::<TcpClientState>()
                    .new_send_request(&uid, connection, on_success, on_timeout, on_error);

                dispatcher.dispatch(TcpAction::Send {
                    uid,
                    connection,
                    data,
                    timeout,
                    on_success: callback!(|uid: Uid| TcpClientAction::SendSuccess { uid }),
                    on_timeout: callback!(|uid: Uid| TcpClientAction::SendTimeout { uid }),
                    on_error: callback!(|(uid: Uid, error: String)| TcpClientAction::SendError { uid, error }),
                });
            }
            TcpClientAction::SendSuccess { uid } => {
                let SendRequest { on_success, .. } = state
                    .substate_mut::<TcpClientState>()
                    .take_send_request(&uid);

                dispatcher.dispatch_back(&on_success, uid)
            }
            TcpClientAction::SendTimeout { uid } => {
                let SendRequest { on_timeout, .. } = state
                    .substate_mut::<TcpClientState>()
                    .take_send_request(&uid);

                dispatcher.dispatch_back(&on_timeout, uid)
            }
            TcpClientAction::SendError { uid, error } => {
                let SendRequest {
                    connection,
                    on_error,
                    ..
                } = state
                    .substate_mut::<TcpClientState>()
                    .take_send_request(&uid);

                dispatcher.dispatch_back(&on_error, (uid, error));
                dispatcher.dispatch(TcpAction::Close {
                    connection,
                    on_success: callback!(|connection: Uid| TcpClientAction::CloseEventNotify {
                        connection
                    }),
                })
            }
            TcpClientAction::Recv {
                uid,
                connection,
                count,
                timeout,
                on_success,
                on_timeout,
                on_error,
            } => {
                state
                    .substate_mut::<TcpClientState>()
                    .new_recv_request(&uid, connection, on_success, on_timeout, on_error);

                dispatcher.dispatch(TcpAction::Recv {
                    uid,
                    connection,
                    count,
                    timeout,
                    on_success: callback!(|(uid: Uid, data: Vec<u8>)| TcpClientAction::RecvSuccess { uid, data }),
                    on_timeout: callback!(|(uid: Uid, partial_data: Vec<u8>)| TcpClientAction::RecvTimeout { uid, partial_data }),
                    on_error: callback!(|(uid: Uid, error: String)| TcpClientAction::RecvError { uid, error }),
                });
            }
            TcpClientAction::RecvSuccess { uid, data } => {
                let RecvRequest { on_success, .. } = state
                    .substate_mut::<TcpClientState>()
                    .take_recv_request(&uid);

                dispatcher.dispatch_back(&on_success, (uid, data))
            }
            TcpClientAction::RecvTimeout { uid, partial_data } => {
                let RecvRequest { on_timeout, .. } = state
                    .substate_mut::<TcpClientState>()
                    .take_recv_request(&uid);

                dispatcher.dispatch_back(&on_timeout, (uid, partial_data))
            }
            TcpClientAction::RecvError { uid, error } => {
                let RecvRequest {
                    connection,
                    on_error,
                    ..
                } = state
                    .substate_mut::<TcpClientState>()
                    .take_recv_request(&uid);

                dispatcher.dispatch_back(&on_error, (uid, error));
                dispatcher.dispatch(TcpAction::Close {
                    connection,
                    on_success: callback!(|connection: Uid| TcpClientAction::CloseEventNotify {
                        connection
                    }),
                })
            }
        }
    }
}
