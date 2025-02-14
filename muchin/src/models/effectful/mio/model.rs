use super::action::{
    MioEffectfulAction, PollResult, TcpAcceptResult, TcpReadResult, TcpWriteResult,
};
use super::state::MioState;
use crate::automaton::{
    Dispatcher, Effectful, EffectfulModel, ModelState, RegisterModel, RunnerBuilder,
};

// The `MioState` struct, implementing the `EffectfulModel` trait, provides the
// interface layer between the state-machine and the MIO crate for asynchronous
// I/O operations.
//
// It includes a set of operations for handling I/O actions such as:
// - Creating new poll objects for monitoring numerous I/O events.
// - Registering/deregistering TCP servers and connections with poll objects.
// - Polling events for asynchronous I/O notifications.
// - Managing TCP connections, including listening for, accepting, and
//   establishing connections, closing active connections, and reading/writing
//   data over established TCP connections.
//
// Each of these operations corresponds to a variant in `MioAction`.
// The `process_effectful` function handles these actions by invoking the
// appropriate function in `MioState`, and dispatches the result back as a
// caller-defined `PureAction`.

impl RegisterModel for MioState {
    fn register<Substate: ModelState>(builder: RunnerBuilder<Substate>) -> RunnerBuilder<Substate> {
        builder.model_effectful(Effectful::<Self>(Self::new()))
    }
}

