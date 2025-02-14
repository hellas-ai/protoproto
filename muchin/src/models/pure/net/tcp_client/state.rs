use crate::automaton::{Objects, Redispatch, Uid};

#[derive(Debug)]
pub struct Connection {
    pub on_success: Redispatch<Uid>,
    pub on_timeout: Redispatch<Uid>,
    pub on_error: Redispatch<(Uid, String)>,
    pub on_close: Redispatch<Uid>,
}

#[derive(Debug)]
pub struct SendRequest {
    pub connection: Uid,
    pub on_success: Redispatch<Uid>,
    pub on_timeout: Redispatch<Uid>,
    pub on_error: Redispatch<(Uid, String)>,
}

#[derive(Debug)]
pub struct RecvRequest {
    pub connection: Uid,
    pub on_success: Redispatch<(Uid, Vec<u8>)>,
    pub on_timeout: Redispatch<(Uid, Vec<u8>)>,
    pub on_error: Redispatch<(Uid, String)>,
}

#[derive(Debug)]
pub struct TcpClientState {
    pub connections: Objects<Connection>,
    pub send_requests: Objects<SendRequest>,
    pub recv_requests: Objects<RecvRequest>,
}

impl TcpClientState {
    pub fn new() -> Self {
        Self {
            connections: Objects::<Connection>::new(),
            send_requests: Objects::<SendRequest>::new(),
            recv_requests: Objects::<RecvRequest>::new(),
        }
    }
    pub fn get_connection(&self, connection: &Uid) -> &Connection {
        self.connections
            .get(connection)
            .expect(&format!("Connection object {:?} not found", connection))
    }

    pub fn new_connection(
        &mut self,
        connection: Uid,
        on_success: Redispatch<Uid>,
        on_timeout: Redispatch<Uid>,
        on_error: Redispatch<(Uid, String)>,
        on_close: Redispatch<Uid>,
    ) {
        if self
            .connections
            .insert(
                connection,
                Connection {
                    on_success,
                    on_timeout,
                    on_error,
                    on_close,
                },
            )
            .is_some()
        {
            panic!("Attempt to re-use existing connection {:?}", connection)
        }
    }

    pub fn remove_connection(&mut self, connection: &Uid) {
        self.connections.remove(connection).expect(&format!(
            "Attempt to remove an inexistent connection {:?}",
            connection
        ));
    }

    pub fn new_send_request(
        &mut self,
        uid: &Uid,
        connection: Uid,
        on_success: Redispatch<Uid>,
        on_timeout: Redispatch<Uid>,
        on_error: Redispatch<(Uid, String)>,
    ) {
        if self
            .send_requests
            .insert(
                *uid,
                SendRequest {
                    connection,
                    on_success,
                    on_timeout,
                    on_error,
                },
            )
            .is_some()
        {
            panic!("Attempt to re-use existing SendRequest {:?}", uid)
        }
    }

    pub fn take_send_request(&mut self, uid: &Uid) -> SendRequest {
        self.send_requests
            .remove(uid)
            .expect(&format!("Take attempt on inexistent SendRequest {:?}", uid))
    }

    pub fn new_recv_request(
        &mut self,
        uid: &Uid,
        connection: Uid,
        on_success: Redispatch<(Uid, Vec<u8>)>,
        on_timeout: Redispatch<(Uid, Vec<u8>)>,
        on_error: Redispatch<(Uid, String)>,
    ) {
        if self
            .recv_requests
            .insert(
                *uid,
                RecvRequest {
                    connection,
                    on_success,
                    on_timeout,
                    on_error,
                },
            )
            .is_some()
        {
            panic!("Attempt to re-use existing {:?}", uid)
        }
    }

    pub fn take_recv_request(&mut self, uid: &Uid) -> RecvRequest {
        self.recv_requests
            .remove(uid)
            .expect(&format!("Take attempt on inexistent RecvRequest {:?}", uid))
    }
}
