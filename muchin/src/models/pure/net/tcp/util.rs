use super::{
    action::{ConnectionEvent, Event, ListenerEvent, TcpPollEvents},
    state::{
        Connection, ConnectionStatus, ConnectionType, EventUpdater, RecvRequest, SendRequest,
        TcpState,
    },
};
use crate::{
    automaton::{Dispatcher, TimeoutAbsolute, Uid},
    callback,
    models::{
        effectful::mio::action::{MioEffectfulAction, MioEvent},
        pure::net::tcp::action::TcpAction,
    },
};

pub fn process_pending_connections(
    current_time: u128,
    tcp_state: &mut TcpState,
    dispatcher: &mut Dispatcher,
) {
    let mut purge_requests = Vec::new();

    for (
        &connection,
        Connection {
            status,
            conn_type,
            timeout,
            ..
        },
    ) in tcp_state.pending_connections_mut()
    {
        let timed_out = match timeout {
            TimeoutAbsolute::Millis(ms) => current_time >= *ms,
            TimeoutAbsolute::Never => false,
        };

        if timed_out {
            if let ConnectionType::Outgoing { on_timeout, .. } = conn_type {
                dispatcher.dispatch_back(&on_timeout, connection);
                purge_requests.push(connection);
            } else {
                unreachable!()
            }
        } else {
            match status {
                ConnectionStatus::Pending => {
                    dispatcher.dispatch_effect(MioEffectfulAction::TcpGetPeerAddress {
                        connection,
                        on_success: callback!(|(connection: Uid, address: String)| TcpAction::GetPeerAddressSuccess { connection, address }),
                        on_error: callback!(|(connection: Uid, error: String)| TcpAction::GetPeerAddressError { connection, error }),
                    });
                    *status = ConnectionStatus::PendingCheck;
                }
                ConnectionStatus::PendingCheck => (),
                _ => unreachable!(),
            }
        }
    }
}

pub fn process_pending_send_requests(
    current_time: u128,
    tcp_state: &mut TcpState,
    dispatcher: &mut Dispatcher,
) {
    let mut purge_requests = Vec::new();
    let mut dispatched_requests = Vec::new();

    process_pending_send_requests_aux(
        current_time,
        tcp_state,
        dispatcher,
        &mut purge_requests,
        &mut dispatched_requests,
    );

    // remove requests for invalid or closed connections
    for uid in purge_requests.iter() {
        tcp_state.remove_send_request(uid)
    }
}

pub fn process_pending_send_requests_aux(
    current_time: u128,
    tcp_state: &mut TcpState,
    dispatcher: &mut Dispatcher,
    purge_requests: &mut Vec<Uid>,
    dispatched_requests: &mut Vec<Uid>,
) {
    for (
        &uid,
        SendRequest {
            connection,
            data,
            bytes_sent,
            timeout,
            on_timeout,
            on_error,
            ..
        },
    ) in tcp_state.pending_send_requests()
    {
        let timed_out = match timeout {
            TimeoutAbsolute::Millis(ms) => current_time >= *ms,
            TimeoutAbsolute::Never => false,
        };
        let connection = *connection;
        let event = tcp_state.get_connection(&connection).events();

        match event {
            ConnectionEvent::Ready { can_send: true, .. } => {
                if timed_out {
                    dispatcher.dispatch_back(on_timeout, uid);
                    purge_requests.push(uid);
                } else {
                    dispatcher.dispatch_effect(MioEffectfulAction::TcpWrite {
                        uid,
                        connection,
                        data: (&data[*bytes_sent..]).into(),
                        on_success: callback!(|uid: Uid| TcpAction::SendSuccess { uid }),
                        on_success_partial: callback!(|(uid: Uid, count: usize)| TcpAction::SendSuccessPartial { uid, count }),
                        on_interrupted: callback!(|uid: Uid| TcpAction::SendErrorInterrupted { uid }),
                        on_would_block: callback!(|uid: Uid| TcpAction::SendErrorTryAgain { uid }),
                        on_error: callback!(|(uid: Uid, error: String)| TcpAction::SendError { uid, error })
                    });

                    dispatched_requests.push(uid);
                }
            }
            ConnectionEvent::Ready {
                can_send: false, ..
            } => {
                if timed_out {
                    dispatcher.dispatch_back(on_timeout, uid);
                    purge_requests.push(uid);
                }
            }
            ConnectionEvent::Closed => {
                dispatcher.dispatch_back(on_error, (uid, "Connection closed".to_string()));
                purge_requests.push(uid);
            }
            ConnectionEvent::Error => {
                dispatcher.dispatch_back(on_error, (uid, "Connection error".to_string()));
                purge_requests.push(uid);
            }
        }
    }
}

pub fn process_pending_recv_requests(
    current_time: u128,
    tcp_state: &mut TcpState,
    dispatcher: &mut Dispatcher,
) {
    let mut purge_requests = Vec::new();
    let mut dispatched_requests = Vec::new();

    input_pending_recv_requests_aux(
        current_time,
        tcp_state,
        dispatcher,
        &mut purge_requests,
        &mut dispatched_requests,
    );

    // remove requests for invalid or closed connections
    for uid in purge_requests.iter() {
        tcp_state.remove_recv_request(uid)
    }
}

