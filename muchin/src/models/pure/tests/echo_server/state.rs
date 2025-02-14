use crate::automaton::{Objects, Uid};
use core::panic;

#[derive(Debug)]
pub enum Connection {
    Ready,
    Receiving { request: Uid },
    Sending { request: Uid },
}

#[derive(Debug)]
pub struct EchoServerConfig {
    pub address: String,
    pub max_connections: usize,
    pub poll_timeout: u64,
    pub recv_timeout: u64,
}

#[derive(Debug)]
pub enum EchoServerStatus {
    Init,
    Listening { connections: Objects<Connection> },
}

#[derive(Debug)]
pub struct EchoServerState {
    pub status: EchoServerStatus,
    pub config: EchoServerConfig,
}

impl EchoServerState {
    pub fn from_config(config: EchoServerConfig) -> Self {
        Self {
            status: EchoServerStatus::Init,
            config,
        }
    }

    pub fn new_connection(&mut self, connection: Uid) {
        if let EchoServerStatus::Listening { connections } = &mut self.status {
            if connections.insert(connection, Connection::Ready).is_some() {
                panic!("Attempt to re-insert existing Connection {:?}", connection)
            }
        } else {
            unreachable!()
        }
    }

    pub fn remove_connection(&mut self, connection: &Uid) {
        if let EchoServerStatus::Listening { connections } = &mut self.status {
            connections.remove(connection).expect(&format!(
                "Attempt to remove an inexistent Connection {:?}",
                connection
            ));
        } else {
            unreachable!()
        }
    }

    pub fn get_connection_mut(&mut self, connection: &Uid) -> &mut Connection {
        if let EchoServerStatus::Listening { connections } = &mut self.status {
            connections
                .get_mut(&connection)
                .expect(&format!("Connection {:?} not found", connection))
        } else {
            unreachable!()
        }
    }

    pub fn connections_ready_to_recv(&self) -> Vec<Uid> {
        if let EchoServerStatus::Listening { connections } = &self.status {
            connections
                .iter()
                .filter_map(|kv| match kv {
                    (connection, Connection::Ready) => Some(*connection),
                    _ => None,
                })
                .collect()
        } else {
            unreachable!()
        }
    }

    pub fn find_connection_uid_by_recv_uid(&self, uid: Uid) -> Uid {
        if let EchoServerStatus::Listening { connections } = &self.status {
            let (connection, _) = connections
                .iter()
                .find(|kv| match kv {
                    (_, Connection::Receiving { request }) => *request == uid,
                    _ => false,
                })
                .expect(&format!("Connection not found for recv {:?}", uid));

            *connection
        } else {
            unreachable!()
        }
    }

    pub fn find_connection_uid_by_send_uid(&self, uid: Uid) -> Uid {
        if let EchoServerStatus::Listening { connections } = &self.status {
            let (connection, _) = connections
                .iter()
                .find(|kv| match kv {
                    (_, Connection::Sending { request }) => *request == uid,
                    _ => false,
                })
                .expect(&format!("Connection not found for send {:?}", uid));

            *connection
        } else {
            unreachable!()
        }
    }
}