impl EffectfulModel for MioState {
    type Action = MioEffectfulAction;

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
            MioEffectfulAction::PollRegisterTcpServer {
                poll,
                listener,
                on_success,
                on_error,
            } => {
                let result = if dispatcher.is_replayer() {
                    Ok(()) // Ignored
                } else {
                    self.poll_register_tcp_server(&poll, listener)
                };

                match result {
                    Ok(_) => dispatcher.dispatch_back(&on_success, listener),
                    Err(error) => dispatcher.dispatch_back(&on_error, (listener, error)),
                }
            }
            MioEffectfulAction::PollRegisterTcpConnection {
                poll,
                connection,
                on_success,
                on_error,
            } => {
                let result = if dispatcher.is_replayer() {
                    Ok(()) // Ignored
                } else {
                    self.poll_register_tcp_connection(&poll, connection)
                };

                match result {
                    Ok(_) => dispatcher.dispatch_back(&on_success, connection),
                    Err(error) => dispatcher.dispatch_back(&on_error, (connection, error)),
                }
            }
            MioEffectfulAction::PollDeregisterTcpConnection {
                poll,
                connection,
                on_success,
                on_error,
            } => {
                let result = if dispatcher.is_replayer() {
                    Ok(()) // Ignored
                } else {
                    self.poll_deregister_tcp_connection(&poll, connection)
                };

                match result {
                    Ok(_) => dispatcher.dispatch_back(&on_success, connection),
                    Err(error) => dispatcher.dispatch_back(&on_error, (connection, error)),
                }
            }
            MioEffectfulAction::PollEvents {
                uid,
                poll,
                events,
                timeout,
                on_success,
                on_interrupted,
                on_error,
            } => {
                let result = if dispatcher.is_replayer() {
                    PollResult::Events(Vec::new()) // Ignored
                } else {
                    self.poll_events(&poll, &events, timeout)
                };
                match result {
                    PollResult::Events(events) => {
                        dispatcher.dispatch_back(&on_success, (uid, events))
                    }
                    PollResult::Interrupted => dispatcher.dispatch_back(&on_interrupted, uid),
                    PollResult::Error(error) => dispatcher.dispatch_back(&on_error, (uid, error)),
                }
            }
            MioEffectfulAction::EventsCreate {
                uid,
                capacity,
                on_success,
            } => {
                if !dispatcher.is_replayer() {
                    self.events_create(uid, capacity);
                }

                dispatcher.dispatch_back(&on_success, uid);
            }
            MioEffectfulAction::TcpListen {
                listener,
                address,
                on_success,
                on_error,
            } => {
                let result = if dispatcher.is_replayer() {
                    Ok(()) // Ignored
                } else {
                    self.tcp_listen(listener, address)
                };

                match result {
                    Ok(_) => dispatcher.dispatch_back(&on_success, listener),
                    Err(error) => dispatcher.dispatch_back(&on_error, (listener, error)),
                }
            }
            MioEffectfulAction::TcpAccept {
                connection,
                listener,
                on_success,
                on_would_block,
                on_error,
            } => {
                let result = if dispatcher.is_replayer() {
                    TcpAcceptResult::Success // Ignored
                } else {
                    self.tcp_accept(connection, &listener)
                };

                match result {
                    TcpAcceptResult::Success => dispatcher.dispatch_back(&on_success, connection),
                    TcpAcceptResult::WouldBlock => {
                        dispatcher.dispatch_back(&on_would_block, connection)
                    }
                    TcpAcceptResult::Error(error) => {
                        dispatcher.dispatch_back(&on_error, (connection, error))
                    }
                }
            }
            MioEffectfulAction::TcpConnect {
                connection,
                address,
                on_success,
                on_error,
            } => {
                let result = if dispatcher.is_replayer() {
                    Ok(()) // Ignored
                } else {
                    self.tcp_connect(connection, address)
                };

                match result {
                    Ok(_) => dispatcher.dispatch_back(&on_success, connection),
                    Err(error) => dispatcher.dispatch_back(&on_error, (connection, error)),
                }
            }
            MioEffectfulAction::TcpClose {
                connection,
                on_success,
            } => {
                if !dispatcher.is_replayer() {
                    self.tcp_close(&connection);
                }

                dispatcher.dispatch_back(&on_success, connection);
            }
            MioEffectfulAction::TcpWrite {
                uid,
                connection: connection_uid,
                data,
                on_success,
                on_success_partial,
                on_interrupted,
                on_would_block,
                on_error,
            } => {
                let result = if dispatcher.is_replayer() {
                    TcpWriteResult::WrittenAll // Ignored
                } else {
                    self.tcp_write(&connection_uid, &data)
                };

                match result {
                    TcpWriteResult::WrittenAll => dispatcher.dispatch_back(&on_success, uid),
                    TcpWriteResult::WrittenPartial(count) => {
                        dispatcher.dispatch_back(&on_success_partial, (uid, count))
                    }
                    TcpWriteResult::Interrupted => dispatcher.dispatch_back(&on_interrupted, uid),
                    TcpWriteResult::WouldBlock => dispatcher.dispatch_back(&on_would_block, uid),
                    TcpWriteResult::Error(error) => {
                        dispatcher.dispatch_back(&on_error, (uid, error))
                    }
                }
            }
            MioEffectfulAction::TcpRead {
                uid,
                connection,
                len,
                on_success,
                on_success_partial,
                on_interrupted,
                on_would_block,
                on_error,
            } => {
                let result = if dispatcher.is_replayer() {
                    TcpReadResult::ReadAll(Vec::new()) // Ignored
                } else {
                    self.tcp_read(&connection, len)
                };
                match result {
                    TcpReadResult::ReadAll(data) => {
                        dispatcher.dispatch_back(&on_success, (uid, data))
                    }
                    TcpReadResult::ReadPartial(partial_data) => {
                        dispatcher.dispatch_back(&on_success_partial, (uid, partial_data))
                    }
                    TcpReadResult::Interrupted => dispatcher.dispatch_back(&on_interrupted, uid),
                    TcpReadResult::WouldBlock => dispatcher.dispatch_back(&on_would_block, uid),
                    TcpReadResult::Error(error) => {
                        dispatcher.dispatch_back(&on_error, (uid, error))
                    }
                }
            }
            MioEffectfulAction::TcpGetPeerAddress {
                connection,
                on_success,
                on_error,
            } => {
                let result = if dispatcher.is_replayer() {
                    Ok(String::new()) // Ignored
                } else {
                    self.tcp_peer_address(&connection)
                };

                match result {
                    Ok(address) => dispatcher.dispatch_back(&on_success, (connection, address)),
                    Err(error) => dispatcher.dispatch_back(&on_error, (connection, error)),
                }
            }
        }
    }
}