pub fn input_pending_recv_requests_aux(
    current_time: u128,
    tcp_state: &mut TcpState,
    dispatcher: &mut Dispatcher,
    purge_requests: &mut Vec<Uid>,
    dispatched_requests: &mut Vec<Uid>,
) {
    for (
        &uid,
        RecvRequest {
            connection,
            buffered_data,
            remaining_bytes,
            timeout,
            on_timeout,
            on_error,
            ..
        },
    ) in tcp_state.pending_recv_requests()
    {
        let connection = *connection;
        let timed_out = match timeout {
            TimeoutAbsolute::Millis(ms) => current_time >= *ms,
            TimeoutAbsolute::Never => false,
        };
        let event = tcp_state.get_connection(&connection).events();

        match event {
            ConnectionEvent::Ready { can_recv: true, .. } => {
                if timed_out {
                    dispatcher.dispatch_back(on_timeout, (uid, buffered_data.clone()));
                    purge_requests.push(uid);
                } else {
                    dispatcher.dispatch_effect(MioEffectfulAction::TcpRead {
                        uid,
                        connection,
                        len: *remaining_bytes,
                        on_success: callback!(|(uid: Uid, data: Vec<u8>)| TcpAction::RecvSuccess { uid, data }),
                        on_success_partial: callback!(|(uid: Uid, partial_data: Vec<u8>)| TcpAction::RecvSuccessPartial { uid, partial_data }),
                        on_interrupted: callback!(|uid: Uid| TcpAction::RecvErrorInterrupted { uid }),
                        on_would_block: callback!(|uid: Uid| TcpAction::RecvErrorTryAgain { uid }),
                        on_error: callback!(|(uid: Uid, error: String)| TcpAction::RecvError { uid, error })
                    });

                    dispatched_requests.push(uid);
                }
            }
            ConnectionEvent::Ready {
                can_recv: false, ..
            } => {
                if timed_out {
                    dispatcher.dispatch_back(on_timeout, (uid, buffered_data.clone()));
                    purge_requests.push(uid);
                }
            }
            ConnectionEvent::Closed => {
                dispatcher.dispatch_back(on_error, (uid, "Connection closed".to_string()));
                purge_requests.push(uid);
            }
            ConnectionEvent::Error => {
                dispatcher.dispatch_back(on_error, (uid, "Connection error".to_string()));
                purge_requests.push(uid);
            }
        }
    }
}

pub fn handle_poll_success(
    tcp_state: &mut TcpState,
    dispatcher: &mut Dispatcher,
    current_time: u128,
    uid: Uid,
    events: Vec<MioEvent>,
) {
    // update TCP object events (even for Uids that were not requested)
    for mio_event in events.iter() {
        tcp_state.update_events(mio_event)
    }

    process_pending_connections(current_time, tcp_state, dispatcher);
    process_pending_send_requests(current_time, tcp_state, dispatcher);
    process_pending_recv_requests(current_time, tcp_state, dispatcher);

    let request = tcp_state.get_poll_request(&uid);
    // Collect events from state for the requested objects
    let events: TcpPollEvents = request
        .objects
        .iter()
        .filter_map(|uid| {
            tcp_state.get_events(uid).and_then(|(uid, event)| {
                if let Event::Listener(ListenerEvent::AllAccepted) = event {
                    None
                } else {
                    Some((uid, event))
                }
            })
        })
        .collect();

    dispatcher.dispatch_back(&request.on_success, (uid, events));
    tcp_state.remove_poll_request(&uid)
}

pub fn handle_send_common(
    tcp_state: &mut TcpState,
    dispatcher: &mut Dispatcher,
    current_time: u128,
    uid: Uid,
    can_send_value: bool,
) {
    let SendRequest {
        connection,
        timeout,
        on_timeout,
        ..
    } = tcp_state.get_send_request_mut(&uid);

    let timed_out = match *timeout {
        TimeoutAbsolute::Millis(ms) => current_time >= ms,
        TimeoutAbsolute::Never => false,
    };

    if timed_out {
        dispatcher.dispatch_back(on_timeout, uid);
        tcp_state.remove_send_request(&uid)
    } else {
        if can_send_value == false {
            tcp_state.get_send_request_mut(&uid).send_on_poll = true;
            return;
        }

        let connection = *connection;
        let conn = tcp_state.get_connection_mut(&connection);

        if conn.events.is_some() {
            let ConnectionEvent::Ready { can_send, .. } = conn.events_mut() else {
                unreachable!()
            };

            *can_send = can_send_value;
            dispatch_send(tcp_state, dispatcher, uid);
        } else {
            tcp_state.get_send_request_mut(&uid).send_on_poll = true;
        }
    }
}

pub fn handle_recv_common(
    tcp_state: &mut TcpState,
    dispatcher: &mut Dispatcher,
    current_time: u128,
    uid: Uid,
    can_recv_value: bool,
) {
    let RecvRequest {
        connection,
        buffered_data,
        timeout,
        on_timeout,
        ..
    } = tcp_state.get_recv_request_mut(&uid);

    let timed_out = match *timeout {
        TimeoutAbsolute::Millis(ms) => current_time >= ms,
        TimeoutAbsolute::Never => false,
    };

    if timed_out {
        dispatcher.dispatch_back(on_timeout, (uid, buffered_data.clone()));
        tcp_state.remove_recv_request(&uid)
    } else {
        if can_recv_value == false {
            tcp_state.get_recv_request_mut(&uid).recv_on_poll = true;
            return;
        }

        let connection = *connection;
        let conn = tcp_state.get_connection_mut(&connection);

        if conn.events.is_some() {
            let ConnectionEvent::Ready { can_recv, .. } = conn.events_mut() else {
                unreachable!()
            };

            *can_recv = can_recv_value;
            dispatch_recv(tcp_state, dispatcher, uid);
        } else {
            tcp_state.get_recv_request_mut(&uid).recv_on_poll = true;
        }
    }
}

pub fn dispatch_send(tcp_state: &mut TcpState, dispatcher: &mut Dispatcher, uid: Uid) {
    let connection = tcp_state.get_send_request(&uid).connection;
    let conn = tcp_state.get_connection(&connection);

    if conn.events.is_none() {
        tcp_state.get_send_request_mut(&uid).send_on_poll = true;
        return;
    }

    match conn.events() {
        ConnectionEvent::Ready { can_send: true, .. } => {
            let SendRequest {
                data, bytes_sent, ..
            } = tcp_state.get_send_request(&uid);

            dispatcher.dispatch_effect(MioEffectfulAction::TcpWrite {
                uid,
                connection,
                data: (&data[*bytes_sent..]).into(),
                on_success: callback!(|uid: Uid| TcpAction::SendSuccess { uid }),
                on_success_partial: callback!(|(uid: Uid, count: usize)| TcpAction::SendSuccessPartial { uid, count }),
                on_interrupted: callback!(|uid: Uid| TcpAction::SendErrorInterrupted { uid }),
                on_would_block: callback!(|uid: Uid| TcpAction::SendErrorTryAgain { uid }),
                on_error: callback!(|(uid: Uid, error: String)| TcpAction::SendError { uid, error })
            });
        }
        ConnectionEvent::Ready {
            can_send: false, ..
        } => tcp_state.get_send_request_mut(&uid).send_on_poll = true,
        ConnectionEvent::Closed => {
            dispatcher.dispatch_back(
                &tcp_state.get_send_request(&uid).on_error,
                (uid, "Connection closed".to_string()),
            );
            tcp_state.remove_send_request(&uid)
        }
        ConnectionEvent::Error => {
            dispatcher.dispatch_back(
                &tcp_state.get_send_request(&uid).on_error,
                (uid, "Connection error".to_string()),
            );
            tcp_state.remove_send_request(&uid)
        }
    };
}

pub fn dispatch_recv(tcp_state: &mut TcpState, dispatcher: &mut Dispatcher, uid: Uid) {
    let connection = tcp_state.get_recv_request(&uid).connection;
    let conn = tcp_state.get_connection(&connection);

    if conn.events.is_none() {
        tcp_state.get_recv_request_mut(&uid).recv_on_poll = true;
        return;
    }

    match conn.events() {
        ConnectionEvent::Ready { can_recv: true, .. } => {
            dispatcher.dispatch_effect(MioEffectfulAction::TcpRead {
                uid,
                connection,
                len: tcp_state.get_recv_request(&uid).remaining_bytes,
                on_success: callback!(|(uid: Uid, data: Vec<u8>)| TcpAction::RecvSuccess { uid, data }),
                on_success_partial: callback!(|(uid: Uid, partial_data: Vec<u8>)| TcpAction::RecvSuccessPartial { uid, partial_data }),
                on_interrupted: callback!(|uid: Uid| TcpAction::RecvErrorInterrupted { uid }),
                on_would_block: callback!(|uid: Uid| TcpAction::RecvErrorTryAgain { uid }),
                on_error: callback!(|(uid: Uid, error: String)| TcpAction::RecvError { uid, error })
            });
        }
        ConnectionEvent::Ready {
            can_recv: false, ..
        } => tcp_state.get_recv_request_mut(&uid).recv_on_poll = true,
        ConnectionEvent::Closed => {
            // Recv failed, notify caller
            dispatcher.dispatch_back(
                &tcp_state.get_recv_request_mut(&uid).on_error,
                (uid, "Connection closed".to_string()),
            );
            tcp_state.remove_recv_request(&uid)
        }
        ConnectionEvent::Error => {
            // Recv failed, notify caller
            dispatcher.dispatch_back(
                &tcp_state.get_recv_request_mut(&uid).on_error,
                (uid, "Connection error".to_string()),
            );
            tcp_state.remove_recv_request(&uid)
        }
    }
}
